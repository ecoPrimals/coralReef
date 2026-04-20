// SPDX-License-Identifier: AGPL-3.0-or-later

//! Verbose tracing helpers for [`diagnostic_matrix`](super::diagnostic_matrix).

use std::fmt::Write as _;

use crate::vfio::channel::diagnostic::types::ExperimentResult;
use crate::vfio::channel::registers::*;

pub(super) fn log_one_shot_probes<R: Fn(usize) -> u32>(r: R) {
    tracing::debug!("╔══ DIAGNOSTIC MATRIX — ONE-SHOT PROBES ═══════════════════╗");
    tracing::debug!("║ BOOT0:         {:#010x}", r(0));
    tracing::debug!("║ PMC_ENABLE:    {:#010x}", r(pmc::ENABLE));
    tracing::debug!("║ PFIFO_ENABLE:  {:#010x}", r(pfifo::ENABLE));
    tracing::debug!("║ SCHED_DISABLE: {:#010x}", r(0x2630));
    tracing::debug!("║ PFIFO_INTR:    {:#010x}", r(pfifo::INTR));
    tracing::debug!("║ PBDMA_MAP:     {:#010x}", r(pfifo::PBDMA_MAP));
    tracing::debug!("║ ENGN0_STATUS:  {:#010x}", r(0x2640));
    tracing::debug!("║ BIND_ERROR:    {:#010x}", r(0x252C));
    tracing::debug!("║ FB_TIMEOUT:    {:#010x}", r(0x2254));
    tracing::debug!("║ PRIV_RING:     {:#010x}", r(0x012070));
    tracing::debug!("║ ── MMU Fault Buffers ──");
    tracing::debug!(
        "║ BUF0_LO:  {:#010x}  BUF0_HI:  {:#010x}  SIZE: {:#010x}",
        r(0x100E24),
        r(0x100E28),
        r(0x100E2C)
    );
    tracing::debug!(
        "║ BUF0_GET: {:#010x}  BUF0_PUT: {:#010x}",
        r(0x100E30),
        r(0x100E34)
    );
    tracing::debug!(
        "║ BUF1_LO:  {:#010x}  BUF1_HI:  {:#010x}  SIZE: {:#010x}",
        r(0x100E44),
        r(0x100E48),
        r(0x100E4C)
    );
    tracing::debug!(
        "║ BUF1_GET: {:#010x}  BUF1_PUT: {:#010x}",
        r(0x100E50),
        r(0x100E54)
    );
    tracing::debug!("║ ── PCCSR Channel Scan ──");
    for ch in 0..8_u32 {
        let inst_val = r(pccsr::inst(ch));
        let chan_val = r(pccsr::channel(ch));
        if inst_val != 0 || chan_val != 0 {
            tracing::debug!("║ CH{ch}: INST={inst_val:#010x} CHAN={chan_val:#010x}");
        }
    }
    tracing::debug!("║ MMU_FAULT_STATUS: {:#010x}", r(0x100A2C));
    tracing::debug!(
        "║ MMU_FAULT_ADDR:   {:#010x}_{:#010x}",
        r(0x100A34),
        r(0x100A30)
    );
    tracing::debug!(
        "║ MMU_FAULT_INST:   {:#010x}_{:#010x}",
        r(0x100A3C),
        r(0x100A38)
    );
}

pub(super) fn log_post_warm_oracle_compare<R: Fn(usize) -> u32>(r: R) {
    tracing::debug!("║ ── Post-warm Oracle Compare ──");
    tracing::debug!(
        "║ PMC_ENABLE:         {:#010x} (oracle: 0x5fecdff1)",
        r(pmc::ENABLE)
    );
    tracing::debug!(
        "║ BAR1_BLOCK(1704):   {:#010x} (oracle: 0x002ffeca)",
        r(misc::PBUS_BAR1_BLOCK)
    );
    tracing::debug!(
        "║ BAR2_BLOCK(1714):   {:#010x} (oracle: 0x802ffedf)",
        r(misc::PBUS_BAR2_BLOCK)
    );
    tracing::debug!(
        "║ PFIFO_INTR_EN:      {:#010x} (oracle: 0x061810101)",
        r(pfifo::INTR_EN)
    );
    tracing::debug!(
        "║ CHSW_ERROR(256C):   {:#010x} (0=NO_ERROR)",
        r(pfifo::CHSW_ERROR)
    );
    tracing::debug!("╚═══════════════════════════════════════════════════════════╝");
}

