// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pure IOVA layout and BAR0 encoding values for [`super::VfioChannel`] setup.
//!
//! [`ChannelLayout::compute`] has no I/O — it only derives the fixed infrastructure
//! IOVAs, fault-buffer PFN fields, MMU invalidate encoding, and GV100 runlist
//! register values used by `VfioChannel::create_with_config`.

use super::registers::{
    FAULT_BUF_IOVA, INSTANCE_IOVA, PD0_IOVA, PD1_IOVA, PD2_IOVA, PD3_IOVA, PT0_IOVA, RUNLIST_IOVA,
};
use super::registers::{TARGET_SYS_MEM_COHERENT, pccsr, pfifo};

/// GPFIFO entry size in bytes (Volta+ GPFIFO fetch record).
pub const GPFIFO_ENTRY_BYTES: usize = 8;

/// Standard 4 KiB DMA buffer size for channel infrastructure objects.
pub const DMA_BUFFER_SIZE: usize = 4096;

/// Caller-provided PFIFO channel parameters (GPFIFO / USERD IOVAs and IDs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelConfig {
    /// IOVA of the GPFIFO ring in system memory.
    pub gpfifo_iova: u64,
    /// Number of GPFIFO entries (each [`GPFIFO_ENTRY_BYTES`] bytes).
    pub gpfifo_entries: u32,
    /// IOVA of the USERD page.
    pub userd_iova: u64,
    /// Hardware channel ID.
    pub channel_id: u32,
}

/// Encoded `MMU_INVALIDATE` register writes for a given PD3 IOVA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmuInvalidateEncode {
    /// `MMU_INVALIDATE_PDB` low word.
    pub pdb_lo: u32,
    /// `MMU_INVALIDATE_PDB_HI`.
    pub pdb_hi: u32,
    /// `MMU_INVALIDATE` trigger (`PAGE_ALL | HUB_ONLY | trigger bit`).
    pub trigger: u32,
}

/// Precomputed IOVAs and register payloads for one `VfioChannel` bring-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelLayout {
    /// Caller GPFIFO ring IOVA.
    pub gpfifo_iova: u64,
    /// `gpfifo_entries * GPFIFO_ENTRY_BYTES`.
    pub gpfifo_size_bytes: usize,
    /// Caller USERD IOVA.
    pub userd_iova: u64,
    /// Caller channel ID.
    pub channel_id: u32,
    /// Instance block DMA IOVA (RAMFC + RAMIN).
    pub instance_iova: u64,
    /// Runlist buffer IOVA.
    pub runlist_iova: u64,
    /// Page directory / table IOVAs (V2 MMU chain).
    pub pd3_iova: u64,
    /// Page directory level 2 IOVA.
    pub pd2_iova: u64,
    /// Page directory level 1 IOVA.
    pub pd1_iova: u64,
    /// Page directory level 0 IOVA.
    pub pd0_iova: u64,
    /// Page table IOVA.
    pub pt0_iova: u64,
    /// MMU fault buffer IOVA (non-replayable + replayable use same PFN).
    pub fault_buffer_iova: u64,
    /// Size of each infrastructure `DmaBuffer` (bytes).
    pub dma_buffer_size: usize,
    /// Number of fault records per buffer (`FAULT_BUFn_SIZE`).
    pub fault_buffer_entries: u32,
    /// Low PFN bits written to `FAULT_BUFn_LO` (`iova >> 12`).
    pub fault_buf_pfn_lo: u32,
    /// `PBUS_BAR2_BLOCK` value: PHYSICAL mode, SYS_MEM_COH (`2 << 28`).
    pub bar2_physical_block: u32,
    /// PDB invalidation encoding for [`Self::pd3_iova`].
    pub mmu_invalidate: MmuInvalidateEncode,
    /// Runlist entry count passed to `gv100_runlist_submit_value` (TSG + channel).
    pub runlist_submit_entry_count: u32,
    /// Value for `runlist_base(runlist_id)` including aperture target in bits [29:28].
    pub gv100_runlist_base_reg: u32,
    /// Value for `runlist_submit(runlist_id)`.
    pub gv100_runlist_submit_reg: u32,
}

impl ChannelLayout {
    /// Encode MMU TLB invalidation for a page directory at `pd3_iova`.
    #[must_use]
    pub fn mmu_invalidate_encode(pd3_iova: u64) -> MmuInvalidateEncode {
        const TRIG: u32 = 0x8000_0005;
        let pdb_inv = ((pd3_iova >> 12) << 4) | 2_u64;
        MmuInvalidateEncode {
            pdb_lo: pdb_inv as u32,
            pdb_hi: (pd3_iova >> 32) as u32,
            trigger: TRIG,
        }
    }

