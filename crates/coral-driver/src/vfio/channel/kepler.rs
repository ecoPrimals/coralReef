// SPDX-License-Identifier: AGPL-3.0-only
//! PFIFO hardware channel creation for Kepler (GK104/GK110/GK210).
//!
//! Kepler uses a simpler channel setup than Volta:
//! - 2-level page tables (PDB → SPT) instead of 5-level
//! - 8-byte runlist entries (no TSG headers)
//! - GK104 runlist registers (stride 0x08 instead of 0x10)
//! - No USERMODE doorbell — PBDMA polls USERD GP_PUT directly
//! - No non-replayable MMU fault buffers
//! - RAMFC CONFIG register exists (unlike Volta)
//!
//! # Channel creation sequence
//!
//! 1. Allocate DMA buffers for instance block, runlist, PDB, and SPT
//! 2. Initialize PFIFO engine (toggle 0x2200, enable scheduler)
//! 3. Populate RAMFC (GPFIFO base, USERD, channel ID, GF100 PDB)
//! 4. Set up GF100 V1 page tables (identity map for first 2 MiB)
//! 5. Populate runlist with flat channel entry
//! 6. Bind instance block to channel via PCCSR (same as Volta)
//! 7. Enable channel and submit runlist

use std::borrow::Cow;

use crate::error::{DriverError, DriverResult};
use crate::vfio::device::{DmaBackend, MappedBar};
use crate::vfio::dma::DmaBuffer;

use super::kepler_page_tables;
use super::registers::*;

/// Kepler PFIFO hardware channel — owns DMA resources for a single channel.
///
/// Uses GF100 V1 two-level page tables (PDB + SPT) instead of Volta's
/// five-level hierarchy.
pub struct KeplerChannel {
    instance: DmaBuffer,
    runlist: DmaBuffer,
    pdb: DmaBuffer,
    spt: DmaBuffer,
    channel_id: u32,
    runlist_id: u32,
}