pub(super) fn log_pbdma_runlist_and_engine_tables<R: Fn(usize) -> u32>(r: R, pbdma_map: u32) {
    let mut seq = 0_usize;
    for pid in 0..32_usize {
        if pbdma_map & (1 << pid) == 0 {
            continue;
        }
        let rl = r(0x2390 + seq * 4);
        tracing::debug!(seq, pid, runlist = rl, "PBDMA_RUNL_MAP");
        seq += 1;
    }
    tracing::debug!("║ ── Engine Table (0x22700) ──");
    let mut cur_type: u32 = 0xFFFF;
    let mut cur_rl: u32 = 0xFFFF;
    for i in 0..32_u32 {
        let data = r(0x2_2700 + (i as usize) * 4);
        if data == 0 {
            break;
        }
        let kind = data & 3;
        match kind {
            1 => cur_type = (data >> 2) & 0x3F,
            3 => cur_rl = (data >> 11) & 0x1F,
            _ => {}
        }
        if data & (1 << 31) != 0 {
            tracing::debug!(
                "║   ENGN_TABLE[{i}]: {data:#010x} — type={cur_type} runlist={cur_rl} (FINAL)"
            );
        } else {
            tracing::debug!("║   ENGN_TABLE[{i}]: {data:#010x} — kind={kind}");
        }
    }
    for eidx in 0..8_u32 {
        let status = r(0x2640 + (eidx as usize) * 4);
        if status != 0 {
            let rl_from_status = (status >> 12) & 0xF;
            tracing::debug!(
                "║   ENGN{eidx}_STATUS: {status:#010x} runlist_from_bits={rl_from_status}"
            );
        }
    }
}

pub(super) fn log_full_pbdma_register_dump<R: Fn(usize) -> u32>(r: R, pbdma_map: u32) {
    tracing::debug!("║ ── Full PBDMA Register Dump ──");
    for pid in [0_usize, 1, 2, 3] {
        if pbdma_map & (1 << pid) == 0 && pid != 0 {
            continue;
        }
        let base = 0x40000 + pid * 0x2000;
        let active = pbdma_map & (1 << pid) != 0;
        let mut line = format!("║ PBDMA{pid}{}:", if active { "" } else { "(off)" });
        for off in (0x00..=0x1FC_usize).step_by(4) {
            let val = r(base + off);
            if val != 0 {
                let _ = write!(line, " [{off:#05x}]={val:#010x}");
            }
        }
        tracing::debug!("{line}");
    }
}

