// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kepler-specific VFIO channel creation (GK110/GK210).
//!
//! Kepler uses fundamentally different channel structures than Volta+:
//! - 2-level page tables (PD -> PT) instead of 5-level
//! - Simple instance block (no subcontexts, 40-bit VA limit)
//! - 8-byte global runlist entries (GK104 style) instead of TSG + 16-byte entries
//! - GK104 global runlist registers instead of GV100 per-runlist registers
//! - No MMU fault buffer setup (Kepler uses a different fault mechanism)

use std::borrow::Cow;

use crate::error::{DriverError, DriverResult};
use crate::vfio::device::{DmaBackend, MappedBar};
use crate::vfio::dma::DmaBuffer;

use super::registers::{self, pccsr, pfb};
use super::{
    page_tables, pfifo, FAULT_BUF_IOVA, INSTANCE_IOVA, PD0_IOVA, PD1_IOVA, PD2_IOVA, PD3_IOVA,
    PT0_IOVA, RUNLIST_IOVA, TARGET_SYS_MEM_COHERENT, VfioChannel,
};

impl VfioChannel {
    /// Create a VFIO channel for Kepler (GK110/GK210) GPUs.
    ///
    /// Kepler FECS can be loaded directly without ACR, so cold VFIO boot works.
    ///
    /// # Errors
    ///
    /// Returns error if any DMA allocation or BAR0 write fails.
    pub fn create_kepler(
        container: DmaBackend,
        guard: &crate::nv::vfio_compute::hardware_guard::GuardedBar<'_>,
        gpfifo_iova: u64,
        gpfifo_entries: u32,
        userd_iova: u64,
        channel_id: u32,
    ) -> DriverResult<Self> {
        let bar0 = guard.inner();
        let instance = DmaBuffer::new(container.clone(), 4096, INSTANCE_IOVA)?;
        let runlist = DmaBuffer::new(container.clone(), 4096, RUNLIST_IOVA)?;
        let pd3 = DmaBuffer::new(container.clone(), 4096, PD3_IOVA)?;
        let pd2 = DmaBuffer::new(container.clone(), 4096, PD2_IOVA)?;
        let pd1 = DmaBuffer::new(container.clone(), 4096, PD1_IOVA)?;
        let pd0 = DmaBuffer::new(container.clone(), 4096, PD0_IOVA)?;
        let pt0 = DmaBuffer::new(container.clone(), 4096, PT0_IOVA)?;
        let fault_buf = DmaBuffer::new(container.clone(), 4096, FAULT_BUF_IOVA)?;

        let mut chan = Self {
            instance,
            runlist,
            pd3,
            pd2,
            pd1,
            pd0,
            pt0,
            fault_buf,
            channel_id,
            runlist_id: 0,
        };

        let (_runq, target_runlist) = pfifo::init_pfifo_engine_kepler(guard)?;
        chan.runlist_id = target_runlist;

        {
            let bar2_val: u32 = 2 << 28;
            guard
                .write_u32(registers::misc::PBUS_BAR2_BLOCK as u32, bar2_val)
                .map_err(|refusal| {
                    DriverError::SubmitFailed(Cow::Owned(format!("BAR2_BLOCK: {refusal}")))
                })?;
            std::thread::sleep(std::time::Duration::from_millis(5));
            tracing::info!(
                bar2_block = format_args!("{bar2_val:#010x}"),
                "Kepler BAR2 set to PHYSICAL mode"
            );
        }

        page_tables::populate_kepler_page_tables(
            chan.pd3.as_mut_slice(),
            chan.pt0.as_mut_slice(),
            PT0_IOVA,
        );
        page_tables::populate_kepler_instance_block(
            chan.instance.as_mut_slice(),
            gpfifo_iova,
            gpfifo_entries,
            userd_iova,
            channel_id,
            PD3_IOVA,
        );
        page_tables::populate_kepler_runlist(
            chan.runlist.as_mut_slice(),
            INSTANCE_IOVA,
            channel_id,
        );

        invalidate_tlb_kepler(bar0, PD3_IOVA)?;

        let stale = bar0.read_u32(pccsr::channel(channel_id)).unwrap_or(0);
        if stale != 0 {
            Self::clear_stale_pccsr(bar0, channel_id, stale)?;
        }

        chan.bind_channel(bar0)?;
        std::thread::sleep(std::time::Duration::from_millis(5));
        chan.clear_channel_faults(bar0)?;

        let r = |reg: usize| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

        {
            let pccsr_inst = r(registers::pccsr::inst(channel_id));
            let pccsr_chan = r(registers::pccsr::channel(channel_id));
            let pfifo_en = r(registers::pfifo::ENABLE);
            let pmc_en = r(registers::pmc::ENABLE);
            tracing::info!(
                pccsr_inst = format_args!("{pccsr_inst:#010x}"),
                pccsr_chan = format_args!("{pccsr_chan:#010x}"),
                pfifo_en = format_args!("{pfifo_en:#010x}"),
                pmc_en = format_args!("{pmc_en:#010x}"),
                "Kepler: pre-enable state"
            );
        }

        chan.enable_channel(bar0)?;
        std::thread::sleep(std::time::Duration::from_millis(2));

        {
            let pccsr_chan = r(registers::pccsr::channel(channel_id));
            tracing::info!(
                pccsr_chan = format_args!("{pccsr_chan:#010x}"),
                status = registers::pccsr::status_name(pccsr_chan),
                "Kepler: post-enable, pre-runlist"
            );
        }

        let _ = bar0.write_u32(registers::pfifo::INTR, 0xFFFF_FFFF);
        let _ = bar0.write_u32(0x0000_252C_usize, 0);
        let pfifo_intr_pre = r(registers::pfifo::INTR);
        tracing::info!(
            pfifo_intr_pre = format_args!("{pfifo_intr_pre:#010x}"),
            "Kepler: PFIFO INTR cleared before runlist submit"
        );

        submit_runlist_kepler(&chan, bar0)?;

        let mut rl_done = false;
        for tick in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let intr = r(registers::pfifo::INTR);
            if intr & registers::pfifo::INTR_RL_COMPLETE != 0 {
                let _ = bar0.write_u32(registers::pfifo::INTR, registers::pfifo::INTR_RL_COMPLETE);
                tracing::info!(tick, "runlist completed (INTR bit 30 ACK)");
                rl_done = true;
                break;
            }
            if intr & 0x0000_0001 != 0 {
                let bind_err = r(0x0000_252C);
                tracing::warn!(
                    tick,
                    intr = format_args!("{intr:#010x}"),
                    bind_err = format_args!("{bind_err:#010x}"),
                    "BIND_ERROR during runlist submit"
                );
                break;
            }
            if intr != 0 && tick % 5 == 0 {
                tracing::debug!(
                    tick,
                    intr = format_args!("{intr:#010x}"),
                    "runlist poll: waiting"
                );
            }
        }
        if !rl_done {
            let intr = r(registers::pfifo::INTR);
            tracing::warn!(
                intr = format_args!("{intr:#010x}"),
                "runlist did not complete within 150ms"
            );
        }

