// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign BAR0 MMIO access — direct GPU register read/write via sysfs.
//!
//! Maps `/sys/class/drm/{node}/device/resource0` (or an explicit PCI sysfs
//! path) to perform volatile 32-bit register operations. This is the same
//! physical BAR0 window used by ecosystem PMU/init tooling.
//!
//! Requires root or appropriate PCI sysfs permissions.
//!
//! # Safety model
//!
//! BAR0 writes affect real hardware state. Incorrect register writes can
//! hang the GPU, corrupt display, or require a reboot. This module is used
//! exclusively for well-known init sequences parsed from NVIDIA firmware
//! blobs by the `gsp::firmware_parser` module.

use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use std::ptr::NonNull;

use crate::gsp::{ApplyError, RegisterAccess};
use crate::mmio::VolatilePtr;

// ── Well-known BAR0 register offsets ────────────────────────────────
// Canonical source for FECS/GPCCS/PMC offsets used by ember, glowplug,
// and diagnostics. Avoids scattering magic numbers across crates.

/// FECS Falcon CPU control register.
pub const FECS_CPUCTL: u32 = 0x0040_9100;
/// FECS Falcon secure control register.
pub const FECS_SCTL: u32 = 0x0040_9240;
/// FECS Falcon program counter.
pub const FECS_PC: u32 = 0x0040_9030;
/// FECS Falcon mailbox 0.
pub const FECS_MB0: u32 = 0x0040_9040;
/// FECS Falcon mailbox 1.
pub const FECS_MB1: u32 = 0x0040_9044;
/// FECS Falcon exception cause.
pub const FECS_EXCI: u32 = 0x0040_904C;

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
            .finish_non_exhaustive()
    }
}

impl Bar0Access {
    /// Open BAR0 from a DRM render node path (e.g. `/dev/dri/renderD128`).
    ///
    /// Resolves the sysfs device directory and maps `resource0`.
    ///
    /// # Errors
    ///
    /// Returns `ApplyError::MmioFailed` if the render node path cannot be parsed
    /// or if opening/mapping the BAR0 resource fails.
    pub fn from_render_node(render_node_path: &str) -> Result<Self, ApplyError> {
        let node_name =
            render_node_path
                .rsplit('/')
                .next()
                .ok_or_else(|| ApplyError::MmioFailed {
                    offset: 0,
                    detail: format!("cannot parse render node from '{render_node_path}'"),
                })?;
        let sysfs_device = crate::linux_paths::sysfs_class_drm_device(node_name);
        Self::from_sysfs_device(&sysfs_device)
    }

    /// Open BAR0 from a sysfs device directory (e.g. `/sys/class/drm/renderD128/device`).
    ///
    /// # Errors
    ///
    /// Returns `ApplyError::MmioFailed` if opening/mapping the BAR0 resource fails.
    pub fn from_sysfs_device(sysfs_device: &str) -> Result<Self, ApplyError> {
        let path = format!("{sysfs_device}/resource0");
        Self::open_resource(&path)
    }

    /// Open BAR0 read-only from an explicit resource file path.
    ///
    /// Suitable for diagnostics and preflight checks where no register
    /// writes are needed. Uses `PROT_READ`-only mmap.
    pub fn open_resource_readonly(path: &str) -> Result<Self, ApplyError> {
        let file =
            OpenOptions::new()
                .read(true)
                .open(path)
                .map_err(|e| ApplyError::MmioFailed {
                    offset: 0,
                    detail: format!("open {path}: {e}"),
                })?;
        Self::mmap_file(file, path, rustix::mm::ProtFlags::READ)
    }

    /// Open BAR0 read-write from an explicit resource file path.
    pub fn open_resource(path: &str) -> Result<Self, ApplyError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| ApplyError::MmioFailed {
                offset: 0,
                detail: format!("open {path}: {e}"),
            })?;
        let this = Self::mmap_file(
            file,
            path,
            rustix::mm::ProtFlags::READ | rustix::mm::ProtFlags::WRITE,
        )?;
        tracing::info!(
            path,
            size_mib = this.size / (1024 * 1024),
            "BAR0 MMIO mapped for sovereign register access"
        );
        Ok(this)
    }

    /// Shared mmap logic for both read-only and read-write BAR0 mappings.
    fn mmap_file(
        file: std::fs::File,
        path: &str,
        prot: rustix::mm::ProtFlags,
    ) -> Result<Self, ApplyError> {
        let size = usize::try_from(
            file.metadata()
                .map_err(|e| ApplyError::MmioFailed {
                    offset: 0,
                    detail: format!("stat {path}: {e}"),
                })?
                .len(),
        )
        .map_err(|_| ApplyError::MmioFailed {
            offset: 0,
            detail: format!("{path}: BAR0 size exceeds usize"),
        })?;

        if size == 0 {
            return Err(ApplyError::MmioFailed {
                offset: 0,
                detail: format!("{path}: BAR0 resource has zero size"),
            });
        }

        // SAFETY: resource0 is a PCI BAR sysfs file. mmap with SHARED gives
        // direct MMIO access to GPU registers. file.as_raw_fd() is valid (open
        // File); BorrowedFd::borrow_raw requires valid fd for the call duration.
        let raw_ptr = unsafe {
            rustix::mm::mmap(
                std::ptr::null_mut(),
                size,
                prot,
                rustix::mm::MapFlags::SHARED,
                std::os::unix::io::BorrowedFd::borrow_raw(file.as_raw_fd()),
                0,
            )
        }
        .map_err(|e| ApplyError::MmioFailed {
            offset: 0,
            detail: format!("mmap {path} ({size} bytes): {e}"),
        })?;

        let ptr = NonNull::new(raw_ptr.cast::<u8>()).ok_or_else(|| ApplyError::MmioFailed {
            offset: 0,
            detail: format!("mmap {path}: returned null"),
        })?;

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

    /// Read a GPU identification register (`NV_PMC_BOOT_0` at offset 0x0).
    ///
    /// Returns the chip ID word. Useful for verifying BAR0 access works.
    ///
    /// # Errors
    ///
    /// Returns `ApplyError::MmioFailed` if the read fails.
    pub fn read_boot_id(&self) -> Result<u32, ApplyError> {
        self.read_u32(0)
    }
}

