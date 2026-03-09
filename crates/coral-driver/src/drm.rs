// SPDX-License-Identifier: AGPL-3.0-only
//! Pure Rust DRM ioctl interface — uses `rustix` for mmap/munmap, `libc` for ioctl.
//!
//! All ioctl numbers and structures are defined here from the Linux
//! kernel headers (GPL-2.0-only) via clean-room constant extraction.
//!
//! Memory mapping uses `rustix::mm` (safe wrappers, no raw unsafe).
//! ioctl remains on `libc` until rustix stabilises a generic typed
//! ioctl helper. See also `amd/ioctl.rs`, `nv/ioctl.rs`.

use crate::error::{DriverError, DriverResult};
use std::fs::{File, OpenOptions};
use std::os::unix::io::{AsRawFd, RawFd};
use std::ptr::NonNull;

/// DRM ioctl direction flags.
const _IOC_NONE: u32 = 0;
const IOC_WRITE: u32 = 1;
const IOC_READ: u32 = 2;

const IOC_NRBITS: u32 = 8;
const IOC_TYPEBITS: u32 = 8;
const IOC_SIZEBITS: u32 = 14;

const IOC_NRSHIFT: u32 = 0;
const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS;
const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS;
const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS;

const DRM_IOCTL_BASE: u32 = b'd' as u32;

/// Construct a DRM ioctl number.
const fn drm_ioctl(dir: u32, nr: u32, size: u32) -> u64 {
    ((dir << IOC_DIRSHIFT)
        | (DRM_IOCTL_BASE << IOC_TYPESHIFT)
        | (nr << IOC_NRSHIFT)
        | (size << IOC_SIZESHIFT)) as u64
}

const fn _drm_io(nr: u32) -> u64 {
    drm_ioctl(_IOC_NONE, nr, 0)
}

const fn drm_iowr(nr: u32, size: u32) -> u64 {
    drm_ioctl(IOC_READ | IOC_WRITE, nr, size)
}

const fn drm_iow(nr: u32, size: u32) -> u64 {
    drm_ioctl(IOC_WRITE, nr, size)
}

const fn _drm_ior(nr: u32, size: u32) -> u64 {
    drm_ioctl(IOC_READ, nr, size)
}

/// `DRM_IOCTL_VERSION`
pub const DRM_IOCTL_VERSION: u64 = drm_iowr(0x00, 32);

/// `DRM_IOCTL_GEM_CLOSE` (generic, not vendor-specific)
pub const DRM_IOCTL_GEM_CLOSE: u64 = drm_iow(0x09, 8);

/// Argument for `DRM_IOCTL_GEM_CLOSE`.
#[repr(C)]
#[derive(Default)]
pub struct DrmGemClose {
    pub handle: u32,
    pub pad: u32,
}

/// Public helper for submodules to construct IOWR ioctl numbers.
#[must_use]
pub const fn drm_iowr_pub(nr: u32, size: u32) -> u64 {
    drm_iowr(nr, size)
}

/// Public helper for submodules to construct IOW ioctl numbers.
#[must_use]
pub const fn drm_iow_pub(nr: u32, size: u32) -> u64 {
    drm_iow(nr, size)
}

/// DRM version info returned by the kernel.
#[repr(C)]
#[derive(Debug, Default)]
pub struct DrmVersion {
    pub version_major: i32,
    pub version_minor: i32,
    pub version_patchlevel: i32,
    pub name_len: u64,
    pub name: u64,
    pub date_len: u64,
    pub date: u64,
    pub desc_len: u64,
    pub desc: u64,
}

// ---------------------------------------------------------------------------
// Unified memory-mapped region (used by both AMD and NV backends)
// ---------------------------------------------------------------------------

/// RAII wrapper around a memory-mapped region. Unmaps on drop.
///
/// Consolidates `mmap`/`munmap`/`from_raw_parts` into a single safe abstraction
/// used by both AMD (GEM) and NVIDIA (nouveau) backends.
pub(crate) struct MappedRegion {
    ptr: NonNull<u8>,
    len: usize,
}

impl MappedRegion {
    /// Map a file descriptor region into memory using `rustix::mm::mmap`.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::MmapFailed`] if mmap fails.
    pub(crate) fn new(
        len: usize,
        prot: rustix::mm::ProtFlags,
        flags: rustix::mm::MapFlags,
        fd: RawFd,
        offset: u64,
    ) -> DriverResult<Self> {
        if len == 0 {
            return Err(DriverError::MmapFailed("mmap length must be > 0".into()));
        }
        use std::os::unix::io::BorrowedFd;
        // SAFETY: `fd` is a valid open DRM GEM handle, `offset` is from the
        // kernel (gem_mmap_offset or map_handle), and `len` > 0. The mapped
        // region is owned by this struct and unmapped in `Drop`.
        let ptr = unsafe {
            rustix::mm::mmap(
                std::ptr::null_mut(),
                len,
                prot,
                flags,
                BorrowedFd::borrow_raw(fd),
                offset,
            )
        }
        .map_err(|e| DriverError::MmapFailed(format!("mmap failed: {e}").into()))?;
        // mmap succeeded — rustix returns Err on MAP_FAILED, so ptr is non-null.
        let ptr = NonNull::new(ptr.cast::<u8>())
            .ok_or_else(|| DriverError::MmapFailed("mmap returned null".into()))?;
        Ok(Self { ptr, len })
    }

