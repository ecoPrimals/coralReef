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

/// IOVA base for user DMA allocations — above all fixed allocations
/// (including the 2 MiB SLM pool at `SLM_IOVA`).
pub(super) const USER_IOVA_BASE: u64 = 0x30_0000;

/// Guard page IOVA — absorbs spurious firmware DMA (e.g. FECS/PMU accessing
/// IOVA 0x200 during boot on K80). Without this mapping, such DMA causes
/// IO_PAGE_FAULT which triggers an IOMMU device reset mid-operation.
pub(super) const GUARD_PAGE_IOVA: u64 = 0x0;

/// GPFIFO ring IOVA.
pub(super) const GPFIFO_IOVA: u64 = 0x1000;

/// USERD page IOVA.
pub(super) const USERD_IOVA: u64 = 0x2000;

/// Semaphore fence value buffer IOVA (Blackwell+).
pub(super) const FENCE_BUF_IOVA: u64 = 0x8_0000;

/// Semaphore fence push buffer IOVA (Blackwell+).
pub(super) const FENCE_PB_IOVA: u64 = 0x9_0000;

/// SLM (shader local memory) pool IOVA — backing for `SET_SHADER_LOCAL_MEMORY`.
pub(super) const SLM_IOVA: u64 = 0xA_0000;

/// SLM pool size (2 MiB — matches UVM path; supports up to 64 TPCs at 32 KiB each).
pub(super) const SLM_SIZE: usize = 2 * 1024 * 1024;

/// SLM stride per TPC (32 KiB — standard for all supported generations).
pub(super) const SLM_PER_TPC: u64 = 0x8000;

/// Local memory window address for Volta+ (SM >= 70).
#[expect(
    dead_code,
    reason = "WIP: VFIO QMD codegen — local-memory window bases (exported for tests/host tools)"
)]
pub const LOCAL_MEM_WINDOW_VOLTA: u64 = 0xFF00_0000_0000_0000;

/// Local memory window address for pre-Volta (SM < 70).
#[expect(
    dead_code,
    reason = "WIP: VFIO QMD codegen — local-memory window bases (exported for tests/host tools)"
)]
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
        FENCE_BUF_IOVA, FENCE_PB_IOVA, GPFIFO_IOVA, LOCAL_MEM_WINDOW_LEGACY,
        LOCAL_MEM_WINDOW_VOLTA, SLM_IOVA, SLM_SIZE, USER_IOVA_BASE, USERD_IOVA, gpfifo,
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
        const { assert!(USERD_IOVA + 4096 <= FENCE_BUF_IOVA) };
        const { assert!(FENCE_BUF_IOVA + 4096 <= FENCE_PB_IOVA) };
        const { assert!(FENCE_PB_IOVA + 4096 <= SLM_IOVA) };
        const { assert!(SLM_IOVA + SLM_SIZE as u64 <= USER_IOVA_BASE) };
    }

    #[test]
    fn fence_iovas_page_aligned() {
        assert_eq!(
            FENCE_BUF_IOVA % 4096,
            0,
            "fence buf IOVA must be page-aligned"
        );
        assert_eq!(
            FENCE_PB_IOVA % 4096,
            0,
            "fence pb IOVA must be page-aligned"
        );
    }

    #[test]
    fn fence_iovas_distinct() {
        assert_ne!(FENCE_BUF_IOVA, FENCE_PB_IOVA);
        assert_ne!(FENCE_BUF_IOVA, GPFIFO_IOVA);
        assert_ne!(FENCE_PB_IOVA, USERD_IOVA);
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
