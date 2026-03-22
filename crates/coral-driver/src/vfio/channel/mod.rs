// SPDX-License-Identifier: AGPL-3.0-only
//! PFIFO hardware channel creation for Volta+ via BAR0 MMIO.
//!
//! Creates a GPU command channel from scratch using direct register writes,
//! bypassing the kernel GPU driver. This is the bridge between VFIO BAR0/DMA
//! setup and actual GPU command dispatch — without a channel, the GPU's PFIFO
//! engine does not know our GPFIFO ring exists.
//!
//! # Channel creation sequence
//!
//! 1. Allocate DMA buffers for instance block, runlist, and V2 page tables
//! 2. Populate RAMFC (GPFIFO base, USERD pointer, channel ID, signature)
//! 3. Set up V2 MMU page tables (identity map for first 2 MiB of IOVA space)
//! 4. Build runlist with TSG header + channel entry (Volta RAMRL format)
//! 5. Bind instance block to channel via PCCSR registers
//! 6. Enable channel and submit runlist to PFIFO

pub mod devinit;
pub mod glowplug;
pub mod hbm2_training;
pub mod nouveau_oracle;
pub mod oracle;
pub mod pri_monitor;
pub mod registers;

pub mod diagnostic;
pub mod mmu_fault;
mod page_tables;
mod pfifo;

pub use diagnostic::{
    ExperimentConfig, ExperimentOrdering, ExperimentResult, build_experiment_matrix,
    build_metal_discovery_matrix, diagnostic_matrix,
    interpreter::{ProbeInterpreter, ProbeReport, memory_probe},
};
pub use registers::ramuserd;

use std::borrow::Cow;

use crate::error::{DriverError, DriverResult};
use crate::vfio::device::{DmaBackend, MappedBar};
use crate::vfio::dma::DmaBuffer;

use registers::*;

/// PFIFO hardware channel — owns all DMA resources for a single GPU channel.
///
/// Created during `NvVfioComputeDevice::open()` and held alive for
/// the device lifetime. Dropped automatically when the parent device drops,
/// releasing all DMA allocations.
pub struct VfioChannel {
    instance: DmaBuffer,
    runlist: DmaBuffer,
    pd3: DmaBuffer,
    pd2: DmaBuffer,
    pd1: DmaBuffer,
    pd0: DmaBuffer,
    pt0: DmaBuffer,
    channel_id: u32,
    runlist_id: u32,
}