pub(super) fn log_first_experiment_dma_buffers(
    instance: &[u8],
    runlist: &[u8],
    userd_iova: u64,
    gpfifo_iova: u64,
) {
    let rd = |off: usize| {
        u32::from_le_bytes(
            instance[off..off + 4]
                .try_into()
                .expect("DMA buffer slice is always 4 bytes"),
        )
    };
    tracing::debug!("║ ── DMA Buffer Verification (first experiment) ──");
    tracing::debug!(
        "║   RAMFC[0x008] USERD_LO   = {:#010x} (expect userd|tgt)",
        rd(ramfc::USERD_LO)
    );
    tracing::debug!(
        "║   RAMFC[0x00C] USERD_HI   = {:#010x}",
        rd(ramfc::USERD_HI)
    );
    tracing::debug!(
        "║   RAMFC[0x010] SIGNATURE  = {:#010x} (expect 0x0000FACE)",
        rd(ramfc::SIGNATURE)
    );
    tracing::debug!("║   RAMFC[0x030] ACQUIRE    = {:#010x}", rd(ramfc::ACQUIRE));
    tracing::debug!(
        "║   RAMFC[0x048] GP_BASE_LO = {:#010x}",
        rd(ramfc::GP_BASE_LO)
    );
    let rr = |off: usize| {
        u32::from_le_bytes(
            runlist[off..off + 4]
                .try_into()
                .expect("DMA buffer slice is always 4 bytes"),
        )
    };
    tracing::debug!(
        "║   RL[0x010] ChanDW0       = {:#010x} (USERD_PTR|tgts|runq)",
        rr(0x10)
    );
    tracing::debug!(
        "║   RL[0x018] ChanDW2       = {:#010x} (INST_PTR|CHID)",
        rr(0x18)
    );
    tracing::debug!(
        "║   userd_iova={userd_iova:#x} gpfifo_iova={gpfifo_iova:#x} instance_iova={INSTANCE_IOVA:#x}"
    );
}

pub(super) fn log_matrix_summary<R: Fn(usize) -> u32>(
    results: &[ExperimentResult],
    r: R,
    total_ms: u128,
    config_count: usize,
) {
    let num_sched = results.iter().filter(|rslt| rslt.scheduled).count();
    let num_faulted = results.iter().filter(|rslt| rslt.faulted).count();
    let num_on_pbdma = results.iter().filter(|rslt| rslt.status >= 5).count();
    let num_chsw = results.iter().filter(|rslt| rslt.chsw_error != 0).count();
    let num_gp_fetch = results
        .iter()
        .filter(|rslt| rslt.pbdma_gp_fetch_050 > 0)
        .count();
    let num_gp_get = results.iter().filter(|rslt| rslt.userd_gp_get > 0).count();
    tracing::debug!("╠══ SUMMARY ═══════════════════════════════════════════════════╣");
    tracing::debug!(
        "║ Total: {} | Scheduled: {} | ON_PBDMA+: {} | Faulted: {} | CHSW_ERR: {} | GP_FETCH advancing: {} | GP_GET writeback: {}",
        results.len(),
        num_sched,
        num_on_pbdma,
        num_faulted,
        num_chsw,
        num_gp_fetch,
        num_gp_get
    );
    if num_faulted > 0 {
        tracing::warn!("Faulted experiments:");
        for rslt in results.iter().filter(|rslt| rslt.faulted) {
            tracing::debug!(
                "║   {} PCCSR={:#010x} PBDMA_INTR={:#010x}",
                rslt.name,
                rslt.pccsr_chan,
                rslt.pbdma_intr
            );
        }
    }
    if num_chsw > 0 {
        tracing::warn!("Channel switch errors:");
        for rslt in results.iter().filter(|rslt| rslt.chsw_error != 0) {
            tracing::debug!(
                "║   {} CHSW={:#x} ({})",
                rslt.name,
                rslt.chsw_error,
                rslt.chsw_error_name()
            );
        }
    }
    if num_gp_get > 0 {
        tracing::debug!("║ ★ GP_GET WRITEBACK — GPU wrote to host USERD:");
        for rslt in results.iter().filter(|rslt| rslt.userd_gp_get > 0) {
            tracing::debug!(
                "║   {} GP_GET={} GP_PUT={}",
                rslt.name,
                rslt.userd_gp_get,
                rslt.userd_gp_put
            );
        }
    }
    tracing::debug!("║ Final CHSW_ERROR: {:#x}", r(pfifo::CHSW_ERROR));
    tracing::debug!("║ Final PFIFO_INTR: {:#010x}", r(pfifo::INTR));
    tracing::debug!(
        "╚══ {total_ms}ms total, {} experiments ═══════════════════════╝",
        config_count
    );
}
