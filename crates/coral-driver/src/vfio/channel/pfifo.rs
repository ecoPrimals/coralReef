// SPDX-License-Identifier: AGPL-3.0-only
//! PFIFO engine initialization and diagnostic readback for Volta+ GPUs.
//!
//! Implements the engine bring-up sequence from nouveau's `gk104_fifo_init()`,
//! `gk104_fifo_init_pbdmas()`, `gf100_runq_init()`, and `gk208_runq_init()`.

use std::borrow::Cow;

use crate::error::{DriverError, DriverResult};
use crate::vfio::device::MappedBar;

use super::registers::*;

/// Behavioral knobs for the PFIFO bring-up sequence.
///
/// Differences between the `VfioChannel::create` path and the diagnostic
/// runner are expressed here rather than as code forks.
#[derive(Debug, Clone)]
pub struct PfifoInitConfig {
    /// Clear PRIV_RING faults (5× ACK retry) before touching engine regs.
    /// Needed after driver swap (nouveau → vfio); skippable on warm GPU.
    pub clear_priv_ring: bool,
    /// Write `0xFFFF_FFFF` to `PMC_ENABLE` to un-gate all engines.
    /// Diagnostic runner may prefer to preserve nouveau's PMC state.
    pub pmc_glow_plug: bool,
    /// Milliseconds to wait after `PFIFO_ENABLE = 1`.
    pub pfifo_settle_ms: u64,
    /// Re-clear PRIV_RING and retry if `PFIFO_ENABLE` reads back 0.
    pub retry_on_priv_fault: bool,
    /// Reset PFIFO via PMC_ENABLE bit 8 toggle. Set false for warm
    /// handoff to preserve the PFIFO scheduler state from nouveau.
    pub pmc_pfifo_reset: bool,
    /// Force-clear all PBDMA registers to remove stale channel state.
    /// Set false for warm handoff to preserve PBDMA configuration.
    pub pbdma_force_clear: bool,
    /// Flush all runlists with count=0 during init. On warm handoff,
    /// skip this: the empty flush tells FECS "no channels" which causes
    /// it to disable GR and halt. Our channel submit replaces the
    /// runlist immediately after, but FECS won't wake to process it.
    pub flush_empty_runlists: bool,
    /// Preempt all active runlists during init to clear stale channel
    /// state. On warm handoff, skip this: the preempt forces FECS to
    /// unload all channels; with none remaining FECS disables GR.
    pub preempt_runlists: bool,
    /// `true` → write `SCHED_EN (0x2504) = 1`; `false` → write `SCHED_DISABLE (0x2630) = 0`.
    pub use_sched_en: bool,
    /// Milliseconds to wait after empty-runlist flush.
    pub post_flush_settle_ms: u64,
}

impl Default for PfifoInitConfig {
    /// Standard init for `VfioChannel::create` — aggressive fault clearing,
    /// full glow plug, long settle, retry.
    fn default() -> Self {
        Self {
            clear_priv_ring: true,
            pmc_glow_plug: true,
            pfifo_settle_ms: 50,
            retry_on_priv_fault: true,
            pmc_pfifo_reset: true,
            pbdma_force_clear: true,
            flush_empty_runlists: true,
            preempt_runlists: true,
            use_sched_en: true,
            post_flush_settle_ms: 20,
        }
    }
}

impl PfifoInitConfig {
    /// Config for the diagnostic runner — lighter touch, preserves
    /// nouveau's warm state, shorter settle.
    #[must_use]
    pub fn diagnostic() -> Self {
        Self {
            clear_priv_ring: false,
            pmc_glow_plug: false,
            pfifo_settle_ms: 10,
            retry_on_priv_fault: false,
            pmc_pfifo_reset: false,
            pbdma_force_clear: false,
            flush_empty_runlists: false,
            preempt_runlists: false,
            use_sched_en: false,
            post_flush_settle_ms: 0,
        }
    }

    /// Config for warm handoff from nouveau — preserves PFIFO/PMC state
    /// left by nouveau. Skips PMC glow plug, PMC PFIFO reset, and PBDMA
    /// force-clear so falcon engines (FECS/GPCCS) remain alive.
    #[must_use]
    pub fn warm_handoff() -> Self {
        Self {
            clear_priv_ring: true,
            pmc_glow_plug: false,
            pfifo_settle_ms: 10,
            retry_on_priv_fault: true,
            pmc_pfifo_reset: false,
            pbdma_force_clear: false,
            flush_empty_runlists: false,
            preempt_runlists: false,
            use_sched_en: true,
            post_flush_settle_ms: 10,
        }
    }

