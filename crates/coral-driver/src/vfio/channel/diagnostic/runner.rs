// SPDX-License-Identifier: AGPL-3.0-only

use std::borrow::Cow;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::Instant;

use crate::error::{DriverError, DriverResult};
use crate::mmio::VolatilePtr;
use crate::vfio::device::MappedBar;
use crate::vfio::dma::DmaBuffer;

use super::super::page_tables::{
    populate_instance_block_static, populate_page_tables, populate_runlist_static, write_u32_le,
};
use super::super::registers::*;
use super::experiments::context::ExperimentContext;
use super::experiments::run_experiment;
use super::types::{ExperimentConfig, ExperimentOrdering, ExperimentResult};

/// Run the full diagnostic experiment matrix.
///
/// Allocates shared DMA buffers, runs PFIFO engine init ONCE, then iterates
/// over all configurations, capturing register snapshots for each.
///
/// The GPU should be warm from nouveau (bind nouveau → unbind → bind vfio-pci)
/// so the PFIFO scheduler is already running.
#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::too_many_arguments,
    reason = "diagnostic matrix needs all buffers and configs"
)]
pub fn diagnostic_matrix(
    container: Arc<OwnedFd>,
    bar0: &MappedBar,
    gpfifo_iova: u64,
    gpfifo_entries: u32,
    userd_iova: u64,
    channel_id: u32,
    configs: &[ExperimentConfig],
    gpfifo_ring: &mut [u8],
    userd_page: &mut [u8],
) -> DriverResult<Vec<ExperimentResult>> {
    let mut instance = DmaBuffer::new(Arc::clone(&container), 4096, INSTANCE_IOVA)?;
    let mut runlist = DmaBuffer::new(Arc::clone(&container), 4096, RUNLIST_IOVA)?;
    let mut pd3 = DmaBuffer::new(Arc::clone(&container), 4096, PD3_IOVA)?;
    let mut pd2 = DmaBuffer::new(Arc::clone(&container), 4096, PD2_IOVA)?;
    let mut pd1 = DmaBuffer::new(Arc::clone(&container), 4096, PD1_IOVA)?;
    let mut pd0 = DmaBuffer::new(Arc::clone(&container), 4096, PD0_IOVA)?;
    let mut pt0 = DmaBuffer::new(Arc::clone(&container), 4096, PT0_IOVA)?;
    let mut nop_pb = DmaBuffer::new(Arc::clone(&container), 4096, NOP_PB_IOVA)?;
    {
        let pb_mut = nop_pb.as_mut_slice();
        let nop_hdr: u32 = (1 << 29) | (1 << 16) | 0x40;
        pb_mut[0..4].copy_from_slice(&nop_hdr.to_le_bytes());
        pb_mut[4..8].copy_from_slice(&0_u32.to_le_bytes());
    }

    let w = |reg: usize, val: u32| -> DriverResult<()> {
        bar0.write_u32(reg, val)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("diag {reg:#x}: {e}"))))
    };
    let r = |reg: usize| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

    // ── One-shot probes ─────────────────────────────────────────────────

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

    // ── GlowPlug: warm GPU + BAR2 + fault buffers ─────────────────────
    let gp = crate::vfio::channel::glowplug::GlowPlug::new(bar0, Arc::clone(&container));
    let state = gp.check_state();
    tracing::debug!("╔══ GLOW PLUG — GPU STATE: {state:?} ════════════════════════╗");
    match state {
        crate::vfio::channel::glowplug::GpuThermalState::D3Hot => {
            return Err(DriverError::SubmitFailed(Cow::Borrowed(
                "BAR0 returns 0xFFFFFFFF — GPU in D3hot (PCIe sleep). \
                 Set power/control=on: echo on > /sys/bus/pci/devices/<BDF>/power/control",
            )));
        }
        crate::vfio::channel::glowplug::GpuThermalState::Warm => {
            tracing::debug!("║ GPU already warm — skipping glow plug");
        }
        _ => {
            let result = gp.full_init();
            for msg in &result.log {
                tracing::debug!("║ {msg}");
            }
            if !result.success {
                tracing::warn!("glow plug did not fully succeed");
            }
        }
    }
    let gpu_warm = !matches!(
        gp.check_state(),
        crate::vfio::channel::glowplug::GpuThermalState::D3Hot
            | crate::vfio::channel::glowplug::GpuThermalState::ColdGated
    );

    // DMA fault buffer — kept alive for the entire matrix run.
    let fault_buf = DmaBuffer::new(Arc::clone(&container), 4096, FAULT_BUF_IOVA)?;
    fault_buf.as_slice();

    // Oracle-compared register snapshot.
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

    // ── Shared init ─────────────────────────────────────────────────────

    let pbdma_map = r(pfifo::PBDMA_MAP);
    if pbdma_map == 0 || pbdma_map == 0xBAD0_DA00 {
        return Err(DriverError::SubmitFailed(Cow::Borrowed(
            "no PBDMAs after self-warm — PFIFO failed to initialize",
        )));
    }

    let mut gr_runlist: Option<u32> = None;
    let mut cur_type: u32 = 0xFFFF;
    let mut cur_runlist: u32 = 0xFFFF;
    for i in 0..64_u32 {
        let data = r(0x0002_2700 + (i as usize) * 4);
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
            }
            cur_type = 0xFFFF;
            cur_runlist = 0xFFFF;
        }
    }
    if gr_runlist.is_none() {
        let engn0 = r(0x2640);
        let rl = (engn0 >> 12) & 0xF;
        if rl <= 31 {
            gr_runlist = Some(rl);
        }
    }
    let target_runlist = gr_runlist.unwrap_or(0);
    tracing::debug!("║ Target runlist: {target_runlist}");

    // Dump ALL PBDMA → runlist mappings and engine info
    {
        let mut seq = 0_usize;
        for pid in 0..32_usize {
            if pbdma_map & (1 << pid) == 0 {
                continue;
            }
            let rl = r(0x2390 + seq * 4);
            tracing::debug!("║ PBDMA_RUNL_MAP[{seq}]: PBDMA {pid} → runlist {rl}");
            seq += 1;
        }
        // Also dump engine table at 0x22700
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
        // Dump all engine statuses
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

    // Find the PBDMA serving our GR runlist (used for all experiments)
    let mut target_pbdma: usize = 0;
    let mut alt_pbdma: Option<usize> = None;
    {
        let mut seq = 0_usize;
        let mut found_first = false;
        for pid in 0..32_usize {
            if pbdma_map & (1 << pid) == 0 {
                continue;
            }
            let rl = r(0x2390 + seq * 4);
            if rl == target_runlist {
                if !found_first {
                    target_pbdma = pid;
                    found_first = true;
                } else if alt_pbdma.is_none() {
                    alt_pbdma = Some(pid);
                }
            }
            seq += 1;
        }
    }
    let pb = 0x040000 + target_pbdma * 0x2000;
    let pb2 = alt_pbdma.map(|id| 0x040000 + id * 0x2000);
    tracing::debug!("║ Target PBDMA: {target_pbdma} (base={pb:#x})");
    if let Some((alt, alt_base)) = alt_pbdma.zip(pb2) {
        tracing::debug!("║ Alt PBDMA: {alt} (base={alt_base:#x})");
    }

    for id in 0..32_usize {
        if pbdma_map & (1 << id) == 0 {
            continue;
        }
        w(pbdma::intr(id), 0xFFFF_FFFF)?;
        w(pbdma::intr_en(id), 0xFFFF_FEFF)?;
        let b = 0x0004_0000 + id * 0x2000;
        w(b + 0x13C, 0)?;
        w(pbdma::hce_intr(id), 0)?;
        w(pbdma::hce_intr_en(id), 0)?;
        w(b + 0x164, 0xFFFF_FFFF)?;
    }

    w(pfifo::INTR, 0xFFFF_FFFF)?;
    w(pfifo::INTR_EN, 0x7FFF_FFFF)?;
    w(pfifo::SCHED_DISABLE, 0)?; // ensure scheduler is NOT disabled
    // NB: SCHED_EN (0x2504) does NOT exist on GV100 — writes cause MMIO fault (0xbad00200).
    // The oracle value at 0x2634 is actually NV_PFIFO_PREEMPT, not SCHED_EN.
    tracing::debug!(
        "║ SCHED_DISABLE={:#010x} (0=scheduler runs)",
        r(pfifo::SCHED_DISABLE)
    );
    // Empty runlist flush — GV100 per-runlist registers at stride 0x10.
    {
        let rl_base_val = pfifo::gv100_runlist_base_value(RUNLIST_IOVA);
        let rl_submit_val = pfifo::gv100_runlist_submit_value(RUNLIST_IOVA, 0);
        let mut flushed = std::collections::HashSet::new();
        let mut seq = 0_usize;
        for pid in 0..32_usize {
            if pbdma_map & (1 << pid) == 0 {
                continue;
            }
            let rl = r(0x2390 + seq * 4);
            seq += 1;
            if rl > 31 || !flushed.insert(rl) {
                continue;
            }
            let _ = w(pfifo::runlist_base(rl), rl_base_val);
            let _ = w(pfifo::runlist_submit(rl), rl_submit_val);
            std::thread::sleep(std::time::Duration::from_millis(10));
            let intr = r(pfifo::INTR);
            let chsw = r(pfifo::CHSW_ERROR);
            if intr & pfifo::INTR_RL_COMPLETE != 0 {
                let _ = w(pfifo::RUNLIST_ACK, 1u32 << rl);
                let _ = w(pfifo::INTR, pfifo::INTR_RL_COMPLETE);
                tracing::debug!("║ Flush RL{rl}: BIT30 ACK'd ✓ CHSW={chsw:#x}");
            } else {
                let chsw_bit = intr & pfifo::INTR_CHSW_ERROR != 0;
                tracing::debug!(
                    "║ Flush RL{rl}: no BIT30 (INTR={intr:#010x}) CHSW_ERR={chsw:#x} bit16={chsw_bit}"
                );
                if chsw_bit {
                    let _ = w(pfifo::INTR, pfifo::INTR_CHSW_ERROR);
                }
            }
        }
    }

    populate_page_tables(
        pd3.as_mut_slice(),
        pd2.as_mut_slice(),
        pd1.as_mut_slice(),
        pd0.as_mut_slice(),
        pt0.as_mut_slice(),
    );

    // Snapshot PBDMA residual state before any experiments (for comparison)
    let residual_userd_lo = r(pb + 0xD0);
    let residual_ramfc_userd_lo = r(pb + 0x08);
    let residual_gp_base_lo = r(pb + 0x40);
    tracing::debug!(
        "║ PBDMA residual: USERD@xD0={residual_userd_lo:#010x} USERD@x08={residual_ramfc_userd_lo:#010x} GP_BASE={residual_gp_base_lo:#010x}"
    );

    // Comprehensive PBDMA register dump for all active PBDMAs
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
                line.push_str(&format!(" [{off:#05x}]={val:#010x}"));
            }
        }
        tracing::debug!("{line}");
    }

    // ── Run experiment matrix ───────────────────────────────────────────

    let header = format!(
        "{:<42} | {:>8} | {:<5} | {:<5} | {:>14} | {:>19} | {:>3} | {:>9} | {:>8}",
        "Config",
        "PCCSR",
        "Fault",
        "Sched",
        "STATUS",
        "USERD D0=xD0 R8=x08",
        "Own",
        "GP pt/ft",
        "ENGN0"
    );
    tracing::debug!(
        "\n╔══ EXPERIMENT MATRIX ({} configs) ════════════════════════╗",
        configs.len()
    );
    tracing::debug!("║ {header}");
    tracing::debug!("║ {}", "─".repeat(header.len()));

    let limit2 = gpfifo_entries.ilog2();
    let mut results = Vec::with_capacity(configs.len());
    let mut first = true;
    let matrix_start = Instant::now();

    for cfg in configs {
        let exp_start = Instant::now();
        instance.as_mut_slice().fill(0);
        runlist.as_mut_slice().fill(0);

        populate_instance_block_static(
            instance.as_mut_slice(),
            gpfifo_iova,
            gpfifo_entries,
            userd_iova,
            channel_id,
        );

        populate_runlist_static(
            runlist.as_mut_slice(),
            userd_iova,
            channel_id,
            cfg.runlist_userd_target,
            cfg.runlist_inst_target,
            0,
        );

        // Flush ALL DMA buffers from CPU cache to ensure GPU sees latest data.
        // AMD Zen 2 VFIO DMA may not snoop CPU cache for all transaction types.
        #[cfg(target_arch = "x86_64")]
        {
            let flush = |slice: &[u8]| crate::vfio::cache_ops::clflush_range(slice);
            flush(instance.as_slice());
            flush(runlist.as_slice());
            flush(pd3.as_slice());
            flush(pd2.as_slice());
            flush(pd1.as_slice());
            flush(pd0.as_slice());
            flush(pt0.as_slice());
            flush(nop_pb.as_slice());
            flush(gpfifo_ring);
            flush(userd_page);
            crate::vfio::cache_ops::memory_fence();
        }

        if first {
            first = false;
            let inst = instance.as_slice();
            let rd = |off: usize| {
                u32::from_le_bytes(
                    inst[off..off + 4]
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
            let rl = runlist.as_slice();
            let rr = |off: usize| {
                u32::from_le_bytes(
                    rl[off..off + 4]
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

        // Clear stale PCCSR state
        let stale = r(pccsr::channel(channel_id));
        if stale & 1 != 0 {
            let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if stale & (pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET) != 0 {
            let _ = w(
                pccsr::channel(channel_id),
                pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let _ = w(pccsr::inst(channel_id), 0);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let _ = w(pfifo::INTR, 0xFFFF_FFFF);

        // Build PCCSR inst value
        let pccsr_inst_val = {
            let base = (INSTANCE_IOVA >> 12) as u32 | (cfg.pccsr_target << 28);
            match cfg.ordering {
                ExperimentOrdering::BindWithInstBindEnableRunlist
                | ExperimentOrdering::DirectPbdmaWithInstBind
                | ExperimentOrdering::FullDispatchWithInstBind
                | ExperimentOrdering::FullDispatchWithPreempt
                | ExperimentOrdering::ScheduledPlusDirectPbdma
                | ExperimentOrdering::VramFullDispatch => base | pccsr::INST_BIND_TRUE,
                _ => base,
            }
        };
        let rl_base = pfifo::gv100_runlist_base_value(RUNLIST_IOVA);
        let rl_submit = pfifo::gv100_runlist_submit_value(RUNLIST_IOVA, 2);

        {
            let mut ctx = ExperimentContext {
                bar0,
                channel_id,
                gpfifo_iova,
                userd_iova,
                instance: &mut instance,
                runlist: &mut runlist,
                gpfifo_ring,
                userd_page,
                target_runlist,
                target_pbdma,
                pbdma_base: pb,
                pbdma_map,
                pccsr_inst_val,
                rl_base_reg: pfifo::runlist_base(target_runlist),
                rl_submit_reg: pfifo::runlist_submit(target_runlist),
                rl_base,
                rl_submit,
                limit2,
                gpu_warm,
                cfg,
            };
            run_experiment(&mut ctx)?;
        }
        // Wait for hardware to process
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Capture snapshot
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        let pccsr_chan = r(pccsr::channel(channel_id));
        let pccsr_inst_rb = r(pccsr::inst(channel_id));
        let cur_userd_lo = r(pb + 0xD0);
        let cur_userd_hi = r(pb + 0xD4);
        let cur_ramfc_userd_lo = r(pb + 0x08);
        let cur_ramfc_userd_hi = r(pb + 0x0C);
        let cur_gp_base_lo = r(pb + 0x40);

        // Read GP_GET and GP_PUT from host USERD page via volatile reads
        // (GPU may have written to this DMA-mapped page)
        // SAFETY: userd_page is a valid DMA-mapped slice; ramuserd::GP_GET/GP_PUT are in-bounds
        // offsets; volatile required because GPU may have written to this shared memory.
        let vol_get = unsafe {
            VolatilePtr::new((userd_page.as_ptr().add(ramuserd::GP_GET) as *mut u8).cast::<u32>())
        };
        let vol_put = unsafe {
            VolatilePtr::new((userd_page.as_ptr().add(ramuserd::GP_PUT) as *mut u8).cast::<u32>())
        };
        let host_gp_get = vol_get.read();
        let host_gp_put = vol_put.read();

        let result = ExperimentResult {
            name: cfg.name.to_string(),
            pccsr_chan,
            pccsr_inst_readback: pccsr_inst_rb,
            pbdma_userd_lo: cur_userd_lo,
            pbdma_userd_hi: cur_userd_hi,
            pbdma_ramfc_userd_lo: cur_ramfc_userd_lo,
            pbdma_ramfc_userd_hi: cur_ramfc_userd_hi,
            pbdma_gp_base_lo: cur_gp_base_lo,
            pbdma_gp_base_hi: r(pb + 0x44),
            pbdma_gp_put: r(pb + 0x54),
            pbdma_gp_fetch: r(pb + 0x48),
            pbdma_channel_state: r(pb + 0xB0),
            pbdma_signature: r(pb + 0xC0),
            pfifo_intr: r(pfifo::INTR),
            mmu_fault_status: r(0x100A2C),
            engn0_status: r(0x2640),
            faulted: pccsr_chan & (pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET) != 0,
            scheduled: (pccsr_chan & 2) != 0,
            status: pccsr::status(pccsr_chan),
            pbdma_intr: r(pbdma::intr(target_pbdma)),
            alt_gp_put: pb2.map_or(0, |b| r(b + pbdma::GP_PUT)),
            alt_gp_fetch: pb2.map_or(0, |b| r(b + pbdma::GP_FETCH)),
            alt_gp_state: pb2.map_or(0, |b| r(b + pbdma::GP_STATE)),
            alt_ctx_userd: pb2.map_or(0, |b| r(b + pbdma::CTX_USERD_LO)),
            pbdma_ours: cur_userd_lo != residual_userd_lo
                || cur_ramfc_userd_lo != residual_ramfc_userd_lo
                || cur_gp_base_lo != residual_gp_base_lo,
            chsw_error: r(pfifo::CHSW_ERROR),
            userd_gp_get: host_gp_get,
            userd_gp_put: host_gp_put,
        };

        let exp_ms = exp_start.elapsed().as_millis();
        tracing::debug!("║ {} [{exp_ms}ms]", result.summary_line());
        if result.chsw_error != 0 {
            tracing::warn!(
                "CHSW_ERROR={:#x} ({}) PFIFO_INTR={:#010x}",
                result.chsw_error,
                result.chsw_error_name(),
                result.pfifo_intr,
            );
        }
        if result.scheduled && pb2.is_some() {
            tracing::debug!(
                "║   ALT_PBDMA{}: PUT={} FETCH={:#010x} STATE={:#010x} USERD={:#010x}",
                alt_pbdma.unwrap_or(0),
                result.alt_gp_put,
                result.alt_gp_fetch,
                result.alt_gp_state,
                result.alt_ctx_userd,
            );
        }

        // Tear down — full isolation between experiments
        let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let _ = w(
            pccsr::channel(channel_id),
            pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
        let _ = w(pccsr::inst(channel_id), 0);
        std::thread::sleep(std::time::Duration::from_millis(5));

        // Clear CTX registers to prevent contamination across experiments
        let _ = w(pb + pbdma::CTX_USERD_LO, 0);
        let _ = w(pb + pbdma::CTX_USERD_HI, 0);
        let _ = w(pb + pbdma::CTX_SIGNATURE, 0);
        let _ = w(pb + pbdma::CTX_GP_BASE_LO, 0);
        let _ = w(pb + pbdma::CTX_GP_BASE_HI, 0);
        let _ = w(pb + pbdma::CTX_ACQUIRE, 0);
        // Clear direct PBDMA state
        let _ = w(pb + pbdma::USERD_LO, 0);
        let _ = w(pb + pbdma::USERD_HI, 0);
        let _ = w(pb + pbdma::GP_BASE_LO, 0);
        let _ = w(pb + pbdma::GP_BASE_HI, 0);
        let _ = w(pb + pbdma::GP_PUT, 0);
        let _ = w(pb + pbdma::SIGNATURE, 0);
        // Clear PBDMA and PFIFO interrupts (including CHSW_ERROR bit 16)
        let _ = w(pbdma::intr(target_pbdma), 0xFFFF_FFFF);
        let _ = w(pfifo::INTR, 0xFFFF_FFFF);
        std::thread::sleep(std::time::Duration::from_millis(2));

        // Reset GPFIFO/USERD DMA buffers for next experiment
        gpfifo_ring.iter_mut().take(16).for_each(|b| *b = 0);
        write_u32_le(userd_page, ramuserd::GP_PUT, 0);
        write_u32_le(userd_page, ramuserd::GP_GET, 0);

        runlist.as_mut_slice().fill(0);
        let _ = w(
            pfifo::runlist_base(target_runlist),
            pfifo::gv100_runlist_base_value(RUNLIST_IOVA),
        );
        let _ = w(
            pfifo::runlist_submit(target_runlist),
            pfifo::gv100_runlist_submit_value(RUNLIST_IOVA, 0),
        );
        std::thread::sleep(std::time::Duration::from_millis(10));

        results.push(result);
    }

    let total_ms = matrix_start.elapsed().as_millis();
    let num_sched = results.iter().filter(|r| r.scheduled).count();
    let num_faulted = results.iter().filter(|r| r.faulted).count();
    let num_on_pbdma = results.iter().filter(|r| r.status >= 5).count();
    let num_chsw = results.iter().filter(|r| r.chsw_error != 0).count();
    let num_gp_fetch = results
        .iter()
        .filter(|r| r.pbdma_gp_fetch > 0 && r.pbdma_gp_fetch != r.pbdma_gp_base_lo)
        .count();
    let num_gp_get = results.iter().filter(|r| r.userd_gp_get > 0).count();
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
        for r in results.iter().filter(|r| r.faulted) {
            tracing::debug!(
                "║   {} PCCSR={:#010x} PBDMA_INTR={:#010x}",
                r.name,
                r.pccsr_chan,
                r.pbdma_intr
            );
        }
    }
    if num_chsw > 0 {
        tracing::warn!("Channel switch errors:");
        for r in results.iter().filter(|r| r.chsw_error != 0) {
            tracing::debug!(
                "║   {} CHSW={:#x} ({})",
                r.name,
                r.chsw_error,
                r.chsw_error_name()
            );
        }
    }
    if num_gp_get > 0 {
        tracing::debug!("║ ★ GP_GET WRITEBACK — GPU wrote to host USERD:");
        for r in results.iter().filter(|r| r.userd_gp_get > 0) {
            tracing::debug!(
                "║   {} GP_GET={} GP_PUT={}",
                r.name,
                r.userd_gp_get,
                r.userd_gp_put
            );
        }
    }
    tracing::debug!("║ Final CHSW_ERROR: {:#x}", r(pfifo::CHSW_ERROR));
    tracing::debug!("║ Final PFIFO_INTR: {:#010x}", r(pfifo::INTR));
    tracing::debug!(
        "╚══ {total_ms}ms total, {} experiments ═══════════════════════╝",
        configs.len()
    );
    Ok(results)
}
