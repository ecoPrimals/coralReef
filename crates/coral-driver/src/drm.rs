// SPDX-License-Identifier: AGPL-3.0-only
//! Pure Rust DRM ioctl interface — uses `libc` for syscalls, no `drm-sys` or `nix`.
//!
//! All ioctl numbers and structures are defined here from the Linux
//! kernel headers (GPL-2.0-only) via clean-room constant extraction.
//! Syscalls go through `libc` for cross-architecture portability.

use crate::error::{DriverError, DriverResult};
use std::fs::{File, OpenOptions};
use std::os::unix::io::{AsRawFd, RawFd};

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
    ///
    /// # Panics
    ///
    /// Panics if the name buffer length exceeds `u64::MAX` (impossible in
    /// practice — the buffer is 64 bytes).
    pub fn driver_name(&self) -> DriverResult<String> {
        let mut name_buf = [0u8; 64];
        let mut ver = DrmVersion {
            name_len: u64::try_from(name_buf.len()).expect("buffer len fits in u64"),
            name: name_buf.as_mut_ptr() as u64,
            ..Default::default()
        };
        // Safety: DrmVersion is #[repr(C)] and matches the kernel ioctl struct.
        unsafe { drm_ioctl_typed(self.fd(), DRM_IOCTL_VERSION, &mut ver)? };
        let len = usize::try_from(ver.name_len).unwrap_or(0);
        let len = len.min(name_buf.len());
        Ok(String::from_utf8_lossy(&name_buf[..len])
            .trim_end_matches('\0')
            .to_string())
    }
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
    // libc::ioctl expects c_ulong for the request on Linux.
    let ret = unsafe { libc::ioctl(fd, request as libc::c_ulong, std::ptr::from_mut::<T>(arg)) };
    if ret < 0 {
        return Err(DriverError::IoctlFailed {
            name: "drm_ioctl",
            errno: -ret,
        });
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