    /// View the mapped region as a byte slice.
    #[must_use]
    pub(crate) fn as_slice(&self) -> &[u8] {
        // SAFETY: ptr is valid for self.len bytes from a successful mmap.
        // The region lives as long as self (Drop handles munmap).
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    /// View the mapped region as a mutable byte slice.
    #[must_use]
    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: ptr was mmap'd with PROT_READ | PROT_WRITE (for write ops).
        // We have exclusive access via &mut self.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    /// Bounds-checked subslice. Returns the region [offset..offset+len].
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::MmapFailed`] if the range is out of bounds.
    pub(crate) fn slice_at(&self, offset: usize, len: usize) -> DriverResult<&[u8]> {
        let end = offset
            .checked_add(len)
            .ok_or_else(|| DriverError::MmapFailed("slice range overflow".into()))?;
        if end > self.len {
            return Err(DriverError::MmapFailed(
                format!(
                    "slice out of bounds: offset={offset}, len={len}, region_len={}",
                    self.len
                )
                .into(),
            ));
        }
        Ok(&self.as_slice()[offset..end])
    }

    /// Bounds-checked mutable subslice. Returns the region [offset..offset+len].
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::MmapFailed`] if the range is out of bounds.
    pub(crate) fn slice_at_mut(&mut self, offset: usize, len: usize) -> DriverResult<&mut [u8]> {
        let end = offset
            .checked_add(len)
            .ok_or_else(|| DriverError::MmapFailed("slice range overflow".into()))?;
        if end > self.len {
            return Err(DriverError::MmapFailed(
                format!(
                    "slice out of bounds: offset={offset}, len={len}, region_len={}",
                    self.len
                )
                .into(),
            ));
        }
        Ok(&mut self.as_mut_slice()[offset..end])
    }
}

impl Drop for MappedRegion {
    fn drop(&mut self) {
        // SAFETY: ptr was returned by a successful rustix::mm::mmap call.
        // len is the same value passed to mmap. This runs exactly once.
        unsafe {
            let _ = rustix::mm::munmap(self.ptr.as_ptr().cast::<std::ffi::c_void>(), self.len);
        }
    }
}

/// Metadata about a discovered DRM render node.
#[derive(Debug, Clone)]
pub struct DrmDeviceInfo {
    /// Render node path (e.g. `/dev/dri/renderD128`).
    pub path: String,
    /// Kernel driver name (e.g. `"amdgpu"`, `"nouveau"`, `"nvidia"`).
    pub driver: String,
    /// DRM driver major version.
    pub version_major: i32,
    /// DRM driver minor version.
    pub version_minor: i32,
}

/// A DRM render node file descriptor.
pub struct DrmDevice {
    file: File,
    pub path: String,
}

impl DrmDevice {
    /// Open a DRM render node.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the device file cannot be opened.
    pub fn open(path: &str) -> DriverResult<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Ok(Self {
            file,
            path: path.to_string(),
        })
    }

    /// Open the first available render node.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::DeviceNotFound`] if no DRM render node exists.
    pub fn open_default() -> DriverResult<Self> {
        for idx in 128..=191 {
            let path = format!("/dev/dri/renderD{idx}");
            if let Ok(dev) = Self::open(&path) {
                return Ok(dev);
            }
        }
        Err(DriverError::DeviceNotFound(
            "no DRM render node found".into(),
        ))
    }

    /// Open the first render node matching a specific driver name.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::DeviceNotFound`] if no render node with the
    /// specified driver is found.
    pub fn open_by_driver(driver_name: &str) -> DriverResult<Self> {
        for info in enumerate_render_nodes() {
            if info.driver == driver_name {
                return Self::open(&info.path);
            }
        }
        Err(DriverError::DeviceNotFound(
            format!("no DRM render node with driver '{driver_name}'").into(),
        ))
    }

    /// Raw file descriptor for ioctl calls.
    #[must_use]
    pub fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    /// Query the DRM driver name.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the version ioctl fails.
    pub fn driver_name(&self) -> DriverResult<String> {
        let (_ver, name) = drm_version(self.fd())?;
        Ok(name)
    }

    /// Query full device info (driver name + version).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the version ioctl fails.
    pub fn device_info(&self) -> DriverResult<DrmDeviceInfo> {
        let (ver, name) = drm_version(self.fd())?;
        Ok(DrmDeviceInfo {
            path: self.path.clone(),
            driver: name,
            version_major: ver.version_major,
            version_minor: ver.version_minor,
        })
    }
}

