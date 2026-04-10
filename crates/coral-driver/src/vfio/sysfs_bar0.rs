// SPDX-License-Identifier: AGPL-3.0-or-later
//! Safe wrapper for sysfs BAR0 mmap reads.
//!
//! Consolidates the mmap → volatile-read → munmap pattern used by
//! multiple oracle modules into a single safe API with bounds checking.

use crate::error::ChannelError;
use crate::mmio_region::MmioRegion;

/// Read-only mmap of a PCI BAR0 resource via sysfs.
///
/// Provides safe, bounds-checked volatile reads for register probing.
/// The mapping is automatically unmapped on drop.
///
/// ## Thread safety (`Send` / `Sync`)
///
/// The [`std::fs::File`] keeps the sysfs mapping alive; `MmioRegion`
/// holds the base pointer and length. Read-only volatile `u32` loads are safe to
/// share across threads for aligned MMIO access on the supported platforms, in line
/// with other BAR0 readers in this crate.
pub struct SysfsBar0 {
    _file: std::fs::File,
    region: MmioRegion,
}

/// 16 MiB — standard BAR0 size for NVIDIA Volta-class GPUs.
pub const DEFAULT_BAR0_SIZE: usize = 16 * 1024 * 1024;

// SAFETY: Matches the `Send` / `Sync` rationale in the [`SysfsBar0`] docs.
unsafe impl Send for SysfsBar0 {}

// SAFETY: Matches the `Send` / `Sync` rationale in the [`SysfsBar0`] docs.
unsafe impl Sync for SysfsBar0 {}

impl SysfsBar0 {
    /// Open and mmap a PCI device's BAR0 via sysfs `resource0`.
    ///
    /// # Errors
    ///
    /// Returns an error if the sysfs path cannot be opened or mmap fails.
    pub fn open(bdf: &str, size: usize) -> Result<Self, ChannelError> {
        let path = crate::linux_paths::sysfs_pci_device_file(bdf, "resource0");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(&path)
            .map_err(|e| ChannelError::resource_io("open", path.clone(), e))?;

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
        .map_err(|e| ChannelError::Bar0Mmap {
            path: path.clone(),
            source: e,
        })?;

        if raw.is_null() {
            return Err(ChannelError::Bar0MmapNull { path });
        }

        // SAFETY: `raw`/`size` come from the successful `mmap` above.
        let region = unsafe { MmioRegion::new(raw.cast::<u8>(), size) };

        Ok(Self {
            _file: file,
            region,
        })
    }

    /// Read a 32-bit register at the given byte offset.
    ///
    /// Returns `0` if the offset is out of bounds.
    #[must_use]
    pub fn read_u32(&self, offset: usize) -> u32 {
        if offset
            .checked_add(4)
            .is_none_or(|end| end > self.region.len())
        {
            return 0;
        }
        self.region.read_u32(offset).unwrap_or_default()
    }

    /// The size of the mapped BAR0 region in bytes.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.region.len()
    }
}
