// SPDX-License-Identifier: AGPL-3.0-or-later
//! IOVA layout and GPFIFO encoding constants for VFIO compute.

use crate::error::DriverError;
use crate::gsp::ApplyError;

use std::borrow::Cow;

pub(super) fn apply_error_to_driver(e: ApplyError) -> DriverError {
    DriverError::MmapFailed(Cow::Owned(e.to_string()))
}

/// BAR0 register offsets for NVIDIA GPU.
pub(super) mod bar0_reg {
    /// Boot0 register — chip identification.
    pub const BOOT0: usize = 0x0000_0000;
}

/// GPFIFO configuration constants.
pub mod gpfifo {
    /// Number of GPFIFO entries (must be power of 2).
    pub const ENTRIES: usize = 128;
    /// Size of each GPFIFO entry in bytes.
    pub const ENTRY_SIZE: usize = 8;
    /// Total GPFIFO ring size in bytes.
    pub const RING_SIZE: usize = ENTRIES * ENTRY_SIZE;

    /// Encode a GPFIFO indirect-buffer entry (NVB06F GP_ENTRY format).
    pub fn encode_entry(gpu_addr: u64, len_bytes: u32) -> u64 {
        let lo = gpu_addr & 0xFFFF_FFFC;
        let hi_addr = (gpu_addr >> 32) & 0xFF;
        let len_dwords = u64::from(len_bytes / 4);
        let hi = hi_addr | (len_dwords << 10);
        lo | (hi << 32)
    }
}

/// IOVA base for user DMA allocations — above GPFIFO/USERD.
pub(super) const USER_IOVA_BASE: u64 = 0x10_0000;

/// GPFIFO ring IOVA.
pub(super) const GPFIFO_IOVA: u64 = 0x1000;

/// USERD page IOVA.
pub(super) const USERD_IOVA: u64 = 0x2000;

/// Local memory window address for Volta+ (SM >= 70).
pub const LOCAL_MEM_WINDOW_VOLTA: u64 = 0xFF00_0000_0000_0000;

/// Local memory window address for pre-Volta (SM < 70).
pub const LOCAL_MEM_WINDOW_LEGACY: u64 = 0xFF00_0000;

/// Map SM version to chip codename for firmware lookup.
///
/// Delegates to [`crate::nv::identity::chip_name`] — single source of truth.
pub const fn sm_to_chip(sm: u32) -> &'static str {
    crate::nv::identity::chip_name(sm)
}

#[cfg(test)]
mod tests {
    use super::{
        GPFIFO_IOVA, LOCAL_MEM_WINDOW_LEGACY, LOCAL_MEM_WINDOW_VOLTA, USER_IOVA_BASE, USERD_IOVA,
        gpfifo,
    };

    #[test]
    fn gpfifo_entry_encoding() {
        let addr = 0x1000_u64;
        let size = 64_u32;
        let entry = gpfifo::encode_entry(addr, size);
        let dw0 = entry as u32;
        assert_eq!(dw0, 0x1000, "DW0 = addr with type=0");
        let dw1 = (entry >> 32) as u32;
        let len_field = (dw1 >> 10) & 0x1F_FFFF;
        assert_eq!(len_field, 16, "length = 16 dwords");
        let recovered = (dw0 as u64 & 0xFFFF_FFFC) | ((dw1 as u64 & 0xFF) << 32);
        assert_eq!(recovered, addr);
    }

    #[test]
    fn gpfifo_entry_zero() {
        assert_eq!(gpfifo::encode_entry(0, 0), 0);
    }

    #[test]
    fn gpfifo_ring_size() {
        assert_eq!(gpfifo::RING_SIZE, 128 * 8);
    }

    #[test]
    fn gpfifo_entry_large_addr() {
        let addr = 0x10_0000_0000_u64;
        let size = 256_u32;
        let entry = gpfifo::encode_entry(addr, size);
        let dw0 = entry as u32;
        let dw1 = (entry >> 32) as u32;
        let recovered = (dw0 as u64 & 0xFFFF_FFFC) | ((dw1 as u64 & 0xFF) << 32);
        assert_eq!(recovered, addr);
        let len_field = (dw1 >> 10) & 0x1F_FFFF;
        assert_eq!(len_field, 64, "length = 64 dwords");
    }

    #[test]
    fn iova_constants_non_overlapping() {
        const { assert!(GPFIFO_IOVA < USERD_IOVA) };
        const { assert!(USERD_IOVA + 4096 <= USER_IOVA_BASE) };
    }

    #[test]
    fn local_mem_window_volta() {
        assert_eq!(LOCAL_MEM_WINDOW_VOLTA, 0xFF00_0000_0000_0000);
    }

    #[test]
    fn local_mem_window_legacy() {
        assert_eq!(LOCAL_MEM_WINDOW_LEGACY, 0xFF00_0000);
    }
}