/// Enumerate all available DRM render nodes with their driver info.
///
/// Scans `/dev/dri/renderD128` through `renderD191` and returns metadata
/// for every node that can be opened and queried. Nodes that fail to open
/// (permissions, no device) are silently skipped.
#[must_use]
pub fn enumerate_render_nodes() -> Vec<DrmDeviceInfo> {
    let mut devices = Vec::new();
    for idx in 128..=191 {
        let path = format!("/dev/dri/renderD{idx}");
        if let Ok(dev) = DrmDevice::open(&path)
            && let Ok(info) = dev.device_info()
        {
            devices.push(info);
        }
    }
    devices
}

/// Close a GEM buffer object. Safe wrapper around `DRM_IOCTL_GEM_CLOSE`.
pub(crate) fn gem_close(fd: RawFd, handle: u32) -> DriverResult<()> {
    let mut args = DrmGemClose { handle, pad: 0 };
    // SAFETY: DrmGemClose is #[repr(C)] matching kernel's drm_gem_close (8 bytes).
    // Stack-allocated, synchronous ioctl.
    unsafe { drm_ioctl_named(fd, DRM_IOCTL_GEM_CLOSE, &mut args, "gem_close") }
}

/// Query the DRM driver version. Safe wrapper around `DRM_IOCTL_VERSION`.
pub(crate) fn drm_version(fd: RawFd) -> DriverResult<(DrmVersion, String)> {
    let mut name_buf = [0u8; 64];
    let mut ver = DrmVersion {
        name_len: name_buf.len() as u64,
        name: name_buf.as_mut_ptr() as u64,
        ..Default::default()
    };
    // SAFETY: DrmVersion is #[repr(C)] matching kernel's drm_version struct.
    // name_buf is stack-allocated and outlives the synchronous ioctl.
    unsafe { drm_ioctl_named(fd, DRM_IOCTL_VERSION, &mut ver, "drm_version")? };
    let len = usize::try_from(ver.name_len)
        .unwrap_or(name_buf.len())
        .min(name_buf.len());
    let name = String::from_utf8_lossy(&name_buf[..len])
        .trim_end_matches('\0')
        .to_string();
    Ok((ver, name))
}

/// Perform a DRM ioctl on a `#[repr(C)]` structure.
///
/// # Safety
///
/// The caller must ensure `T` is the correct `#[repr(C)]` struct for `request`.
///
/// # Errors
///
/// Returns [`DriverError::IoctlFailed`] if the kernel returns an error.
pub(crate) unsafe fn drm_ioctl_typed<T>(fd: RawFd, request: u64, arg: &mut T) -> DriverResult<()> {
    // SAFETY: caller guarantees T matches the ioctl request.
    unsafe { drm_ioctl_named(fd, request, arg, "drm_ioctl") }
}

/// Like `drm_ioctl_typed` but with a custom name for error messages.
///
/// # Safety
///
/// The caller must ensure `T` is the correct `#[repr(C)]` struct for `request`.
pub(crate) unsafe fn drm_ioctl_named<T>(
    fd: RawFd,
    request: u64,
    arg: &mut T,
    name: &'static str,
) -> DriverResult<()> {
    // SAFETY: The caller guarantees `T` is the correct `#[repr(C)]` kernel
    // struct for `request`. `arg` is a valid mutable reference (non-null,
    // aligned, initialized). `libc::ioctl` performs a synchronous syscall —
    // the pointer does not escape the call.
    let ret = unsafe { libc::ioctl(fd, request as libc::c_ulong, std::ptr::from_mut::<T>(arg)) };
    if ret < 0 {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(-1);
        return Err(DriverError::IoctlFailed { name, errno });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_are_consistent() {
        assert_eq!(DRM_IOCTL_VERSION & 0xFF, 0);
    }

    #[test]
    fn drm_device_not_found_on_invalid_path() {
        let result = DrmDevice::open("/dev/dri/renderD999");
        assert!(result.is_err());
    }

    #[test]
    fn enumerate_render_nodes_returns_vec() {
        let nodes = enumerate_render_nodes();
        // May be empty in CI without GPUs, but should not panic.
        for info in &nodes {
            assert!(!info.path.is_empty());
            assert!(!info.driver.is_empty());
        }
    }

    #[test]
    fn drm_device_info_has_driver_and_path() {
        let info = DrmDeviceInfo {
            path: "/dev/dri/renderD128".to_string(),
            driver: "amdgpu".to_string(),
            version_major: 3,
            version_minor: 57,
        };
        let debug = format!("{info:?}");
        assert!(debug.contains("amdgpu"));
        assert!(debug.contains("renderD128"));
    }

    #[test]
    fn open_by_driver_nonexistent_fails() {
        let result = DrmDevice::open_by_driver("nonexistent_drm_driver_xyz");
        assert!(result.is_err());
    }
}
