// SPDX-License-Identifier: AGPL-3.0-only
#![allow(missing_docs)]

use std::borrow::Cow;

use crate::error::{DriverError, DriverResult};
use crate::vfio::device::MappedBar;
use crate::vfio::dma::DmaBuffer;

use super::super::types::ExperimentConfig;

/// Shared context for a single diagnostic experiment.
/// Holds all state needed by experiment handlers.
pub(in crate::vfio::channel::diagnostic) struct ExperimentContext<'a> {
    pub bar0: &'a MappedBar,
    pub channel_id: u32,
    pub gpfifo_iova: u64,
    pub userd_iova: u64,
    pub instance: &'a mut DmaBuffer,
    pub runlist: &'a mut DmaBuffer,
    pub gpfifo_ring: &'a mut [u8],
    pub userd_page: &'a mut [u8],
    pub target_runlist: u32,
    pub target_pbdma: usize,
    pub pbdma_base: usize,
    pub pbdma_map: u32,
    pub pccsr_inst_val: u32,
    /// GV100 per-runlist BASE register address.
    pub rl_base_reg: usize,
    /// GV100 per-runlist SUBMIT register address.
    pub rl_submit_reg: usize,
    /// Value to write to the runlist BASE register.
    pub rl_base: u32,
    /// Value to write to the runlist SUBMIT register.
    pub rl_submit: u32,
    pub limit2: u32,
    pub gpu_warm: bool,
    pub cfg: &'a ExperimentConfig,
}

impl<'a> ExperimentContext<'a> {
    /// Read BAR0 register at `reg`.
    #[inline]
    pub fn r(&self, reg: usize) -> u32 {
        self.bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD)
    }

    /// Write BAR0 register at `reg` with `val`.
    #[inline]
    pub fn w(&self, reg: usize, val: u32) -> DriverResult<()> {
        self.bar0
            .write_u32(reg, val)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("diag {reg:#x}: {e}"))))
    }

    /// PBDMA base address for target PBDMA (pb in original).
    #[inline]
    pub fn pb(&self) -> usize {
        self.pbdma_base
    }

    /// Submit the runlist using GV100 per-runlist registers.
    pub fn submit_runlist(&self) -> DriverResult<()> {
        self.w(self.rl_base_reg, self.rl_base)?;
        self.w(self.rl_submit_reg, self.rl_submit)
    }

    /// Clear all pending PFIFO interrupts and return the value that was cleared.
    pub fn clear_pfifo_intr(&self) -> u32 {
        use crate::vfio::channel::registers::pfifo;
        let intr = self.r(pfifo::INTR);
        if intr != 0 {
            let _ = self.w(pfifo::INTR, intr);
        }
        intr
    }

    /// Clear PFIFO INTR bit 8 (post-runlist-submit interrupt on GV100).
    /// Returns true if bit 8 was set and cleared.
    #[allow(dead_code)]
    pub fn clear_pfifo_intr_bit8(&self) -> bool {
        use crate::vfio::channel::registers::pfifo;
        let intr = self.r(pfifo::INTR);
        if intr & pfifo::INTR_BIT8 != 0 {
            let _ = self.w(pfifo::INTR, pfifo::INTR_BIT8);
            true
        } else {
            false
        }
    }

    /// Full PBDMA reset: clear interrupts, clear PCCSR faults, toggle PMC PBDMA bit.
    /// Call before context load to clear any stale state from previous dispatches.
    pub fn reset_pbdma(&self) {
        use crate::vfio::channel::registers::{pbdma, pccsr, pmc};

        let _ = self.w(pbdma::intr(self.target_pbdma), 0xFFFF_FFFF);
        let _ = self.w(pbdma::hce_intr(self.target_pbdma), 0xFFFF_FFFF);

        let ch_ctrl = self.r(pccsr::channel(self.channel_id));
        if ch_ctrl & pccsr::PBDMA_FAULTED_RESET != 0 {
            let _ = self.w(pccsr::channel(self.channel_id), pccsr::PBDMA_FAULTED_RESET);
        }
        if ch_ctrl & pccsr::ENG_FAULTED_RESET != 0 {
            let _ = self.w(pccsr::channel(self.channel_id), pccsr::ENG_FAULTED_RESET);
        }

        // Toggle PMC PBDMA enable bit to force a full reset of the PBDMA engine.
        let pmc_en = self.r(pmc::ENABLE);
        let pbdma_bit = 1_u32 << 8; // PFIFO/PBDMA engine in PMC
        if pmc_en & pbdma_bit != 0 {
            let _ = self.w(pmc::ENABLE, pmc_en & !pbdma_bit);
            std::thread::sleep(std::time::Duration::from_millis(1));
            let _ = self.w(pmc::ENABLE, pmc_en | pbdma_bit);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Flush all DMA buffers from CPU cache so the GPU sees latest writes.
    #[cfg(target_arch = "x86_64")]
    pub fn flush_dma(&self) {
        crate::vfio::cache_ops::clflush_range(
            self.instance.as_slice().as_ptr(),
            self.instance.as_slice().len(),
        );
        crate::vfio::cache_ops::clflush_range(
            self.runlist.as_slice().as_ptr(),
            self.runlist.as_slice().len(),
        );
        crate::vfio::cache_ops::clflush_range(self.gpfifo_ring.as_ptr(), self.gpfifo_ring.len());
        crate::vfio::cache_ops::clflush_range(self.userd_page.as_ptr(), self.userd_page.len());
        crate::vfio::cache_ops::memory_fence();
    }
}
