// SPDX-License-Identifier: AGPL-3.0-only

use std::borrow::Cow;
use std::os::fd::RawFd;
use std::time::Instant;

use crate::error::{DriverError, DriverResult};
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
#[expect(clippy::cast_possible_truncation, clippy::too_many_lines)]
pub fn diagnostic_matrix(
    container_fd: RawFd,
    bar0: &MappedBar,
    gpfifo_iova: u64,
    gpfifo_entries: u32,
    userd_iova: u64,
    channel_id: u32,
    configs: &[ExperimentConfig],
    gpfifo_ring: &mut [u8],
    userd_page: &mut [u8],
) -> DriverResult<Vec<ExperimentResult>> {
    let mut instance = DmaBuffer::new(container_fd, 4096, INSTANCE_IOVA)?;
    let mut runlist = DmaBuffer::new(container_fd, 4096, RUNLIST_IOVA)?;
    let mut pd3 = DmaBuffer::new(container_fd, 4096, PD3_IOVA)?;
    let mut pd2 = DmaBuffer::new(container_fd, 4096, PD2_IOVA)?;
    let mut pd1 = DmaBuffer::new(container_fd, 4096, PD1_IOVA)?;
    let mut pd0 = DmaBuffer::new(container_fd, 4096, PD0_IOVA)?;
    let mut pt0 = DmaBuffer::new(container_fd, 4096, PT0_IOVA)?;
    let mut nop_pb = DmaBuffer::new(container_fd, 4096, NOP_PB_IOVA)?;
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

    eprintln!("╔══ DIAGNOSTIC MATRIX — ONE-SHOT PROBES ═══════════════════╗");
    eprintln!("║ BOOT0:         {:#010x}", r(0));
    eprintln!("║ PMC_ENABLE:    {:#010x}", r(pmc::ENABLE));
    eprintln!("║ PFIFO_ENABLE:  {:#010x}", r(pfifo::ENABLE));
    eprintln!("║ SCHED_DISABLE: {:#010x}", r(0x2630));
    eprintln!("║ PFIFO_INTR:    {:#010x}", r(pfifo::INTR));
    eprintln!("║ PBDMA_MAP:     {:#010x}", r(pfifo::PBDMA_MAP));
    eprintln!("║ ENGN0_STATUS:  {:#010x}", r(0x2640));
    eprintln!("║ BIND_ERROR:    {:#010x}", r(0x252C));
    eprintln!("║ FB_TIMEOUT:    {:#010x}", r(0x2254));
    eprintln!("║ PRIV_RING:     {:#010x}", r(0x012070));
    eprintln!("║ ── MMU Fault Buffers ──");
    eprintln!(
        "║ BUF0_LO:  {:#010x}  BUF0_HI:  {:#010x}  SIZE: {:#010x}",
        r(0x100E24),
        r(0x100E28),
        r(0x100E2C)
    );
    eprintln!(
        "║ BUF0_GET: {:#010x}  BUF0_PUT: {:#010x}",
        r(0x100E30),
        r(0x100E34)
    );
    eprintln!(
        "║ BUF1_LO:  {:#010x}  BUF1_HI:  {:#010x}  SIZE: {:#010x}",
        r(0x100E44),
        r(0x100E48),
        r(0x100E4C)
    );
    eprintln!(
        "║ BUF1_GET: {:#010x}  BUF1_PUT: {:#010x}",
        r(0x100E50),
        r(0x100E54)
    );
    eprintln!("║ ── PCCSR Channel Scan ──");
    for ch in 0..8_u32 {
        let inst_val = r(pccsr::inst(ch));
        let chan_val = r(pccsr::channel(ch));
        if inst_val != 0 || chan_val != 0 {
            eprintln!("║ CH{ch}: INST={inst_val:#010x} CHAN={chan_val:#010x}");
        }
    }
    eprintln!("║ MMU_FAULT_STATUS: {:#010x}", r(0x100A2C));
    eprintln!(
        "║ MMU_FAULT_ADDR:   {:#010x}_{:#010x}",
        r(0x100A34),
        r(0x100A30)
    );
    eprintln!(
        "║ MMU_FAULT_INST:   {:#010x}_{:#010x}",
        r(0x100A3C),
        r(0x100A38)
    );

    // ── Warm state verification + self-warming ("glow plug") ──
    let pmc_en = r(pmc::ENABLE);
    let pfifo_en = r(pfifo::ENABLE);

    if pmc_en == 0xFFFF_FFFF {
        return Err(DriverError::SubmitFailed(Cow::Borrowed(
            "BAR0 returns 0xFFFFFFFF — GPU in D3hot (PCIe sleep). \
             Set power/control=on: echo on > /sys/bus/pci/devices/<BDF>/power/control",
        )));
    }

    let gpu_warm = pmc_en != 0x4000_0020 && pfifo_en != 0xBAD0_DA00;

    if !gpu_warm {
        eprintln!("╔══ GLOW PLUG — SELF-WARMING GPU ═══════════════════════════╗");
        eprintln!("║ PMC_ENABLE={pmc_en:#010x} → writing 0xFFFFFFFF to clock all engines");

        w(pmc::ENABLE, 0xFFFF_FFFF)?;
        std::thread::sleep(std::time::Duration::from_millis(50));

        let pmc_after = r(pmc::ENABLE);
        let pfifo_after = r(pfifo::ENABLE);
        eprintln!("║ PMC_ENABLE={pmc_after:#010x} (readback)");
        eprintln!("║ PFIFO_ENABLE={pfifo_after:#010x}");

        if pfifo_after != 1 {
            eprintln!("║ PFIFO not enabled ({pfifo_after:#010x}) — toggling PFIFO 0→1");
            let _ = w(pfifo::ENABLE, 0);
            std::thread::sleep(std::time::Duration::from_millis(10));
            let _ = w(pfifo::ENABLE, 1);
            std::thread::sleep(std::time::Duration::from_millis(50));
            let pfifo_retry = r(pfifo::ENABLE);
            eprintln!("║ PFIFO_ENABLE={pfifo_retry:#010x} (after toggle)");
        }

        let pmc_final = r(pmc::ENABLE);
        let pfifo_final = r(pfifo::ENABLE);
        let warmed = pmc_final != 0x4000_0020 && pfifo_final != 0xBAD0_DA00;
        eprintln!(
            "║ SELF-WARM: {}",
            if warmed { "SUCCESS ✓" } else { "FAILED ✗" }
        );
        eprintln!("╚═══════════════════════════════════════════════════════════╝");
    } else {
        eprintln!("║ WARM STATE:       WARM ✓ (PMC={pmc_en:#010x})");
        eprintln!("╚═══════════════════════════════════════════════════════════╝");
    }

    // ── BAR2 page table setup ──────────────────────────────────────────────
    // PFIFO scheduler requires BAR2_BLOCK (0x1714) configured. On a cold GPU
    // it reads 0x40000000 (invalid) → CHSW_ERROR RDAT_TIMEOUT.
    // Build a minimal V2 page table in VRAM and program BAR2_BLOCK.
    {
        let bar2_val = r(misc::PBUS_BAR2_BLOCK);
        let bar2_valid = bar2_val & 0x8000_0000 != 0 || (bar2_val != 0x4000_0000 && bar2_val != 0);
        if !bar2_valid {
            eprintln!("║ BAR2_BLOCK={bar2_val:#010x} (invalid) → building VRAM page table");
            super::super::pfifo::setup_bar2_page_table(bar0)?;
            let bar2_after = r(misc::PBUS_BAR2_BLOCK);
            eprintln!("║ BAR2_BLOCK={bar2_after:#010x} (after glow plug setup)");
        } else {
            eprintln!("║ BAR2_BLOCK={bar2_val:#010x} (already configured)");
        }
    }

    // ── MMU physical memory access + fault buffer init ────────────────────
    // NV_PFB_PRI_MMU_PHYS_SECURE controls what physical memory the GPU can DMA.
    // Oracle shows 0x002FFEDE0; our GPU has 0x0 → GPU can't access system memory!
    {
        let phys_sec_before = r(0x100CB8);
        if phys_sec_before == 0 {
            let _ = w(0x100CB8, 0x002F_FEDE_u32);
            let phys_sec_after = r(0x100CB8);
            eprintln!(
                "║ MMU_PHYS_SECURE: {phys_sec_before:#010x} → {phys_sec_after:#010x} (wrote oracle value)"
            );
        } else {
            eprintln!("║ MMU_PHYS_SECURE: {phys_sec_before:#010x} (already set)");
        }
    }

    // BUF0 = replayable fault buffer (oracle has this configured).
    let fault_buf = DmaBuffer::new(container_fd, 4096, FAULT_BUF_IOVA)?;
    fault_buf.as_slice(); // ensure mlock'd
    {
        let fb_addr_lo = (FAULT_BUF_IOVA >> 12) as u32;
        let fb_entries: u32 = 64; // 64 entries × 32 bytes = 2KB
        // BUF0 (replayable): oracle uses this one
        let _ = w(mmu::FAULT_BUF0_LO, fb_addr_lo);
        let _ = w(mmu::FAULT_BUF0_HI, 0);
        let _ = w(mmu::FAULT_BUF0_SIZE, fb_entries);
        let _ = w(mmu::FAULT_BUF0_PUT, 0x8000_0000); // CTRL bit 31 = enable
        let fb0_lo = r(mmu::FAULT_BUF0_LO);
        let fb0_ctrl = r(mmu::FAULT_BUF0_PUT);
        // BUF1 (non-replayable): try it too
        let _ = w(mmu::FAULT_BUF1_LO, fb_addr_lo);
        let _ = w(mmu::FAULT_BUF1_HI, 0);
        let _ = w(mmu::FAULT_BUF1_SIZE, fb_entries);
        let _ = w(mmu::FAULT_BUF1_PUT, 0x8000_0000);
        let fb1_lo = r(mmu::FAULT_BUF1_LO);
        let fb1_ctrl = r(mmu::FAULT_BUF1_PUT);
        eprintln!("║ FAULT_BUF0: LO={fb0_lo:#010x} CTRL={fb0_ctrl:#010x}");
        eprintln!("║ FAULT_BUF1: LO={fb1_lo:#010x} CTRL={fb1_ctrl:#010x}");
    }

    // ── PTIMER check ──────────────────────────────────────────────────────
    {
        let t0 = r(0x009400); // NV_PTIMER_TIME_0
        std::thread::sleep(std::time::Duration::from_millis(5));
        let t1 = r(0x009400);
        let ticking = t0 != t1 && t0 != 0xDEAD_DEAD && t0 != 0xBAD0_DA00;
        eprintln!("║ PTIMER: {t0:#010x} → {t1:#010x} ticking={ticking}");
        if !ticking {
            eprintln!("║ PTIMER NOT TICKING — DMA timeouts will not work!");
        }
    }

    // ── Oracle-compared register snapshot (after glow plug) ─────────────
    eprintln!("║ ── Post-warm Oracle Compare ──");
    eprintln!(
        "║ PMC_ENABLE:         {:#010x} (oracle: 0x5fecdff1)",
        r(pmc::ENABLE)
    );
    eprintln!(
        "║ BAR1_BLOCK(1704):   {:#010x} (oracle: 0x002ffeca)",
        r(misc::PBUS_BAR1_BLOCK)
    );
    eprintln!(
        "║ BAR2_BLOCK(1714):   {:#010x} (oracle: 0x802ffedf)",
        r(misc::PBUS_BAR2_BLOCK)
    );
    eprintln!(
        "║ MMU_PHYS_SECURE:    {:#010x} (oracle: 0x002ffede0)",
        r(0x100CB8)
    );
    eprintln!(
        "║ MMU_PHYS_CTRL:      {:#010x} (oracle: 0x000000000)",
        r(0x100CB4)
    );
    eprintln!(
        "║ PFIFO_INTR_EN:      {:#010x} (oracle: 0x061810101)",
        r(pfifo::INTR_EN)
    );
    eprintln!(
        "║ PMC_INTR_EN_0:      {:#010x} (oracle: 0x05f37ffff)",
        r(0x000140)
    );
    eprintln!(
        "║ PFIFO_PREEMPT(2634):{:#010x} (oracle: 0x001000002 — NOT SCHED_EN)",
        r(0x002634)
    );
    eprintln!(
        "║ CHSW_ERROR(256C):   {:#010x} (0=NO_ERROR)",
        r(pfifo::CHSW_ERROR)
    );

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
    eprintln!("║ Target runlist: {target_runlist}");

    // Dump ALL PBDMA → runlist mappings and engine info
    {
        let mut seq = 0_usize;
        for pid in 0..32_usize {
            if pbdma_map & (1 << pid) == 0 {
                continue;
            }
            let rl = r(0x2390 + seq * 4);
            eprintln!("║ PBDMA_RUNL_MAP[{seq}]: PBDMA {pid} → runlist {rl}");
            seq += 1;
        }
        // Also dump engine table at 0x22700
        eprintln!("║ ── Engine Table (0x22700) ──");
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
                eprintln!(
                    "║   ENGN_TABLE[{i}]: {data:#010x} — type={cur_type} runlist={cur_rl} (FINAL)"
                );
            } else {
                eprintln!("║   ENGN_TABLE[{i}]: {data:#010x} — kind={kind}");
            }
        }
        // Dump all engine statuses
        for eidx in 0..8_u32 {
            let status = r(0x2640 + (eidx as usize) * 4);
            if status != 0 {
                let rl_from_status = (status >> 12) & 0xF;
                eprintln!(
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
    eprintln!("║ Target PBDMA: {target_pbdma} (base={pb:#x})");
    if let Some((alt, alt_base)) = alt_pbdma.zip(pb2) {
        eprintln!("║ Alt PBDMA: {alt} (base={alt_base:#x})");
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
    eprintln!(
        "║ SCHED_DISABLE={:#010x} (0=scheduler runs)",
        r(pfifo::SCHED_DISABLE)
    );
    // Empty runlist flush to clear stale channels (gk104 format: global regs, no stride)
    {
        let rl_base_flush = (RUNLIST_IOVA >> 12) as u32 | (TARGET_SYS_MEM_COHERENT << 28);
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
            let _ = w(pfifo::RUNLIST_BASE, rl_base_flush);
            let _ = w(pfifo::RUNLIST_SUBMIT, (rl << 20) | 0); // count=0 → empty flush
            std::thread::sleep(std::time::Duration::from_millis(10));
            let intr = r(pfifo::INTR);
            let chsw = r(pfifo::CHSW_ERROR);
            if intr & pfifo::INTR_RL_COMPLETE != 0 {
                let _ = w(pfifo::RUNLIST_ACK, 1u32 << rl);
                let _ = w(pfifo::INTR, pfifo::INTR_RL_COMPLETE);
                eprintln!("║ Flush RL{rl}: BIT30 ACK'd ✓ CHSW={chsw:#x}");
            } else {
                let chsw_bit = intr & pfifo::INTR_CHSW_ERROR != 0;
                eprintln!(
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
    eprintln!(
        "║ PBDMA residual: USERD@xD0={residual_userd_lo:#010x} USERD@x08={residual_ramfc_userd_lo:#010x} GP_BASE={residual_gp_base_lo:#010x}"
    );

    // Comprehensive PBDMA register dump for all active PBDMAs
    eprintln!("║ ── Full PBDMA Register Dump ──");
    for pid in [0_usize, 1, 2, 3] {
        if pbdma_map & (1 << pid) == 0 && pid != 0 {
            continue;
        }
        let base = 0x40000 + pid * 0x2000;
        let active = pbdma_map & (1 << pid) != 0;
        eprint!("║ PBDMA{pid}{}:", if active { "" } else { "(off)" });
        for off in (0x00..=0x1FC_usize).step_by(4) {
            let val = r(base + off);
            if val != 0 {
                eprint!(" [{off:#05x}]={val:#010x}");
            }
        }
        eprintln!();
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
    eprintln!(
        "\n╔══ EXPERIMENT MATRIX ({} configs) ════════════════════════╗",
        configs.len()
    );
    eprintln!("║ {header}");
    eprintln!("║ {}", "─".repeat(header.len()));

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

        if first {
            first = false;
            let inst = instance.as_slice();
            let rd = |off: usize| u32::from_le_bytes(inst[off..off + 4].try_into().unwrap());
            eprintln!("║ ── DMA Buffer Verification (first experiment) ──");
            eprintln!(
                "║   RAMFC[0x008] USERD_LO   = {:#010x} (expect userd|tgt)",
                rd(ramfc::USERD_LO)
            );
            eprintln!(
                "║   RAMFC[0x00C] USERD_HI   = {:#010x}",
                rd(ramfc::USERD_HI)
            );
            eprintln!(
                "║   RAMFC[0x010] SIGNATURE  = {:#010x} (expect 0x0000FACE)",
                rd(ramfc::SIGNATURE)
            );
            eprintln!("║   RAMFC[0x030] ACQUIRE    = {:#010x}", rd(ramfc::ACQUIRE));
            eprintln!(
                "║   RAMFC[0x048] GP_BASE_LO = {:#010x}",
                rd(ramfc::GP_BASE_LO)
            );
            let rl = runlist.as_slice();
            let rr = |off: usize| u32::from_le_bytes(rl[off..off + 4].try_into().unwrap());
            eprintln!(
                "║   RL[0x010] ChanDW0       = {:#010x} (USERD_PTR|tgts|runq)",
                rr(0x10)
            );
            eprintln!(
                "║   RL[0x018] ChanDW2       = {:#010x} (INST_PTR|CHID)",
                rr(0x18)
            );
            eprintln!(
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
        // GK104/GV100 runlist register format (global, no stride):
        //   RUNLIST_BASE (0x2270): (target << 28) | (addr >> 12)
        //   RUNLIST_SUBMIT (0x2274): (runlist_id << 20) | count — triggers scheduler
        let rl_base = (RUNLIST_IOVA >> 12) as u32 | (cfg.runlist_base_target << 28);
        let rl_submit = (target_runlist << 20) | 2_u32;

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
        let host_gp_get = unsafe {
            std::ptr::read_volatile(userd_page.as_ptr().add(ramuserd::GP_GET).cast::<u32>())
        };
        let host_gp_put = unsafe {
            std::ptr::read_volatile(userd_page.as_ptr().add(ramuserd::GP_PUT).cast::<u32>())
        };

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
        eprintln!("║ {} [{exp_ms}ms]", result.summary_line());
        if result.chsw_error != 0 {
            eprintln!(
                "║   ⚠ CHSW_ERROR={:#x} ({}) PFIFO_INTR={:#010x}",
                result.chsw_error,
                result.chsw_error_name(),
                result.pfifo_intr,
            );
        }
        if result.scheduled && pb2.is_some() {
            eprintln!(
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
            pfifo::RUNLIST_BASE,
            (RUNLIST_IOVA >> 12) as u32 | (TARGET_SYS_MEM_COHERENT << 28),
        );
        let _ = w(pfifo::RUNLIST_SUBMIT, (target_runlist << 20) | 0); // empty flush
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
    eprintln!("╠══ SUMMARY ═══════════════════════════════════════════════════╣");
    eprintln!(
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
        eprintln!("║ ⚠ Faulted experiments:");
        for r in results.iter().filter(|r| r.faulted) {
            eprintln!(
                "║   {} PCCSR={:#010x} PBDMA_INTR={:#010x}",
                r.name, r.pccsr_chan, r.pbdma_intr
            );
        }
    }
    if num_chsw > 0 {
        eprintln!("║ ⚠ Channel switch errors:");
        for r in results.iter().filter(|r| r.chsw_error != 0) {
            eprintln!(
                "║   {} CHSW={:#x} ({})",
                r.name,
                r.chsw_error,
                r.chsw_error_name()
            );
        }
    }
    if num_gp_get > 0 {
        eprintln!("║ ★ GP_GET WRITEBACK — GPU wrote to host USERD:");
        for r in results.iter().filter(|r| r.userd_gp_get > 0) {
            eprintln!(
                "║   {} GP_GET={} GP_PUT={}",
                r.name, r.userd_gp_get, r.userd_gp_put
            );
        }
    }
    eprintln!("║ Final CHSW_ERROR: {:#x}", r(pfifo::CHSW_ERROR));
    eprintln!("║ Final PFIFO_INTR: {:#010x}", r(pfifo::INTR));
    eprintln!(
        "╚══ {total_ms}ms total, {} experiments ═══════════════════════╝",
        configs.len()
    );
    Ok(results)
}