impl VfioChannel {
    /// Create and activate a GPU PFIFO channel via BAR0 register programming.
    ///
    /// This performs the full channel lifecycle:
    /// 1. Allocate DMA buffers for instance block, runlist, and page tables
    /// 2. Populate RAMFC (GPFIFO base, USERD, channel ID)
    /// 3. Set up V2 MMU page tables (identity map for first 2 MiB)
    /// 4. Build runlist with TSG header + channel entry
    /// 5. Bind instance block and enable channel via PCCSR
    /// 6. Submit runlist to PFIFO
    ///
    /// # Errors
    ///
    /// Returns error if any DMA allocation or BAR0 write fails.
    pub fn create(
        container: DmaBackend,
        bar0: &MappedBar,
        gpfifo_iova: u64,
        gpfifo_entries: u32,
        userd_iova: u64,
        channel_id: u32,
    ) -> DriverResult<Self> {
        let instance = DmaBuffer::new(container.clone(), 4096, INSTANCE_IOVA)?;
        let runlist = DmaBuffer::new(container.clone(), 4096, RUNLIST_IOVA)?;
        let pd3 = DmaBuffer::new(container.clone(), 4096, PD3_IOVA)?;
        let pd2 = DmaBuffer::new(container.clone(), 4096, PD2_IOVA)?;
        let pd1 = DmaBuffer::new(container.clone(), 4096, PD1_IOVA)?;
        let pd0 = DmaBuffer::new(container.clone(), 4096, PD0_IOVA)?;
        let pt0 = DmaBuffer::new(container.clone(), 4096, PT0_IOVA)?;

        let mut chan = Self {
            instance,
            runlist,
            pd3,
            pd2,
            pd1,
            pd0,
            pt0,
            channel_id,
            runlist_id: 0,
        };

        let pfifo_ck = |bar0: &MappedBar, label: &str| {
            let en = bar0.read_u32(registers::pfifo::ENABLE).unwrap_or(0xDEAD);
            let intr = bar0.read_u32(registers::pfifo::INTR).unwrap_or(0xDEAD);
            tracing::debug!(en = format_args!("{en:#010x}"), intr = format_args!("{intr:#010x}"), "{label}");
            if en == 0 && intr != 0xDEAD {
                tracing::warn!(intr = format_args!("{intr:#010x}"), "PFIFO disabled at {label}");
            }
        };

        let (runq, runlist_id) = pfifo::init_pfifo_engine(bar0)?;
        chan.runlist_id = runlist_id;
        pfifo_ck(bar0, "after-pfifo-init");

        // Configure BAR2 in PHYSICAL mode targeting system memory.
        // The VRAM-based BAR2 setup (VIRTUAL mode) fails on cold VFIO cards
        // because VRAM is not initialized. PHYSICAL mode bypasses page tables
        // and gives PFIFO a direct path to system memory via PCIe+IOMMU.
        {
            let bar2_val: u32 = 2 << 28; // target=COH, mode=PHYSICAL, ptr=0
            bar0.write_u32(registers::misc::PBUS_BAR2_BLOCK, bar2_val)
                .map_err(|e| DriverError::SubmitFailed(
                    Cow::Owned(format!("BAR2_BLOCK: {e}"))
                ))?;
            std::thread::sleep(std::time::Duration::from_millis(5));
            tracing::info!(
                bar2_block = format_args!("{bar2_val:#010x}"),
                "BAR2 set to PHYSICAL mode (SYS_MEM_COH)"
            );
        }
        pfifo_ck(bar0, "after-bar2-setup");

        page_tables::populate_page_tables(
            chan.pd3.as_mut_slice(),
            chan.pd2.as_mut_slice(),
            chan.pd1.as_mut_slice(),
            chan.pd0.as_mut_slice(),
            chan.pt0.as_mut_slice(),
        );
        page_tables::populate_instance_block(
            chan.instance.as_mut_slice(),
            gpfifo_iova,
            gpfifo_entries,
            userd_iova,
            channel_id,
        );
        page_tables::populate_runlist(
            chan.runlist.as_mut_slice(),
            userd_iova,
            channel_id,
            INSTANCE_IOVA,
            runq,
        );

        Self::invalidate_tlb(bar0, PD3_IOVA)?;
        pfifo_ck(bar0, "after-tlb-invalidate");

        // Clear stale PCCSR state from prior driver (nouveau residue).
        let stale = bar0.read_u32(pccsr::channel(channel_id)).unwrap_or(0);
        if stale != 0 {
            Self::clear_stale_pccsr(bar0, channel_id, stale)?;
        }
        pfifo_ck(bar0, "after-clear-pccsr");

        chan.bind_channel(bar0)?;
        pfifo_ck(bar0, "after-bind-channel");

        std::thread::sleep(std::time::Duration::from_millis(5));
        chan.clear_channel_faults(bar0)?;
        pfifo_ck(bar0, "after-clear-faults");

        chan.enable_channel(bar0)?;
        pfifo_ck(bar0, "after-enable-channel");

        chan.submit_runlist(bar0)?;
        pfifo_ck(bar0, "after-submit-runlist");

        std::thread::sleep(std::time::Duration::from_millis(50));
        pfifo_ck(bar0, "after-50ms-settle");
        pfifo::log_pfifo_diagnostics(bar0);

        tracing::info!(
            channel_id,
            gpfifo_iova = format_args!("{gpfifo_iova:#x}"),
            userd_iova = format_args!("{userd_iova:#x}"),
            instance_iova = format_args!("{INSTANCE_IOVA:#x}"),
            "VFIO PFIFO channel created"
        );

        Ok(chan)
    }

