// SPDX-License-Identifier: AGPL-3.0-only
//! PFIFO engine initialization and diagnostic readback for Volta+ GPUs.
//!
//! Implements the engine bring-up sequence from nouveau's `gk104_fifo_init()`,
//! `gk104_fifo_init_pbdmas()`, `gf100_runq_init()`, and `gk208_runq_init()`.

use std::borrow::Cow;

use crate::error::{DriverError, DriverResult};
use crate::vfio::device::MappedBar;

use super::registers::*;

/// Enable the PFIFO engine in PMC, discover PBDMAs, and initialize.
///
/// Returns the RUNQ selector (0-based index into the PBDMAs serving
/// runlist 0) and the target runlist ID.
///
/// After VFIO FLR the GPU's engine clock domains are gated — PFIFO
/// registers read `0xBAD0_DA00`. We must enable the engine in
/// `NV_PMC_ENABLE` first, matching nouveau's `gp100_mc_init()`.
///
/// # Errors
///
/// Returns error if BAR0 reads indicate D3hot or no PBDMAs are found.
pub(super) fn init_pfifo_engine(bar0: &MappedBar) -> DriverResult<(u32, u32)> {
    let w = |reg: usize, val: u32| {
        bar0.write_u32(reg, val)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PFIFO init {reg:#x}: {e}"))))
    };

    let boot0 = bar0.read_u32(0).unwrap_or(0);
    if boot0 == 0xFFFF_FFFF {
        return Err(DriverError::SubmitFailed(Cow::Borrowed(
            "BAR0 returns 0xFFFFFFFF — GPU in D3hot (PCIe sleep). \
             Fix: echo on > /sys/bus/pci/devices/<BDF>/power/control",
        )));
    }

    // Glow plug — enable all engines in PMC.
    let pmc_before = bar0.read_u32(pmc::ENABLE).unwrap_or(0);
    w(pmc::ENABLE, 0xFFFF_FFFF)?;
    std::thread::sleep(std::time::Duration::from_millis(50));
    let pmc_after = bar0.read_u32(pmc::ENABLE).unwrap_or(0);

    tracing::info!(
        pmc_before = format_args!("{pmc_before:#010x}"),
        pmc_after = format_args!("{pmc_after:#010x}"),
        "PMC glow plug"
    );

    // Initialize PFIFO.
    let pfifo_en = bar0.read_u32(pfifo::ENABLE).unwrap_or(0);
    if pfifo_en == 0 || pfifo_en == 0xBAD0_DA00 {
        w(pfifo::ENABLE, 0)?;
        std::thread::sleep(std::time::Duration::from_millis(10));
        w(pfifo::ENABLE, 1)?;
        std::thread::sleep(std::time::Duration::from_millis(50));
        tracing::debug!(
            pfifo_en = format_args!("{pfifo_en:#010x}"),
            "PFIFO was disabled, toggled 0→1"
        );
    } else {
        tracing::debug!(
            pfifo_en = format_args!("{pfifo_en:#010x}"),
            "PFIFO already enabled"
        );
    }

    // Discover PBDMAs and their runlist assignments.
    let pbdma_map = bar0.read_u32(pfifo::PBDMA_MAP).unwrap_or(0);
    if pbdma_map == 0 {
        return Err(DriverError::SubmitFailed(Cow::Borrowed(
            "no PBDMAs found in PBDMA_MAP (0x2004)",
        )));
    }

    let mut gr_runlist: Option<u32> = None;
    let mut cur_type: u32 = 0xFFFF;
    let mut cur_runlist: u32 = 0xFFFF;
    for i in 0..64_u32 {
        let data = bar0
            .read_u32(pfifo::ENGN_TABLE + (i as usize) * 4)
            .unwrap_or(0);
        if data == 0 {
            break;
        }
        let kind = data & 3;
        match kind {
            1 => cur_type = (data >> 2) & 0x3F,
            3 => cur_runlist = (data >> 11) & 0x1F,
            _ => {}
        }
        if data & (1 << 31) != 0 {
            if cur_type == 0 && gr_runlist.is_none() && cur_runlist != 0xFFFF {
                gr_runlist = Some(cur_runlist);
                tracing::debug!(runlist = cur_runlist, "GR engine found");
            }
            cur_type = 0xFFFF;
            cur_runlist = 0xFFFF;
        }
    }

    let mut pbdma_ids: Vec<u32> = Vec::new();
    for pid in 0..32_u32 {
        if pbdma_map & (1 << pid) != 0 {
            pbdma_ids.push(pid);
        }
    }
    let mut pbdma_runlists: Vec<(u32, u32)> = Vec::new();
    for (seq, &pid) in pbdma_ids.iter().enumerate() {
        let rl = bar0
            .read_u32(pfifo::PBDMA_RUNL_MAP + seq * 4)
            .unwrap_or(0xFFFF);
        pbdma_runlists.push((pid, rl));
    }

    let target_runlist = gr_runlist.unwrap_or_else(|| pbdma_runlists.first().map_or(0, |e| e.1));

    tracing::info!(
        pbdma_map = format_args!("{pbdma_map:#010x}"),
        target_runlist,
        "PBDMA/runlist discovery"
    );

    // Per-PBDMA init (gk104_fifo_init_pbdmas + gk208_runq_init).
    for id in 0..32_usize {
        if pbdma_map & (1 << id) == 0 {
            continue;
        }
        let b = 0x0004_0000 + id * 0x2000;
        w(pbdma::intr(id), 0xFFFF_FFFF)?;
        w(pbdma::intr_en(id), 0xFFFF_FEFF)?;
        w(b + 0x13C, 0)?;
        w(pbdma::hce_intr(id), 0)?;
        w(pbdma::hce_intr_en(id), 0)?;
        w(b + 0x164, 0xFFFF_FFFF)?;
    }

    // Clear + enable PFIFO interrupts and scheduler.
    w(pfifo::INTR, 0xFFFF_FFFF)?;
    w(pfifo::INTR_EN, 0x7FFF_FFFF)?;
    w(pfifo::SCHED_EN, 1)?;

    // Submit empty runlists to flush stale channels (gk104 format).
    // RUNLIST_BASE (0x2270) = (target << 28) | (addr >> 12)
    // RUNLIST_SUBMIT (0x2274) = (runlist_id << 20) | count — triggers scheduler
    let mut flushed_runlists = std::collections::HashSet::new();
    #[expect(clippy::cast_possible_truncation)]
    let rl_base = (RUNLIST_IOVA >> 12) as u32 | (TARGET_SYS_MEM_COHERENT << 28);
    for &(_, rl) in &pbdma_runlists {
        if rl > 31 || !flushed_runlists.insert(rl) {
            continue;
        }
        w(pfifo::RUNLIST_BASE, rl_base)?;
        w(pfifo::RUNLIST_SUBMIT, rl << 20)?; // count=0 → empty flush
        std::thread::sleep(std::time::Duration::from_millis(10));
        let intr = bar0.read_u32(pfifo::INTR).unwrap_or(0);
        if intr & pfifo::INTR_RL_COMPLETE != 0 {
            let _ = bar0.read_u32(pfifo::RUNLIST_ACK);
            w(pfifo::RUNLIST_ACK, 1u32 << rl)?;
            w(pfifo::INTR, pfifo::INTR_RL_COMPLETE)?;
            tracing::debug!(runlist = rl, "ACK'd empty runlist completion");
        }
        tracing::debug!(runlist = rl, "flushed runlist (empty, gk104 format)");
    }
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Confirm GR runlist via ENGN0_STATUS register.
    let engn0 = bar0.read_u32(0x0000_2640).unwrap_or(0);
    let engn0_runlist = (engn0 >> 12) & 0xF;
    if gr_runlist.is_none() && engn0_runlist <= 31 {
        gr_runlist = Some(engn0_runlist);
    }
    let target_runlist = gr_runlist.unwrap_or_else(|| pbdma_runlists.first().map_or(0, |e| e.1));

    let runq: u32 = 0;
    tracing::info!(target_runlist, runq, "PFIFO engine initialized");
    Ok((runq, target_runlist))
}

/// Read back PFIFO/PBDMA/PCCSR state for diagnostics.
pub(super) fn log_pfifo_diagnostics(bar0: &MappedBar) {
    let r = |reg: usize| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

    let pfifo_intr = r(pfifo::INTR);
    let pfifo_en = r(pfifo::INTR_EN);
    let sched = r(pfifo::SCHED_EN);
    let pccsr_inst = r(pccsr::inst(0));
    let pccsr_chan = r(pccsr::channel(0));
    let pbdma0_intr = r(pbdma::intr(0));
    let pbdma0_hce = r(pbdma::hce_intr(0));
    let pbdma1_intr = r(pbdma::intr(1));
    let engn0_status = r(0x0000_2640);
    let pbdma0_idle = r(0x0000_3080);
    let pbdma1_idle = r(0x0000_3084);
    let rl0_info = r(0x0000_2284);
    let pmc_enable = r(0x0000_0200);
    let bind_err = r(0x0000_252C);
    let sched_dis = r(0x0000_2630);
    let preempt = r(0x0000_2634);
    let runl_submit_info = r(0x0000_2270);
    let doorbell_test = r(0x0081_0090);
    let pbdma_map = r(0x0000_2004);

    tracing::debug!(
        pmc_enable = format_args!("{pmc_enable:#010x}"),
        sched = format_args!("{sched:#010x}"),
        sched_dis = format_args!("{sched_dis:#010x}"),
        preempt = format_args!("{preempt:#010x}"),
        pfifo_intr = format_args!("{pfifo_intr:#010x}"),
        pfifo_en = format_args!("{pfifo_en:#010x}"),
        pccsr_inst = format_args!("{pccsr_inst:#010x}"),
        pccsr_chan = format_args!("{pccsr_chan:#010x}"),
        pbdma0_intr = format_args!("{pbdma0_intr:#010x}"),
        pbdma0_hce = format_args!("{pbdma0_hce:#010x}"),
        pbdma1_intr = format_args!("{pbdma1_intr:#010x}"),
        pbdma0_idle = format_args!("{pbdma0_idle:#010x}"),
        pbdma1_idle = format_args!("{pbdma1_idle:#010x}"),
        engn0_status = format_args!("{engn0_status:#010x}"),
        rl0_info = format_args!("{rl0_info:#010x}"),
        bind_err = format_args!("{bind_err:#010x}"),
        runl_submit_info = format_args!("{runl_submit_info:#010x}"),
        doorbell_test = format_args!("{doorbell_test:#010x}"),
        pbdma_map = format_args!("{pbdma_map:#010x}"),
        "PFIFO diagnostics"
    );

    let mut seq = 0_usize;
    for pid in 0..32_usize {
        if pbdma_map & (1 << pid) == 0 {
            continue;
        }
        let b = 0x040000 + pid * 0x2000;
        let rl_assign = r(0x2390 + seq * 4);
        tracing::debug!(
            pbdma = pid,
            seq,
            runlist = rl_assign,
            gp_base_hi = format_args!("{:#010x}", r(b + 0x44)),
            gp_base_lo = format_args!("{:#010x}", r(b + 0x40)),
            gp_put = format_args!("{:#010x}", r(b + 0x54)),
            gp_fetch = format_args!("{:#010x}", r(b + 0x48)),
            userd_hi = format_args!("{:#010x}", r(b + 0xD4)),
            userd_lo = format_args!("{:#010x}", r(b + 0xD0)),
            "PBDMA state"
        );
        seq += 1;
    }
}