impl KeplerChannel {
    /// Create and activate a Kepler GPU PFIFO channel via BAR0.
    ///
    /// Full channel lifecycle for GK104/GK110:
    /// 1. Toggle PFIFO engine enable (0x2200)
    /// 2. Allocate DMA buffers (instance, runlist, PDB, SPT)
    /// 3. Populate RAMFC + GF100 V1 page tables
    /// 4. Bind channel via PCCSR
    /// 5. Enable channel and submit runlist
    pub fn create(
        container: DmaBackend,
        bar0: &MappedBar,
        gpfifo_iova: u64,
        gpfifo_entries: u32,
        userd_iova: u64,
        channel_id: u32,
    ) -> DriverResult<Self> {
        let runlist_id = Self::init_kepler_pfifo(bar0)?;

        // Configure BAR2 in PHYSICAL mode (same as Volta cold path).
        {
            let bar2_val: u32 = 2 << 28; // target=COH, mode=PHYSICAL
            bar0.write_u32(misc::PBUS_BAR2_BLOCK, bar2_val)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("BAR2_BLOCK: {e}"))))?;
            std::thread::sleep(std::time::Duration::from_millis(5));
            tracing::info!(
                bar2_block = format_args!("{bar2_val:#010x}"),
                "Kepler: BAR2 set to PHYSICAL mode"
            );
        }

        let instance = DmaBuffer::new(container.clone(), 4096, INSTANCE_IOVA)?;
        let runlist = DmaBuffer::new(container.clone(), 4096, RUNLIST_IOVA)?;
        let pdb = DmaBuffer::new(container.clone(), 4096, KEPLER_PDB_IOVA)?;
        let spt = DmaBuffer::new(container.clone(), 4096, KEPLER_SPT_IOVA)?;

        let mut chan = Self {
            instance,
            runlist,
            pdb,
            spt,
            channel_id,
            runlist_id,
        };

        kepler_page_tables::populate_kepler_page_tables(
            chan.pdb.as_mut_slice(),
            chan.spt.as_mut_slice(),
        );
        kepler_page_tables::populate_kepler_instance_block(
            chan.instance.as_mut_slice(),
            gpfifo_iova,
            gpfifo_entries,
            userd_iova,
            channel_id,
            KEPLER_PDB_IOVA,
        );
        kepler_page_tables::populate_kepler_runlist(chan.runlist.as_mut_slice(), channel_id);

        Self::invalidate_kepler_tlb(bar0)?;

        let stale = bar0.read_u32(pccsr::channel(channel_id)).unwrap_or(0);
        if stale != 0 {
            Self::clear_stale_pccsr(bar0, channel_id, stale)?;
        }

        chan.bind_channel(bar0)?;
        std::thread::sleep(std::time::Duration::from_millis(5));
        chan.clear_channel_faults(bar0)?;
        chan.enable_channel(bar0)?;
        chan.submit_runlist(bar0)?;

        std::thread::sleep(std::time::Duration::from_millis(50));

        tracing::info!(
            channel_id,
            gpfifo_iova = format_args!("{gpfifo_iova:#x}"),
            userd_iova = format_args!("{userd_iova:#x}"),
            pdb_iova = format_args!("{KEPLER_PDB_IOVA:#x}"),
            "Kepler PFIFO channel created"
        );

        Ok(chan)
    }

    /// Channel ID for PCCSR/submission reference.
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.channel_id
    }

    /// Initialize Kepler PFIFO engine.
    ///
    /// Toggle 0x2200 to reset PFIFO, enable scheduler, discover GR runlist.
    fn init_kepler_pfifo(bar0: &MappedBar) -> DriverResult<u32> {
        let w = |reg: usize, val: u32| -> DriverResult<()> {
            bar0.write_u32(reg, val).map_err(|e| {
                DriverError::SubmitFailed(Cow::Owned(format!("PFIFO init {reg:#x}: {e}")))
            })
        };

        // Clear any pending PFIFO interrupts.
        w(pfifo::INTR, 0xFFFF_FFFF)?;

        // Enable PFIFO engine via PMC_ENABLE bit 8.
        let pmc = bar0.read_u32(misc::PMC_ENABLE).unwrap_or(0);
        if pmc & (1 << 8) == 0 {
            w(misc::PMC_ENABLE, pmc | (1 << 8))?;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // PFIFO enable toggle (0→1 resets scheduler + PBDMAs).
        let pfifo_en = bar0.read_u32(pfifo::ENABLE).unwrap_or(0);
        if pfifo_en & 1 == 0 {
            w(pfifo::ENABLE, 0)?;
            std::thread::sleep(std::time::Duration::from_millis(5));
            w(pfifo::ENABLE, 1)?;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Enable scheduler.
        w(pfifo::SCHED_EN, 1)?;
        w(pfifo::INTR, 0xFFFF_FFFF)?;

        // Discover active PBDMAs.
        let pbdma_map = bar0.read_u32(pfifo::PBDMA_MAP).unwrap_or(0);
        tracing::info!(
            pbdma_map = format_args!("{pbdma_map:#010x}"),
            "Kepler PFIFO initialized"
        );

        // GR is typically runlist 0 on Kepler.
        Ok(0)
    }

    /// GF100 MMU TLB invalidation.
    ///
    /// Uses the same PFB MMU registers as Volta, but with simpler encoding.
    fn invalidate_kepler_tlb(bar0: &MappedBar) -> DriverResult<()> {
        use super::registers::pfb;

        for _ in 0..200 {
            let ctrl = bar0.read_u32(pfb::MMU_CTRL).unwrap_or(0);
            if ctrl & 0x00FF_0000 != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }

        let pdb_inv = ((KEPLER_PDB_IOVA >> 12) << 4) | 2; // SYS_MEM_COH
        bar0.write_u32(pfb::MMU_INVALIDATE_PDB, pdb_inv as u32)
            .map_err(|e| {
                DriverError::SubmitFailed(Cow::Owned(format!("MMU_INVALIDATE_PDB: {e}")))
            })?;
        bar0.write_u32(pfb::MMU_INVALIDATE_PDB_HI, 0).map_err(|e| {
            DriverError::SubmitFailed(Cow::Owned(format!("MMU_INVALIDATE_PDB_HI: {e}")))
        })?;

        // Trigger: PAGE_ALL | HUB_ONLY | trigger.
        bar0.write_u32(pfb::MMU_INVALIDATE, 0x8000_0005)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("MMU_INVALIDATE: {e}"))))?;

        for _ in 0..200 {
            let ctrl = bar0.read_u32(pfb::MMU_CTRL).unwrap_or(0);
            if ctrl & 0x0000_8000 != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }

        tracing::info!("Kepler MMU TLB invalidated");
        Ok(())
    }

    /// Clear stale PCCSR from a previous driver.
    fn clear_stale_pccsr(bar0: &MappedBar, channel_id: u32, stale: u32) -> DriverResult<()> {
        if stale & 1 != 0 {
            bar0.write_u32(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR)
                .map_err(|e| {
                    DriverError::SubmitFailed(Cow::Owned(format!("PCCSR disable: {e}")))
                })?;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        bar0.write_u32(
            pccsr::channel(channel_id),
            pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
        )
        .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCCSR fault clear: {e}"))))?;
        std::thread::sleep(std::time::Duration::from_millis(10));

        bar0.write_u32(pccsr::inst(channel_id), 0)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCCSR clear inst: {e}"))))?;
        std::thread::sleep(std::time::Duration::from_millis(10));
        Ok(())
    }

    /// Bind instance block via PCCSR (same encoding as Volta).
    fn bind_channel(&self, bar0: &MappedBar) -> DriverResult<()> {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "INSTANCE_IOVA >> 12 fits u32"
        )]
        let value =
            (INSTANCE_IOVA >> 12) as u32 | (TARGET_SYS_MEM_COHERENT << 28) | pccsr::INST_BIND_TRUE;
        tracing::debug!(
            value = format_args!("{value:#010x}"),
            "Kepler PCCSR inst (BIND | SYS_MEM_COH)"
        );
        bar0.write_u32(pccsr::inst(self.channel_id), value)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCCSR bind: {e}"))))
    }

    /// Clear stale channel faults.
    fn clear_channel_faults(&self, bar0: &MappedBar) -> DriverResult<()> {
        let ch = pccsr::channel(self.channel_id);
        let pre = bar0.read_u32(ch).unwrap_or(0);
        if pre & (pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET) != 0 {
            bar0.write_u32(ch, pccsr::CHANNEL_ENABLE_CLR)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("chan disable: {e}"))))?;
            std::thread::sleep(std::time::Duration::from_millis(2));
            bar0.write_u32(ch, pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("fault clear: {e}"))))?;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        Ok(())
    }

    /// Enable channel via PCCSR.
    fn enable_channel(&self, bar0: &MappedBar) -> DriverResult<()> {
        bar0.write_u32(pccsr::channel(self.channel_id), pccsr::CHANNEL_ENABLE_SET)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("channel enable: {e}"))))
    }

    /// Submit runlist using GK104 registers (stride 0x08).
    fn submit_runlist(&self, bar0: &MappedBar) -> DriverResult<()> {
        let rl_base =
            kepler_pfifo::gk104_runlist_base_value(RUNLIST_IOVA) | (TARGET_SYS_MEM_COHERENT << 28);
        // GK104 runlist has 1 entry (one channel, no TSG header).
        let rl_submit = kepler_pfifo::gk104_runlist_submit_value(RUNLIST_IOVA, 1);

        tracing::debug!(
            runlist_id = self.runlist_id,
            rl_base = format_args!("{rl_base:#010x}"),
            rl_submit = format_args!("{rl_submit:#010x}"),
            "submitting Kepler runlist (gk104)"
        );

        bar0.write_u32(kepler_pfifo::runlist_base(self.runlist_id), rl_base)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("runlist base: {e}"))))?;
        bar0.write_u32(kepler_pfifo::runlist_submit(self.runlist_id), rl_submit)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("runlist submit: {e}"))))
    }
}

impl std::fmt::Debug for KeplerChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeplerChannel")
            .field("channel_id", &self.channel_id)
            .field("instance_iova", &format_args!("{INSTANCE_IOVA:#x}"))
            .finish_non_exhaustive()
    }
}
