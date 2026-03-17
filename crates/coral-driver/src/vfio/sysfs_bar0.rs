// SPDX-License-Identifier: AGPL-3.0-only
//! Safe wrapper for sysfs BAR0 mmap reads.
//!
//! Consolidates the mmap → volatile-read → munmap pattern used by
//! multiple oracle modules into a single safe API with bounds checking.

use std::ptr::NonNull;

/// Read-only mmap of a PCI BAR0 resource via sysfs.
///
/// Provides safe, bounds-checked volatile reads for register probing.
/// The mapping is automatically unmapped on drop.
pub struct SysfsBar0 {
    ptr: NonNull<u8>,
    size: usize,
    _file: std::fs::File,
}

/// 16 MiB — standard BAR0 size for NVIDIA Volta-class GPUs.
pub const DEFAULT_BAR0_SIZE: usize = 16 * 1024 * 1024;

// SAFETY: The mmap region is read-only and the file descriptor is held
// alive for the lifetime of the mapping. Volatile reads are atomic for
// aligned u32 on x86/aarch64.
unsafe impl Send for SysfsBar0 {}
unsafe impl Sync for SysfsBar0 {}

impl SysfsBar0 {
    /// Open and mmap a PCI device's BAR0 via sysfs `resource0`.
    ///
    /// # Errors
    ///
    /// Returns an error if the sysfs path cannot be opened or mmap fails.
    pub fn open(bdf: &str, size: usize) -> Result<Self, String> {
        let path = format!("/sys/bus/pci/devices/{bdf}/resource0");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(&path)
            .map_err(|e| format!("cannot open {path}: {e}"))?;

        // SAFETY: mmap of a sysfs PCI resource file with read-only protection.
        // The file descriptor is kept alive in the struct.
        let raw = unsafe {
            rustix::mm::mmap(
                std::ptr::null_mut(),
                size,
                rustix::mm::ProtFlags::READ,
                rustix::mm::MapFlags::SHARED,
                &file,
                0,
            )
        }
        .map_err(|e| format!("mmap {path} failed: {e}"))?;

        let ptr = NonNull::new(raw.cast::<u8>()).ok_or_else(|| "mmap returned null".to_owned())?;

        Ok(Self {
            ptr,
            size,
            _file: file,
        })
    }

    /// Read a 32-bit register at the given byte offset.
    ///
    /// Returns `0` if the offset is out of bounds.
    #[must_use]
    pub fn read_u32(&self, offset: usize) -> u32 {
        if offset + 4 > self.size {
            return 0;
        }
        // SAFETY: bounds-checked above; volatile read of a valid mmap region.
        unsafe { std::ptr::read_volatile(self.ptr.as_ptr().add(offset).cast::<u32>()) }
    }

    /// The size of the mapped BAR0 region in bytes.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }
}

impl Drop for SysfsBar0 {
    fn drop(&mut self) {
        // SAFETY: unmapping the region we mapped in `open`.
        unsafe {
            let _ = rustix::mm::munmap(self.ptr.as_ptr().cast(), self.size);
        }
    }
}