    /// Channel ID used for doorbell notification.
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.channel_id
    }

    /// BAR0 offset for the USERMODE doorbell register.
    #[must_use]
    pub const fn doorbell_offset() -> usize {
        usermode::NOTIFY_CHANNEL_PENDING
    }

    /// Invalidate the GPU MMU TLB for our page directory base.
    ///
    /// Matches nouveau's `gf100_vmm_invalidate`: write the PDB address to
    /// `MMU_INVALIDATE_PDB`, then trigger with `PAGE_ALL | HUB_ONLY`.
    /// For system memory targets, PDB addr uses the IOVA with target=SYS_COH.
    fn invalidate_tlb(bar0: &MappedBar, pd3_iova: u64) -> DriverResult<()> {
        use registers::pfb;

        // Wait for flush slot availability.
        for _ in 0..200 {
            let ctrl = bar0.read_u32(pfb::MMU_CTRL).unwrap_or(0);
            if ctrl & 0x00FF_0000 != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }

        // PDB address for invalidation: (iova >> 12) << 4 | target.
        // target=2 (SYS_MEM_COH) to match our page table aperture.
        let pdb_inv = ((pd3_iova >> 12) << 4) | 2; // SYS_MEM_COH target
        bar0.write_u32(pfb::MMU_INVALIDATE_PDB, pdb_inv as u32)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("MMU_INVALIDATE_PDB: {e}"))))?;
        bar0.write_u32(pfb::MMU_INVALIDATE_PDB_HI, (pd3_iova >> 32) as u32)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("MMU_INVALIDATE_PDB_HI: {e}"))))?;

        // Trigger: PAGE_ALL (bit 0) | HUB_ONLY (bit 2) | trigger (bit 31).
        bar0.write_u32(pfb::MMU_INVALIDATE, 0x8000_0005)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("MMU_INVALIDATE trigger: {e}"))))?;

        // Wait for flush acknowledgement.
        for _ in 0..200 {
            let ctrl = bar0.read_u32(pfb::MMU_CTRL).unwrap_or(0);
            if ctrl & 0x0000_8000 != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }

        tracing::info!(pd3_iova = format_args!("{pd3_iova:#x}"), "GPU MMU TLB invalidated");
        Ok(())
    }

    /// Clear stale PCCSR state inherited from a previous driver.
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

    /// Bind the channel's instance block to PCCSR.
    fn bind_channel(&self, bar0: &MappedBar) -> DriverResult<()> {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "INSTANCE_IOVA >> 12 fits u32 for our allocation range"
        )]
        let value = (INSTANCE_IOVA >> 12) as u32
            | (TARGET_SYS_MEM_COHERENT << 28)
            | pccsr::INST_BIND_TRUE;
        tracing::debug!(
            value = format_args!("{value:#010x}"),
            "PCCSR inst (BIND | SYS_MEM_COH)"
        );
        bar0.write_u32(pccsr::inst(self.channel_id), value)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCCSR bind: {e}"))))
    }

    /// Clear stale `PBDMA_FAULTED` / `ENG_FAULTED` flags.
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

            tracing::debug!(
                pre = format_args!("{pre:#010x}"),
                post = format_args!("{:#010x}", bar0.read_u32(ch).unwrap_or(0xDEAD)),
                "cleared channel faults"
            );
        }
        Ok(())
    }

    /// Enable the channel via PCCSR `ENABLE_SET` trigger.
    fn enable_channel(&self, bar0: &MappedBar) -> DriverResult<()> {
        bar0.write_u32(pccsr::channel(self.channel_id), pccsr::CHANNEL_ENABLE_SET)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("channel enable: {e}"))))
    }

    /// Submit runlist to PFIFO using GV100 per-runlist registers.
    ///
    /// GV100 uses per-runlist registers at stride 0x10:
    ///   BASE(rl) = 0x2270 + rl*0x10   → lower_32(iova >> 12)
    ///   SUBMIT(rl) = 0x2274 + rl*0x10 → upper_32(iova >> 12) | (count << 16)
    /// Writing SUBMIT triggers the scheduler.
    /// Source: nouveau `gv100_runl_commit()`.
    fn submit_runlist(&self, bar0: &MappedBar) -> DriverResult<()> {
        let rl_base = registers::pfifo::gv100_runlist_base_value(RUNLIST_IOVA)
            | (TARGET_SYS_MEM_COHERENT << 28);
        let rl_submit = registers::pfifo::gv100_runlist_submit_value(RUNLIST_IOVA, 2);

        tracing::debug!(
            runlist_id = self.runlist_id,
            rl_base = format_args!("{rl_base:#010x}"),
            rl_submit = format_args!("{rl_submit:#010x}"),
            "submitting runlist (gv100 per-RL)"
        );

        bar0.write_u32(registers::pfifo::runlist_base(self.runlist_id), rl_base)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("runlist base: {e}"))))?;
        bar0.write_u32(registers::pfifo::runlist_submit(self.runlist_id), rl_submit)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("runlist submit: {e}"))))
    }
}

impl std::fmt::Debug for VfioChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VfioChannel")
            .field("channel_id", &self.channel_id)
            .field("instance_iova", &format_args!("{INSTANCE_IOVA:#x}"))
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_info_constants() {
        assert_eq!(VfioChannel::doorbell_offset(), 0x81_0090);
    }
}