impl RegisterAccess for Bar0Access {
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "MMIO register access at known-aligned offsets"
    )]
    fn read_u32(&self, offset: u32) -> Result<u32, ApplyError> {
        let off = offset as usize;
        if off + 4 > self.size {
            return Err(ApplyError::MmioFailed {
                offset,
                detail: format!("offset {off:#x} + 4 exceeds BAR0 size {:#x}", self.size),
            });
        }
        // SAFETY: ptr is a valid mmap of BAR0. Offset is bounds-checked.
        // Volatile read is required for MMIO semantics.
        let vol = unsafe { VolatilePtr::new(self.ptr.as_ptr().add(off).cast::<u32>()) };
        Ok(vol.read())
    }

    #[expect(
        clippy::cast_ptr_alignment,
        reason = "MMIO register access at known-aligned offsets"
    )]
    fn write_u32(&mut self, offset: u32, value: u32) -> Result<(), ApplyError> {
        let off = offset as usize;
        if off + 4 > self.size {
            return Err(ApplyError::MmioFailed {
                offset,
                detail: format!("offset {off:#x} + 4 exceeds BAR0 size {:#x}", self.size),
            });
        }
        // SAFETY: ptr is a valid mmap of BAR0. Offset is bounds-checked.
        // Volatile write is required for MMIO semantics.
        let vol = unsafe { VolatilePtr::new(self.ptr.as_ptr().add(off).cast::<u32>()) };
        vol.write(value);
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

// SAFETY: ptr points to mmap'd BAR0 MMIO; lifetime tied to _file (keeps mapping
// valid). All access is via VolatilePtr (atomic for aligned u32 on x86/aarch64).
// Bar0Access is used across async/thread boundaries for GSP init.
unsafe impl Send for Bar0Access {}

// SAFETY: Same as Send — ptr valid while _file lives; volatile access is
// thread-safe for aligned 32-bit MMIO; no interior mutability of the pointer.
unsafe impl Sync for Bar0Access {}

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
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("resource0") || err.contains("renderD128"),
            "error should reference the sysfs path: {err}"
        );
    }

    #[test]
    fn open_resource_readonly_nonexistent_fails() {
        let result = Bar0Access::open_resource_readonly("/nonexistent/resource0");
        assert!(result.is_err());
    }

    #[test]
    fn open_resource_readonly_zero_size_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("resource0");
        std::fs::write(&path, b"").expect("write empty file");
        let result = Bar0Access::open_resource_readonly(path.to_str().unwrap());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("zero size"), "should mention zero size: {err}");
    }

    #[test]
    fn fecs_register_offsets_are_in_pgraph_range() {
        for offset in [
            FECS_CPUCTL,
            FECS_SCTL,
            FECS_PC,
            FECS_MB0,
            FECS_MB1,
            FECS_EXCI,
        ] {
            assert!(
                (0x0040_0000..0x0050_0000).contains(&offset),
                "FECS offset {offset:#010x} should be in PGRAPH BAR0 range"
            );
        }
    }

    #[test]
    fn fecs_register_offsets_are_aligned() {
        for offset in [
            FECS_CPUCTL,
            FECS_SCTL,
            FECS_PC,
            FECS_MB0,
            FECS_MB1,
            FECS_EXCI,
        ] {
            assert_eq!(
                offset % 4,
                0,
                "FECS offset {offset:#010x} must be 4-byte aligned"
            );
        }
    }

    #[test]
    #[ignore = "requires root and NVIDIA GPU"]
    fn bar0_read_boot_id() {
        let bar0 =
            Bar0Access::from_render_node("/dev/dri/renderD128").expect("BAR0 access (needs root)");
        let boot_id = bar0.read_boot_id().expect("read NV_PMC_BOOT_0");
        tracing::debug!(boot_id = format!("{boot_id:#010x}"), "NV_PMC_BOOT_0");
        assert_ne!(boot_id, 0, "boot ID should not be zero");
        assert_ne!(
            boot_id, 0xFFFF_FFFF,
            "boot ID should not be all-ones (unmapped)"
        );
    }
}
