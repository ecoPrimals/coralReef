// SPDX-License-Identifier: AGPL-3.0-only
//! Pure Rust DRM ioctl interface — uses `libc` for syscalls, no `drm-sys` or `nix`.
//!
//! All ioctl numbers and structures are defined here from the Linux
//! kernel headers (GPL-2.0-only) via clean-room constant extraction.
//! Syscalls go through `libc` for cross-architecture portability.

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
    /// Map a file descriptor region into memory.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::MmapFailed`] if mmap returns `MAP_FAILED`.
    pub(crate) fn new(
        len: usize,
        prot: libc::c_int,
        flags: libc::c_int,
        fd: RawFd,
        offset: libc::off_t,
    ) -> DriverResult<Self> {
        if len == 0 {
            return Err(DriverError::MmapFailed("mmap length must be > 0".into()));
        }
        // SAFETY: Standard POSIX mmap. Arguments are validated by the caller:
        // `len` > 0, `fd` is a valid open DRM GEM handle, `offset` is from
        // the kernel (gem_mmap_offset or map_handle). The kernel returns
        // MAP_FAILED on error (checked below).
        let ptr = unsafe { libc::mmap(std::ptr::null_mut(), len, prot, flags, fd, offset) };
        if ptr == libc::MAP_FAILED {
            return Err(DriverError::MmapFailed("mmap returned MAP_FAILED".into()));
        }
        // SAFETY: We just verified ptr != MAP_FAILED, so it is non-null.
        // The region is valid for `len` bytes as a byte slice.
        let ptr = unsafe { NonNull::new_unchecked(ptr.cast::<u8>()) };
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
        // SAFETY: ptr was returned by a successful mmap call (MAP_FAILED
        // was checked in `new`). len is the same value passed to mmap.
        // NonNull<u8> and *mut c_void have the same representation for
        // munmap. This runs exactly once via the Drop impl.
        unsafe { libc::munmap(self.ptr.as_ptr().cast::<libc::c_void>(), self.len) };
    }
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
        // DRM_IOCTL_VERSION should be _IOWR('d', 0x00, struct drm_version)
        // On x86_64: direction=3 (R|W), type='d'=100, nr=0, size=32
        assert_eq!(DRM_IOCTL_VERSION & 0xFF, 0);
    }

    #[test]
    fn drm_device_not_found_on_invalid_path() {
        let result = DrmDevice::open("/dev/dri/renderD999");
        assert!(result.is_err());
    }
}
