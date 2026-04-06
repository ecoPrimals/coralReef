// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test-only BAR0-backed [`crate::gsp::RegisterAccess`] for unit tests.

use super::applicator::{ApplyError, RegisterAccess};

/// Heap-backed fake BAR0: byte buffer with little-endian `u32` MMIO semantics.
pub(crate) struct MockBar0 {
    data: Vec<u8>,
}

impl MockBar0 {
    /// Allocate a zero-filled BAR of `size` bytes.
    #[must_use]
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0u8; size],
        }
    }

    /// Write a little-endian `u32` at byte offset `offset` (for seeding reads).
    pub fn seed_u32(&mut self, offset: u32, value: u32) {
        let offset = usize::try_from(offset).expect("offset fits usize");
        if offset
            .checked_add(4)
            .is_none_or(|end| end > self.data.len())
        {
            panic!(
                "MockBar0::seed_u32: offset {offset:#x} + 4 out of range (len {})",
                self.data.len()
            );
        }
        self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}

impl RegisterAccess for MockBar0 {
    fn read_u32(&self, off_u32: u32) -> Result<u32, ApplyError> {
        if !off_u32.is_multiple_of(4) {
            return Err(ApplyError::MmioFailed {
                offset: off_u32,
                detail: "offset is not 4-byte aligned".into(),
            });
        }
        let offset = usize::try_from(off_u32).map_err(|_| ApplyError::MmioFailed {
            offset: off_u32,
            detail: "offset does not fit usize".into(),
        })?;
        if offset
            .checked_add(4)
            .is_none_or(|end| end > self.data.len())
        {
            return Err(ApplyError::MmioFailed {
                offset: off_u32,
                detail: format!("read past end (size {})", self.data.len()),
            });
        }
        Ok(u32::from_le_bytes([
            self.data[offset],
            self.data[offset + 1],
            self.data[offset + 2],
            self.data[offset + 3],
        ]))
    }

    fn write_u32(&mut self, off_u32: u32, value: u32) -> Result<(), ApplyError> {
        if !off_u32.is_multiple_of(4) {
            return Err(ApplyError::MmioFailed {
                offset: off_u32,
                detail: "offset is not 4-byte aligned".into(),
            });
        }
        let offset = usize::try_from(off_u32).map_err(|_| ApplyError::MmioFailed {
            offset: off_u32,
            detail: "offset does not fit usize".into(),
        })?;
        if offset
            .checked_add(4)
            .is_none_or(|end| end > self.data.len())
        {
            return Err(ApplyError::MmioFailed {
                offset: off_u32,
                detail: format!("write past end (size {})", self.data.len()),
            });
        }
        self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod mock_bar0_tests {
    use super::*;

    #[test]
    fn roundtrip_seed_and_read() {
        let mut bar = MockBar0::new(64);
        bar.seed_u32(0, 0x1122_3344);
        assert_eq!(bar.read_u32(0).unwrap(), 0x1122_3344);
    }

    #[test]
    fn write_then_read_offset_16() {
        let mut bar = MockBar0::new(128);
        bar.write_u32(16, 0xAAAA_BBBB).unwrap();
        assert_eq!(bar.read_u32(16).unwrap(), 0xAAAA_BBBB);
    }

    #[test]
    fn read_oob_fails() {
        let bar = MockBar0::new(8);
        assert!(bar.read_u32(8).is_err());
    }
}