    /// Hybrid config for FECS-frozen warm handoff (Exp 132 diesel engine).
    ///
    /// After `coralctl warm-fecs` with STOP_CTXSW, FECS firmware is alive
    /// in IMEM but scheduling is frozen. Nouveau's normal teardown has
    /// destroyed all channel, PBDMA, and runlist state (channels freed,
    /// runlists flushed, memory deallocated). `mc_reset` was NOPed by
    /// livepatch, preserving falcon IMEM.
    ///
    /// This config rebuilds the PFIFO infrastructure while preserving
    /// falcon state:
    /// - PMC_ENABLE: untouched (FECS/GPCCS engine bits stay enabled)
    /// - PMC PFIFO reset: skipped (would reset PFIFO clock domain)
    /// - PRIV ring: cleared (swap may leave stale faults)
    /// - PBDMA: force-cleared (nouveau's addresses are unmapped)
    /// - Runlists: flushed empty (clears stale nouveau entries)
    /// - Preempt: skipped (FECS scheduling is already frozen)
    /// - Scheduler: enabled (needed for dispatch after START_CTXSW)
    ///
    /// After channel creation with this config, send FECS `START_CTXSW`
    /// (method 0x02) to resume scheduling with the new channel.
    #[must_use]
    pub fn warm_fecs() -> Self {
        Self {
            clear_priv_ring: true,
            pmc_glow_plug: false,
            pfifo_settle_ms: 20,
            retry_on_priv_fault: true,
            pmc_pfifo_reset: false,
            pbdma_force_clear: true,
            flush_empty_runlists: true,
            preempt_runlists: false,
            use_sched_en: true,
            post_flush_settle_ms: 20,
        }
    }
}

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
#[expect(
    dead_code,
    reason = "convenience wrapper — used once channel orchestrator is fully wired"
)]
pub(super) fn init_pfifo_engine(bar0: &MappedBar) -> DriverResult<(u32, u32)> {
    init_pfifo_engine_with(bar0, &PfifoInitConfig::default())
}