        {
            let pccsr_chan = r(registers::pccsr::channel(channel_id));
            let pccsr_inst = r(registers::pccsr::inst(channel_id));
            let pfifo_intr = r(registers::pfifo::INTR);
            let rl_pending = r(registers::pfifo::RUNLIST_PENDING);
            let pmc_en = r(registers::pmc::ENABLE);
            let bind_err = r(0x0000_252C);
            let sched_err = r(0x0000_254C);
            let engn0_status = r(0x0000_2640);
            tracing::info!(
                pccsr_chan = format_args!("{pccsr_chan:#010x}"),
                pccsr_inst = format_args!("{pccsr_inst:#010x}"),
                pccsr_status = registers::pccsr::status_name(pccsr_chan),
                pfifo_intr = format_args!("{pfifo_intr:#010x}"),
                rl_pending = format_args!("{rl_pending:#010x}"),
                pmc_en = format_args!("{pmc_en:#010x}"),
                bind_err = format_args!("{bind_err:#010x}"),
                sched_err = format_args!("{sched_err:#010x}"),
                engn0 = format_args!("{engn0_status:#010x}"),
                "Kepler: post-runlist scheduler state"
            );
            if pfifo_intr & 0x0000_0001 != 0 {
                tracing::warn!(
                    bind_err = format_args!("{bind_err:#010x}"),
                    channel = (bind_err >> 8) & 0xFF,
                    "PFIFO BIND_ERROR detected"
                );
            }
            if pfifo_intr & 0x4000_0000 != 0 {
                let code = sched_err & 0x7F;
                let reason = match code {
                    0x0a | 0x0b => "CTXSW_TIMEOUT",
                    0x0c => "CTX_ILLEGAL_ACCESS",
                    0x0d => "MISSING_FENCE",
                    0x1e => "STATE_TIMEOUT",
                    0x1f => "SUBCHANNEL_TOKEN",
                    0x20 => "CONTEXT_RELOAD_TIMEOUT",
                    _ => "UNKNOWN",
                };
                tracing::warn!(
                    sched_err = format_args!("{sched_err:#010x}"),
                    code,
                    reason,
                    "PFIFO SCHED_ERROR detected"
                );
            }
            for pid in 0..3_usize {
                let b = 0x0004_0000 + pid * 0x2000;
                tracing::info!(
                    pbdma = pid,
                    state = format_args!("{:#010x}", r(b + 0x0B0)),
                    gp_base = format_args!("{:#010x}", r(b + 0x040)),
                    gp_put = format_args!("{:#010x}", r(b + 0x054)),
                    gp_get = format_args!("{:#010x}", r(b + 0x058)),
                    userd_lo = format_args!("{:#010x}", r(b + 0x0D0)),
                    intr = format_args!("{:#010x}", r(b + 0x108)),
                    signature = format_args!("{:#010x}", r(b + 0x0C0)),
                    "Kepler: PBDMA post-runlist state"
                );
            }
        }

