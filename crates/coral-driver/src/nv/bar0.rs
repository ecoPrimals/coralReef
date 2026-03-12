// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign BAR0 MMIO access — direct GPU register read/write via sysfs.
//!
//! Maps `/sys/class/drm/{node}/device/resource0` (or an explicit PCI sysfs
//! path) to perform volatile 32-bit register operations. This is the same
//! physical BAR0 window that toadStool's `nvpmu::Bar0Access` uses.
//!
//! Requires root or appropriate PCI sysfs permissions.
//!
//! # Safety model
//!
//! BAR0 writes affect real hardware state. Incorrect register writes can
//! hang the GPU, corrupt display, or require a reboot. This module is used
//! exclusively for well-known init sequences parsed from NVIDIA firmware
//! blobs by [`crate::gsp::firmware_parser`].

use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use std::ptr::NonNull;

use crate::gsp::{ApplyError, RegisterAccess};

/// GPU BAR0 MMIO mapping for direct register access.
///
/// Wraps an mmap of the PCI BAR0 resource file. All reads and writes are
/// volatile, matching hardware MMIO semantics.
pub struct Bar0Access {
    ptr: NonNull<u8>,
    size: usize,
    _file: std::fs::File,
}

impl std::fmt::Debug for Bar0Access {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bar0Access")
            .field("size", &self.size)
            .field("ptr", &self.ptr)
            .finish()
    }
}

impl Bar0Access {
    /// Open BAR0 from a DRM render node path (e.g. `/dev/dri/renderD128`).
    ///
    /// Resolves the sysfs device directory and maps `resource0`.
    pub fn from_render_node(render_node_path: &str) -> Result<Self, ApplyError> {
        let node_name = render_node_path.rsplit('/').next().ok_or(ApplyError::MmioFailed {
            offset: 0,
            detail: format!("cannot parse render node from '{render_node_path}'"),
        })?;
        let sysfs_device = format!("/sys/class/drm/{node_name}/device");
        Self::from_sysfs_device(&sysfs_device)
    }

    /// Open BAR0 from a sysfs device directory (e.g. `/sys/class/drm/renderD128/device`).
    pub fn from_sysfs_device(sysfs_device: &str) -> Result<Self, ApplyError> {
        let path = format!("{sysfs_device}/resource0");
        Self::open_resource(&path)
    }

    /// Open BAR0 from an explicit resource file path.
    fn open_resource(path: &str) -> Result<Self, ApplyError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| ApplyError::MmioFailed {
                offset: 0,
                detail: format!("open {path}: {e}"),
            })?;

        let size = file
            .metadata()
            .map_err(|e| ApplyError::MmioFailed {
                offset: 0,
                detail: format!("stat {path}: {e}"),
            })?
            .len() as usize;

        if size == 0 {
            return Err(ApplyError::MmioFailed {
                offset: 0,
                detail: format!("{path}: BAR0 resource has zero size"),
            });
        }

        // SAFETY: resource0 is a PCI BAR sysfs file. mmap with SHARED gives
        // direct MMIO access to GPU registers. The file descriptor stays open
        // for the lifetime of the mapping.
        let raw_ptr = unsafe {
            rustix::mm::mmap(
                std::ptr::null_mut(),
                size,
                rustix::mm::ProtFlags::READ | rustix::mm::ProtFlags::WRITE,
                rustix::mm::MapFlags::SHARED,
                std::os::unix::io::BorrowedFd::borrow_raw(file.as_raw_fd()),
                0,
            )
        }
        .map_err(|e| ApplyError::MmioFailed {
            offset: 0,
            detail: format!("mmap {path} ({size} bytes): {e}"),
        })?;

        let ptr = NonNull::new(raw_ptr.cast::<u8>()).ok_or(ApplyError::MmioFailed {
            offset: 0,
            detail: format!("mmap {path}: returned null"),
        })?;

        tracing::info!(
            path,
            size_mib = size / (1024 * 1024),
            "BAR0 MMIO mapped for sovereign register access"
        );

        Ok(Self {
            ptr,
            size,
            _file: file,
        })
    }

    /// BAR0 mapping size in bytes.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }

    /// Read a GPU identification register (NV_PMC_BOOT_0 at offset 0x0).
    ///
    /// Returns the chip ID word. Useful for verifying BAR0 access works.
    pub fn read_boot_id(&self) -> Result<u32, ApplyError> {
        self.read_u32(0)
    }
}

impl RegisterAccess for Bar0Access {
    fn read_u32(&self, offset: u32) -> Result<u32, ApplyError> {
        let off = offset as usize;
        if off + 4 > self.size {
            return Err(ApplyError::MmioFailed {
                offset,
                detail: format!(
                    "offset {off:#x} + 4 exceeds BAR0 size {:#x}",
                    self.size
                ),
            });
        }
        // SAFETY: ptr is a valid mmap of BAR0. Offset is bounds-checked.
        // Volatile read is required for MMIO semantics.
        let val = unsafe {
            let p = self.ptr.as_ptr().add(off).cast::<u32>();
            std::ptr::read_volatile(p)
        };
        Ok(val)
    }

    fn write_u32(&mut self, offset: u32, value: u32) -> Result<(), ApplyError> {
        let off = offset as usize;
        if off + 4 > self.size {
            return Err(ApplyError::MmioFailed {
                offset,
                detail: format!(
                    "offset {off:#x} + 4 exceeds BAR0 size {:#x}",
                    self.size
                ),
            });
        }
        // SAFETY: ptr is a valid mmap of BAR0. Offset is bounds-checked.
        // Volatile write is required for MMIO semantics.
        unsafe {
            let p = self.ptr.as_ptr().add(off).cast::<u32>();
            std::ptr::write_volatile(p, value);
        }
        Ok(())
    }
}

impl Drop for Bar0Access {
    fn drop(&mut self) {
        // SAFETY: ptr was returned by a successful mmap in open_resource().
        // Size matches the original mmap length. Drop runs exactly once.
        unsafe {
            let _ = rustix::mm::munmap(self.ptr.as_ptr().cast::<std::ffi::c_void>(), self.size);
        }
    }
}

// SAFETY: Bar0Access owns the mmap. The file descriptor and mapping are
// thread-safe when accessed through &mut self (write) or &self (read).
// Volatile operations provide the necessary ordering for MMIO.
unsafe impl Send for Bar0Access {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar0_nonexistent_path_fails() {
        let result = Bar0Access::from_render_node("/dev/dri/renderD999");
        assert!(result.is_err());
    }

    #[test]
    fn bar0_parse_node_name() {
        let result = Bar0Access::from_render_node("/dev/dri/renderD128");
        // Will fail without root/permissions, but should parse the path correctly
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("resource0") || err.contains("renderD128"),
            "error should reference the sysfs path: {err}"
        );
    }

    #[test]
    #[ignore = "requires root and NVIDIA GPU"]
    fn bar0_read_boot_id() {
        let bar0 = Bar0Access::from_render_node("/dev/dri/renderD128")
            .expect("BAR0 access (needs root)");
        let boot_id = bar0.read_boot_id().expect("read NV_PMC_BOOT_0");
        eprintln!("NV_PMC_BOOT_0 = {boot_id:#010x}");
        assert_ne!(boot_id, 0, "boot ID should not be zero");
        assert_ne!(boot_id, 0xFFFF_FFFF, "boot ID should not be all-ones (unmapped)");
    }
}