/// Configurable PFIFO engine initialization.
///
/// Same as [`init_pfifo_engine`] but takes a [`PfifoInitConfig`] to
/// control the bring-up sequence. Use this from the diagnostic runner.
pub fn init_pfifo_engine_with(bar0: &MappedBar, cfg: &PfifoInitConfig) -> DriverResult<(u32, u32)> {
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

    // Clear stale PRIV_RING faults before touching engine registers.
    if cfg.clear_priv_ring {
        let priv_intr = bar0.read_u32(pri::PRIV_RING_INTR_STATUS).unwrap_or(0);
        if priv_intr != 0 {
            for attempt in 0..5 {
                w(pri::PRIV_RING_COMMAND, pri::PRIV_RING_CMD_ACK)?;
                std::thread::sleep(std::time::Duration::from_millis(20));
                let status = bar0.read_u32(pri::PRIV_RING_INTR_STATUS).unwrap_or(0);
                if status == 0 {
                    tracing::info!(attempt, "PRIV_RING fault cleared");
                    break;
                }
                if attempt == 4 {
                    tracing::warn!(
                        status = format_args!("{status:#010x}"),
                        "PRIV_RING fault persists after 5 ACK attempts"
                    );
                }
            }
        }
        let pmc_intr = bar0.read_u32(pri::PMC_INTR).unwrap_or(0);
        let priv_after = bar0.read_u32(pri::PRIV_RING_INTR_STATUS).unwrap_or(0);
        tracing::info!(
            priv_before = format_args!("{priv_intr:#010x}"),
            priv_after = format_args!("{priv_after:#010x}"),
            pmc_intr = format_args!("{pmc_intr:#010x}"),
            "PRIV_RING fault clear"
        );
    }

    // Glow plug — enable all engines in PMC_ENABLE (0x200).
    // NB: DEVICE_ENABLE (0x600) is NOT present on GV100 (returns 0xBAD00200
    // PBUS timeout). Do not write it.
    let pmc_before = bar0.read_u32(pmc::ENABLE).unwrap_or(0);
    if cfg.pmc_glow_plug {
        w(pmc::ENABLE, 0xFFFF_FFFF)?;
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let pmc_after = bar0.read_u32(pmc::ENABLE).unwrap_or(0);
    tracing::info!(
        pmc_before = format_args!("{pmc_before:#010x}"),
        pmc_after = format_args!("{pmc_after:#010x}"),
        "PMC glow plug"
    );

    // PMC-level PFIFO reset: bit 8 per gk104_mc_reset (NOT bit 1).
    // On GV100, bit 1 of PMC_ENABLE is not the PFIFO engine control.
    // nouveau's gk104_mc_reset uses device-specific engine→bit mappings;
    // for PFIFO (NVKM_ENGINE_FIFO) the bit is 8.
    if cfg.pmc_pfifo_reset {
        let pmc_cur = bar0.read_u32(pmc::ENABLE).unwrap_or(0);
        const PFIFO_BIT: u32 = 1 << 8;
        w(pmc::ENABLE, pmc_cur & !PFIFO_BIT)?;
        std::thread::sleep(std::time::Duration::from_millis(5));
        w(pmc::ENABLE, pmc_cur | PFIFO_BIT)?;
        std::thread::sleep(std::time::Duration::from_millis(10));
        let rb = bar0.read_u32(pmc::ENABLE).unwrap_or(0xDEAD);
        tracing::info!(
            pmc_cur = format_args!("{pmc_cur:#010x}"),
            pmc_after = format_args!("{rb:#010x}"),
            "PMC PFIFO reset (bit 8)"
        );
    } else {
        tracing::info!("PMC PFIFO reset skipped (warm handoff)");
    }

    // On GV100+ (Volta), NV_PFIFO_ENGINE (0x2200) does NOT exist — it reads 0
    // and writes are silently ignored. PFIFO is controlled purely via PMC_ENABLE
    // bit 8, and liveness is confirmed by checking PBDMA_MAP (non-zero = PBDMAs
    // discovered). Pre-Volta GPUs use the 0x2200 toggle normally.
    let is_volta_plus = (boot0 >> 20) & 0x1FF >= 0x140; // GV100 = 0x140
    if is_volta_plus {
        let pmc_cur = bar0.read_u32(pmc::ENABLE).unwrap_or(0);
        let pmc_pfifo_alive = pmc_cur & (1 << 8) != 0;
        let pbdma_map = bar0.read_u32(pfifo::PBDMA_MAP).unwrap_or(0);
        let pbdma_alive = pbdma_map != 0 && pbdma_map != 0xBAD0_DA00;
        tracing::info!(
            pmc = format_args!("{pmc_cur:#010x}"),
            pmc_pfifo_bit8 = pmc_pfifo_alive,
            pbdma_map = format_args!("{pbdma_map:#010x}"),
            pbdma_alive,
            "PFIFO enable (Volta+: PMC bit 8 + PBDMA_MAP, 0x2200 does not exist)"
        );
        if !pmc_pfifo_alive {
            tracing::warn!("PMC bit 8 (PFIFO) not set — enabling");
            let with_bit8 = pmc_cur | (1 << 8);
            w(pmc::ENABLE, with_bit8)?;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    } else {
        let pfifo_en = bar0.read_u32(pfifo::ENABLE).unwrap_or(0);
        w(pfifo::ENABLE, 0)?;
        std::thread::sleep(std::time::Duration::from_millis(1));
        w(pfifo::ENABLE, 1)?;
        std::thread::sleep(std::time::Duration::from_millis(cfg.pfifo_settle_ms));
        let mut readback = bar0.read_u32(pfifo::ENABLE).unwrap_or(0xDEAD);

        if readback == 0 && cfg.retry_on_priv_fault {
            tracing::warn!("PFIFO_ENABLE=0 after first write — retrying with PRI fault re-clear");
            let priv_st = bar0.read_u32(pri::PRIV_RING_INTR_STATUS).unwrap_or(0);
            if priv_st != 0 {
                for _ in 0..5 {
                    w(pri::PRIV_RING_COMMAND, pri::PRIV_RING_CMD_ACK)?;
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    if bar0.read_u32(pri::PRIV_RING_INTR_STATUS).unwrap_or(0) == 0 {
                        break;
                    }
                }
            }
            w(pfifo::ENABLE, 1)?;
            std::thread::sleep(std::time::Duration::from_millis(cfg.pfifo_settle_ms));
            readback = bar0.read_u32(pfifo::ENABLE).unwrap_or(0xDEAD);
        }
        tracing::info!(
            pfifo_before = format_args!("{pfifo_en:#010x}"),
            pfifo_after = format_args!("{readback:#010x}"),
            "PFIFO enable (pre-Volta: 0x2200 toggle)"
        );
    }

    let r = |reg: usize| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

    // Preempt ALL active runlists to clear the scheduler's stale channel
    // table from nouveau's previous session. On warm handoff this is
    // SKIPPED: the preempt forces FECS to unload all channels; with none
    // remaining FECS disables the GR engine and halts permanently.
    if cfg.preempt_runlists {
        let cur_map = r(pfifo::PBDMA_MAP);
        let mut rl_mask: u32 = 0;
        let mut seq = 0_usize;
        for pid in 0..32_usize {
            if cur_map & (1 << pid) == 0 {
                continue;
            }
            let rl = r(0x2390 + seq * 4);
            if rl < 32 {
                rl_mask |= 1 << rl;
            }
            seq += 1;
        }
        if rl_mask != 0 {
            w(pfifo::INTR, 0xFFFF_FFFF)?;
            w(pfifo::GV100_PREEMPT, rl_mask)?;
            let mut got_ack = false;
            for _ in 0..50 {
                std::thread::sleep(std::time::Duration::from_millis(2));
                let intr = r(pfifo::INTR);
                if intr & pfifo::INTR_RL_COMPLETE != 0 {
                    w(pfifo::INTR, pfifo::INTR_RL_COMPLETE)?;
                    got_ack = true;
                    break;
                }
            }
            tracing::info!(
                rl_mask = format_args!("{rl_mask:#010x}"),
                got_ack,
                "runlist preempt"
            );
        }
    } else {
        tracing::info!(
            "runlist preempt skipped (warm handoff — preserving FECS GR scheduling state)"
        );
    }

    // Force-clear PBDMA registers to remove nouveau's stale channel context.
    // This mirrors the diagnostic runner's Phase 4 — without it, PBDMAs may
    // attempt DMA fetches from nouveau's now-unmapped GPFIFO addresses.
    if cfg.pbdma_force_clear {
        let cur_map = r(pfifo::PBDMA_MAP);
        for pid in 0..32_usize {
            if cur_map & (1 << pid) == 0 {
                continue;
            }
            let b = 0x0004_0000 + pid * 0x2000;
            for off in (0x000..=0x1FC).step_by(4) {
                let _ = w(b + off, 0);
            }
            for off in [
                0x040, 0x044, 0x050, 0x054, 0x058, 0x0B0, 0x0D0, 0x0D4, 0x0C0, 0x13C,
            ] {
                let _ = w(b + off, 0);
            }
            let _ = w(b + 0x108, 0xFFFF_FFFF); // clear pending interrupts
            let _ = w(b + 0x110, 0);
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
        tracing::info!("PBDMA registers force-cleared");
    } else {
        // Warm handoff: don't nuke PBDMA register state, but DO clear
        // stale interrupt flags left by nouveau's teardown. Without this,
        // latched errors (GPPTR_INVALID, DEVICE, HCE) prevent the PBDMA
        // from scheduling our new channel after the runlist update.
        let cur_map = r(pfifo::PBDMA_MAP);
        for pid in 0..32_usize {
            if cur_map & (1 << pid) == 0 {
                continue;
            }
            let b = 0x0004_0000 + pid * 0x2000;
            let intr = bar0.read_u32(b + 0x108).unwrap_or(0);
            if intr != 0 {
                tracing::info!(
                    pbdma = pid,
                    intr = format_args!("{intr:#010x}"),
                    "warm handoff: clearing stale PBDMA interrupts"
                );
                let _ = w(b + 0x108, 0xFFFF_FFFF);
            }
        }
        tracing::info!("PBDMA force-clear skipped (warm handoff), interrupts cleared");
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
        let data = bar0.read_u32(0x0002_2700 + (i as usize) * 4).unwrap_or(0);
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
        let rl = bar0.read_u32(0x0000_2390 + seq * 4).unwrap_or(0xFFFF);
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

    {
        let ck = bar0.read_u32(pfifo::ENABLE).unwrap_or(0xDEAD);
        tracing::debug!(pfifo_en = format_args!("{ck:#010x}"), "after PBDMA init");
    }

    // Clear + enable PFIFO interrupts and scheduler.
    w(pfifo::INTR, 0xFFFF_FFFF)?;
    w(pfifo::INTR_EN, 0x7FFF_FFFF)?;
    if cfg.use_sched_en {
        w(pfifo::SCHED_EN, 1)?;
    } else {
        w(pfifo::SCHED_DISABLE, 0)?;
    }

    {
        let ck = bar0.read_u32(pfifo::ENABLE).unwrap_or(0xDEAD);
        let intr = bar0.read_u32(pfifo::INTR).unwrap_or(0xDEAD);
        tracing::debug!(
            pfifo_en = format_args!("{ck:#010x}"),
            intr = format_args!("{intr:#010x}"),
            "after scheduler enable"
        );
    }

    // GV100 per-runlist registers at stride 0x10 — flush with count=0.
    // On warm handoff this is SKIPPED: the empty flush tells FECS "no
    // channels on GR runlist" which causes FECS to disable the GR engine
    // and halt. Our channel submit replaces the runlist immediately after,
    // but FECS won't wake to process it. The preempt above already cleared
    // stale channels; our submit_runlist() will overwrite the runlist.
    if cfg.flush_empty_runlists {
        let mut flushed_runlists = std::collections::HashSet::new();
        let rl_base_val = pfifo::gv100_runlist_base_value(RUNLIST_IOVA);
        let rl_submit_val = pfifo::gv100_runlist_submit_value(RUNLIST_IOVA, 0);
        for &(_, rl) in &pbdma_runlists {
            if rl > 31 || !flushed_runlists.insert(rl) {
                continue;
            }
            w(pfifo::runlist_base(rl), rl_base_val)?;
            w(pfifo::runlist_submit(rl), rl_submit_val)?;
            std::thread::sleep(std::time::Duration::from_millis(10));
            let intr = bar0.read_u32(pfifo::INTR).unwrap_or(0);
            if intr & 0x4000_0000 != 0 {
                let _ = bar0.read_u32(pfifo::RUNLIST_ACK);
                w(pfifo::RUNLIST_ACK, 1u32 << rl)?;
                w(pfifo::INTR, 0x4000_0000)?;
                tracing::debug!(runlist = rl, "ACK'd empty runlist completion");
            }
            tracing::debug!(runlist = rl, "flushed runlist (empty, GV100 per-RL)");
        }
    } else {
        tracing::info!(
            "empty runlist flush skipped (warm handoff — preserving FECS GR scheduling state)"
        );
    }
    if cfg.post_flush_settle_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(cfg.post_flush_settle_ms));
    }

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

/// Build a minimal BAR2 page table in VRAM and program `NV_PBUS_BAR2_BLOCK`.
///
/// On a cold GPU (post-FLR / VFIO bind), BAR2_BLOCK reads `0x40000000` (invalid).
/// The PFIFO scheduler requires a configured BAR2 aperture — without it, channel
/// switches fail with `CHSW_ERROR=0x4` (RDAT_TIMEOUT).
///
/// This replicates what nouveau does in `gf100_bar_bar2_init()` +
/// `nvkm_vmm_boot()` + `gv100_vmm_join()`:
///
/// 1. Write a V2 5-level page table hierarchy in VRAM via the PRAMIN window
/// 2. Write a GV100 instance block with PDB + subcontext entries
/// 3. Program BAR2_BLOCK in VIRTUAL mode pointing to the instance block
/// 4. Identity-map the first 2 MiB of virtual address space to IOVAs
///
/// The 2 MiB identity map covers all our DMA buffer IOVAs (instance block,
/// runlist, page tables, USERD, GPFIFO, NOP push buffer, fault buffer).
#[expect(
    clippy::cast_possible_truncation,
    reason = "VRAM offsets and BAR2 addresses fit in u32"
)]
pub(super) fn setup_bar2_page_table(bar0: &MappedBar) -> DriverResult<()> {
    use super::registers::*;

    let w = |reg: usize, val: u32| {
        bar0.write_u32(reg, val)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("BAR2 init {reg:#x}: {e}"))))
    };
    let r = |reg: usize| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

    let bar2_before = r(misc::PBUS_BAR2_BLOCK);
    tracing::info!(
        bar2_before = format_args!("{bar2_before:#010x}"),
        "BAR2 setup start"
    );

    // Steer PRAMIN window to our VRAM region.
    let old_bar0_win = r(misc::BAR0_WINDOW);
    let bar0_win_val = BAR2_VRAM_BASE >> 16;
    w(misc::BAR0_WINDOW, bar0_win_val)?;
    std::thread::sleep(std::time::Duration::from_millis(1));

    let pm = misc::PRAMIN_BASE;

    // Zero-fill 24 KiB (6 pages) — clears any stale data.
    for off in (0..0x6000).step_by(4) {
        let _ = w(pm + off, 0);
    }

    // ── PD3 → PD2 ──────────────────────────────────────────────────
    // GP100 PDE: (child_vram_addr >> 4) | (aperture << 1)
    // VRAM aperture = 1 in bits[2:1]
    let pd2_abs = BAR2_VRAM_BASE + BAR2_PD2_OFF;
    let pd3_pde = ((pd2_abs >> 4) as u64) | (1_u64 << 1);
    w(pm + BAR2_PD3_OFF as usize, pd3_pde as u32)?;
    w(pm + BAR2_PD3_OFF as usize + 4, (pd3_pde >> 32) as u32)?;

    // ── PD2 → PD1 ──────────────────────────────────────────────────
    let pd1_abs = BAR2_VRAM_BASE + BAR2_PD1_OFF;
    let pd2_pde = ((pd1_abs >> 4) as u64) | (1_u64 << 1);
    w(pm + BAR2_PD2_OFF as usize, pd2_pde as u32)?;
    w(pm + BAR2_PD2_OFF as usize + 4, (pd2_pde >> 32) as u32)?;

    // ── PD1 → PD0 ──────────────────────────────────────────────────
    let pd0_abs = BAR2_VRAM_BASE + BAR2_PD0_OFF;
    let pd1_pde = ((pd0_abs >> 4) as u64) | (1_u64 << 1);
    w(pm + BAR2_PD1_OFF as usize, pd1_pde as u32)?;
    w(pm + BAR2_PD1_OFF as usize + 4, (pd1_pde >> 32) as u32)?;

    // ── PD0[0] → SPT (dual entry: lo=small PT, hi=large PT) ────────
    // PD0 dual PDEs require bit 4 (SPT_PRESENT) — without it the MMU
    // ignores the page table pointer entirely.
    let spt_abs = BAR2_VRAM_BASE + BAR2_SPT_OFF;
    let pd0_small_pde = ((spt_abs >> 4) as u64) | (1_u64 << 1) | (1_u64 << 4);
    // PD0 entry 0, bytes [0:7] = small page PDE
    w(pm + BAR2_PD0_OFF as usize, pd0_small_pde as u32)?;
    w(pm + BAR2_PD0_OFF as usize + 4, (pd0_small_pde >> 32) as u32)?;
    // bytes [8:15] = large page PDE (unused, already zeroed)

    // ── SPT: identity-map 512 × 4 KiB pages (2 MiB) ────────────────
    // GP100 PTE: (phys_addr >> 4) | VALID(bit0) | aper(bits[2:1]) | VOL(bit3)
    // SYS_MEM_COHERENT: aper=2, VOL=1 → flags = 1 | (2<<1) | (1<<3) = 0xD
    const PTE_FLAGS: u64 = 0xD; // VALID + SYS_MEM_COH + VOL
    for i in 1_u32..512 {
        let iova = (i as u64) * 4096;
        let pte = (iova >> 4) | PTE_FLAGS;
        let off = BAR2_SPT_OFF as usize + (i as usize) * 8;
        w(pm + off, pte as u32)?;
        w(pm + off + 4, (pte >> 32) as u32)?;
    }

    // ── Instance block (PDB + GV100 subcontexts) ────────────────────
    let pd3_abs = BAR2_VRAM_BASE + BAR2_PD3_OFF;
    // PDB format (gf100_vmm_join_ + gp100_vmm_join):
    //   pd_addr | VER2(bit10) | BIG_PAGE_64K(bit11) | target[1:0]
    // For VRAM target: bits[1:0] = 0
    let pdb_lo = pd3_abs | (1 << 10) | (1 << 11); // VER2 + 64KiB bigpage
    let pdb_hi = 0_u32;

    w(pm + BAR2_INST_OFF as usize + 0x200, pdb_lo)?;
    w(pm + BAR2_INST_OFF as usize + 0x204, pdb_hi)?;

    // VA limit (BAR2 aperture size - 1). 32 MiB is typical for Volta.
    w(pm + BAR2_INST_OFF as usize + 0x208, 0x01FF_FFFF)?; // 32 MiB - 1
    w(pm + BAR2_INST_OFF as usize + 0x20C, 0)?;

    // GV100 subcontext setup (gv100_vmm_join):
    // inst+0x21C = 0 (ENGINE_WFI_VEID)
    w(pm + BAR2_INST_OFF as usize + 0x21C, 0)?;

    // Subcontext 0: copy main PDB; subcontexts 1-63: invalid (0x00000001)
    let mask: u64 = 1; // only subcontext 0 valid
    w(pm + BAR2_INST_OFF as usize + 0x298, mask as u32)?;
    w(pm + BAR2_INST_OFF as usize + 0x29C, (mask >> 32) as u32)?;

    // SC0 gets the real PDB
    w(pm + BAR2_INST_OFF as usize + 0x2A0, pdb_lo)?;
    w(pm + BAR2_INST_OFF as usize + 0x2A4, pdb_hi)?;
    w(pm + BAR2_INST_OFF as usize + 0x2A8, 0)?;

    // SCs 1-63: mark invalid
    for i in 1_u32..64 {
        let base = BAR2_INST_OFF as usize + 0x2A0 + (i as usize) * 0x10;
        w(pm + base, 0x0000_0001)?;
        w(pm + base + 4, 0x0000_0001)?;
        w(pm + base + 8, 0)?;
    }

    // ── Program BAR1_BLOCK + BAR2_BLOCK ────────────────────────────
    // Both BAR apertures share the same instance block and page tables.
    // Format: MODE_VIRTUAL(bit31) | TARGET_VID_MEM(bits[29:28]=0) | PTR(inst >> 12)
    let inst_abs = BAR2_VRAM_BASE + BAR2_INST_OFF;
    let bar_block_val = 0x8000_0000_u32 | (inst_abs >> 12);
    w(misc::PBUS_BAR1_BLOCK, bar_block_val)?;
    w(misc::PBUS_BAR2_BLOCK, bar_block_val)?;

    // Wait for both BAR binds to complete (BIND_STATUS register).
    // bits [0:1] = BAR1 pending/outstanding, bits [2:3] = BAR2 pending/outstanding.
    for _ in 0..100 {
        let status = r(misc::PBUS_BIND_STATUS);
        if status & 0xF == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    // Flush GPU MMU TLB so the new BAR2 page table takes effect immediately.
    // Without this, the first scheduling attempt may fault because stale TLB
    // entries don't reflect the new page table.
    // Matches nouveau's gf100_vmm_invalidate: wait for flush slot, write PDB
    // address to 0x100CB8 (already done by MMU_PHYS_SECURE), then trigger
    // invalidate via 0x100CBC with PAGE_ALL | HUB_ONLY flags.
    {
        // Wait for flush slot availability (NV_PFB_PRI_MMU_INVALIDATE counter).
        for _ in 0..200 {
            if r(0x100C80) & 0x00FF_0000 != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
        // Write PDB address for BAR2 (VRAM target=0, addr >> 12 << 4).
        let pdb_inv = (pd3_abs >> 12) << 4;
        w(0x100CB8, pdb_inv)?;
        w(0x100CEC, 0)?; // high 32 bits
        // Trigger TLB invalidate: PAGE_ALL(bit0) + HUB_ONLY(bit2) + trigger(bit31).
        w(0x100CBC, 0x8000_0005)?;
        // Wait for flush acknowledgement.
        for _ in 0..200 {
            if r(0x100C80) & 0x0000_8000 != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
    }

    // Restore BAR0_WINDOW.
    w(misc::BAR0_WINDOW, old_bar0_win)?;

    let bar2_after = r(misc::PBUS_BAR2_BLOCK);
    tracing::info!(
        bar2_before = format_args!("{bar2_before:#010x}"),
        bar2_after = format_args!("{bar2_after:#010x}"),
        "BAR2 page table configured in VRAM"
    );

    Ok(())
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