        tracing::info!(
            channel_id,
            gpfifo_iova = format_args!("{gpfifo_iova:#x}"),
            userd_iova = format_args!("{userd_iova:#x}"),
            "Kepler VFIO PFIFO channel created"
        );

        Ok(chan)
    }
}

/// TLB invalidation for Kepler (GF100-style MMU).
fn invalidate_tlb_kepler(bar0: &MappedBar, pd_iova: u64) -> DriverResult<()> {
    for _ in 0..200 {
        let ctrl = bar0.read_u32(pfb::MMU_CTRL).unwrap_or(0);
        if ctrl & 0x00FF_0000 != 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    let pdb_inv = ((pd_iova >> 12) << 4) | 2;
    bar0.write_u32(pfb::MMU_INVALIDATE_PDB, pdb_inv as u32)
        .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("MMU_INVALIDATE_PDB: {e}"))))?;

    bar0.write_u32(pfb::MMU_INVALIDATE, 0x8000_0005)
        .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("MMU_INVALIDATE: {e}"))))?;

    for _ in 0..200 {
        let ctrl = bar0.read_u32(pfb::MMU_CTRL).unwrap_or(0);
        if ctrl & 0x0000_8000 != 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    tracing::info!(
        pd_iova = format_args!("{pd_iova:#x}"),
        "Kepler MMU TLB invalidated"
    );
    Ok(())
}

/// Submit runlist using GK104 global registers.
fn submit_runlist_kepler(chan: &VfioChannel, bar0: &MappedBar) -> DriverResult<()> {
    let rl_base =
        registers::pfifo::gk104_runlist_base_value(RUNLIST_IOVA, TARGET_SYS_MEM_COHERENT);
    let rl_submit = registers::pfifo::gk104_runlist_submit_value(chan.runlist_id, 1);

    tracing::info!(
        rl_base = format_args!("{rl_base:#010x}"),
        rl_submit = format_args!("{rl_submit:#010x}"),
        runlist_id = chan.runlist_id,
        "submitting Kepler runlist (GK104 global)"
    );

    bar0.write_u32(registers::pfifo::GK104_RUNLIST_BASE, rl_base)
        .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("runlist base: {e}"))))?;
    bar0.write_u32(registers::pfifo::GK104_RUNLIST_SUBMIT, rl_submit)
        .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("runlist submit: {e}"))))
}
