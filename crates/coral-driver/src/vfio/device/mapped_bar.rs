// SPDX-License-Identifier: AGPL-3.0-or-later
//! MMIO BAR mapping for VFIO devices.

use crate::error::DriverError;
use crate::gsp::{ApplyError, RegisterAccess};
use crate::mmio_region::MmioRegion;

use std::borrow::Cow;

/// A mapped BAR region from a VFIO device.
///
/// ## Thread safety (`Send` / `Sync`)
///
/// The region wraps a `MmioRegion` whose pointer refers to a `MAP_SHARED` MMIO
/// mapping tied to the VFIO device fd lifetime. Access is performed only through
/// volatile operations (`read_u32` / `write_u32`), which are safe to use from
/// multiple threads for aligned 32-bit MMIO on supported architectures when the
/// mapping is shared read-only or callers coordinate writes. The owning struct is
/// therefore `Send` + `Sync` for the same reasons as other mmap-backed BAR
/// wrappers in this crate.
pub struct MappedBar {
    pub(super) region: MmioRegion,
}

impl MappedBar {
    /// Read a 32-bit register at the given byte offset.
    ///
    /// # Errors
    ///
    /// Returns error if offset is out of range or not 4-byte aligned.
    pub fn read_u32(&self, offset: usize) -> Result<u32, DriverError> {
        if !offset.is_multiple_of(4) {
            return Err(DriverError::MmapFailed(Cow::Owned(format!(
                "BAR offset {offset:#x} is not 4-byte aligned"
            ))));
        }
        self.region.read_u32(offset)
    }

    /// Write a 32-bit register at the given byte offset.
    ///
    /// # Errors
    ///
    /// Returns error if offset is out of range or not 4-byte aligned.
    pub fn write_u32(&self, offset: usize, value: u32) -> Result<(), DriverError> {
        if !offset.is_multiple_of(4) {
            return Err(DriverError::MmapFailed(Cow::Owned(format!(
                "BAR offset {offset:#x} is not 4-byte aligned"
            ))));
        }
        self.region.write_u32(offset, value)
    }

    /// Size of this BAR region in bytes.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.region.len()
    }

    /// Apply a GR init sequence's BAR0 writes.
    ///
    /// Implements the `RegisterAccess` trait bridge so the GSP applicator
    /// can write directly through the VFIO-mapped BAR0.
    pub fn apply_gr_bar0_writes(&self, writes: &[(u32, u32)]) -> (usize, usize) {
        let mut applied = 0;
        let mut failed = 0;
        for &(offset, value) in writes {
            if self.write_u32(offset as usize, value).is_ok() {
                applied += 1;
            } else {
                failed += 1;
            }
        }
        (applied, failed)
    }

    /// Raw pointer to the BAR base (for callers that need ptr arithmetic).
    #[must_use]
    pub const fn base_ptr(&self) -> *mut u8 {
        self.region.as_ptr()
    }
}

impl RegisterAccess for MappedBar {
    fn read_u32(&self, offset: u32) -> Result<u32, ApplyError> {
        self.read_u32(offset as usize)
            .map_err(|e| ApplyError::MmioFailed {
                offset,
                detail: e.to_string(),
            })
    }

    fn write_u32(&mut self, offset: u32, value: u32) -> Result<(), ApplyError> {
        MappedBar::write_u32(self, offset as usize, value).map_err(|e| ApplyError::MmioFailed {
            offset,
            detail: e.to_string(),
        })
    }
}

// SAFETY: Matches the `Send` / `Sync` rationale in the [`MappedBar`] docs.
unsafe impl Send for MappedBar {}

// SAFETY: Matches the `Send` / `Sync` rationale in the [`MappedBar`] docs.
unsafe impl Sync for MappedBar {}