    /// Compute layout from caller parameters and fixed infrastructure IOVAs.
    #[must_use]
    pub fn compute(config: &ChannelConfig) -> Self {
        const FAULT_ENTRIES: u32 = 64;
        const RL_ENTRIES: u32 = 2;
        const BAR2_PHYS: u32 = 2 << 28;

        let gpfifo_size_bytes = (config.gpfifo_entries as usize).saturating_mul(GPFIFO_ENTRY_BYTES);
        let mmu_invalidate = Self::mmu_invalidate_encode(PD3_IOVA);

        Self {
            gpfifo_iova: config.gpfifo_iova,
            gpfifo_size_bytes,
            userd_iova: config.userd_iova,
            channel_id: config.channel_id,
            instance_iova: INSTANCE_IOVA,
            runlist_iova: RUNLIST_IOVA,
            pd3_iova: PD3_IOVA,
            pd2_iova: PD2_IOVA,
            pd1_iova: PD1_IOVA,
            pd0_iova: PD0_IOVA,
            pt0_iova: PT0_IOVA,
            fault_buffer_iova: FAULT_BUF_IOVA,
            dma_buffer_size: DMA_BUFFER_SIZE,
            fault_buffer_entries: FAULT_ENTRIES,
            fault_buf_pfn_lo: (FAULT_BUF_IOVA >> 12) as u32,
            bar2_physical_block: BAR2_PHYS,
            mmu_invalidate,
            runlist_submit_entry_count: RL_ENTRIES,
            gv100_runlist_base_reg: pfifo::gv100_runlist_base_value(RUNLIST_IOVA)
                | (TARGET_SYS_MEM_COHERENT << 28),
            gv100_runlist_submit_reg: pfifo::gv100_runlist_submit_value(RUNLIST_IOVA, RL_ENTRIES),
        }
    }

    /// PCCSR instance bind value (`INST_BIND | target | PFN`).
    #[must_use]
    pub fn pccsr_inst_bind_value(self) -> u32 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "INSTANCE_IOVA >> 12 fits u32 for fixed layout range"
        )]
        let pfn = (INSTANCE_IOVA >> 12) as u32;
        pfn | (TARGET_SYS_MEM_COHERENT << 28) | pccsr::INST_BIND_TRUE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_iovas_and_dma_sizes_align_with_constants() {
        let cfg = ChannelConfig {
            gpfifo_iova: 0x1_0000,
            gpfifo_entries: 32,
            userd_iova: 0x2000,
            channel_id: 5,
        };
        let layout = ChannelLayout::compute(&cfg);
        assert_eq!(layout.instance_iova, INSTANCE_IOVA);
        assert_eq!(layout.runlist_iova, RUNLIST_IOVA);
        assert_eq!(layout.pd3_iova, PD3_IOVA);
        assert_eq!(layout.fault_buffer_iova, FAULT_BUF_IOVA);
        assert_eq!(layout.dma_buffer_size, 4096);
        assert_eq!(layout.gpfifo_size_bytes, 32 * GPFIFO_ENTRY_BYTES);
        assert_eq!(layout.fault_buffer_entries, 64);
    }

    #[test]
    fn fault_buffer_pfn_matches_iova_shift() {
        let cfg = ChannelConfig {
            gpfifo_iova: 0,
            gpfifo_entries: 1,
            userd_iova: 0,
            channel_id: 0,
        };
        let layout = ChannelLayout::compute(&cfg);
        assert_eq!(
            layout.fault_buf_pfn_lo,
            u32::try_from(FAULT_BUF_IOVA >> 12).expect("pfn fits u32")
        );
    }

    #[test]
    fn bar2_and_mmu_invalidate_match_create_with_config_encoding() {
        let cfg = ChannelConfig {
            gpfifo_iova: 0,
            gpfifo_entries: 1,
            userd_iova: 0,
            channel_id: 0,
        };
        let layout = ChannelLayout::compute(&cfg);
        assert_eq!(layout.bar2_physical_block, 2 << 28);
        let inv = ChannelLayout::mmu_invalidate_encode(PD3_IOVA);
        assert_eq!(layout.mmu_invalidate, inv);
        assert_eq!(
            layout.gv100_runlist_base_reg,
            pfifo::gv100_runlist_base_value(RUNLIST_IOVA) | (TARGET_SYS_MEM_COHERENT << 28)
        );
        assert_eq!(
            layout.gv100_runlist_submit_reg,
            pfifo::gv100_runlist_submit_value(RUNLIST_IOVA, 2)
        );
    }

    #[test]
    fn pccsr_bind_value_matches_manual_formula() {
        let layout = ChannelLayout::compute(&ChannelConfig {
            gpfifo_iova: 0,
            gpfifo_entries: 1,
            userd_iova: 0,
            channel_id: 0,
        });
        let expected =
            (INSTANCE_IOVA >> 12) as u32 | (TARGET_SYS_MEM_COHERENT << 28) | pccsr::INST_BIND_TRUE;
        assert_eq!(layout.pccsr_inst_bind_value(), expected);
    }

    #[test]
    fn fault_buf_pfn_lo_matches_shifted_iova() {
        let layout = ChannelLayout::compute(&ChannelConfig {
            gpfifo_iova: 0,
            gpfifo_entries: 1,
            userd_iova: 0,
            channel_id: 0,
        });
        assert_eq!(layout.fault_buf_pfn_lo, (FAULT_BUF_IOVA >> 12) as u32);
    }

    #[test]
    fn gpfifo_size_scales_with_entry_count() {
        let a = ChannelLayout::compute(&ChannelConfig {
            gpfifo_iova: 0,
            gpfifo_entries: 1,
            userd_iova: 0,
            channel_id: 0,
        });
        let b = ChannelLayout::compute(&ChannelConfig {
            gpfifo_iova: 0,
            gpfifo_entries: 128,
            userd_iova: 0,
            channel_id: 0,
        });
        assert_eq!(a.gpfifo_size_bytes, 8);
        assert_eq!(b.gpfifo_size_bytes, 128 * GPFIFO_ENTRY_BYTES);
    }

    #[test]
    fn channel_id_preserved_for_very_large_id() {
        let layout = ChannelLayout::compute(&ChannelConfig {
            gpfifo_iova: 0x10_0000_0000,
            gpfifo_entries: 64,
            userd_iova: 0x2000,
            channel_id: 0xFFFF_FFFF,
        });
        assert_eq!(layout.channel_id, 0xFFFF_FFFF);
    }
}
