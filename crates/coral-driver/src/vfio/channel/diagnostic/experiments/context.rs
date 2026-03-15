// SPDX-License-Identifier: AGPL-3.0-only

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
    pub rl_base: u32,
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
}
