// SPDX-License-Identifier: AGPL-3.0-only
//! Hardware bring-up diagnostic experiment matrix for VFIO channel creation.

use std::borrow::Cow;
use std::os::fd::RawFd;

use crate::error::{DriverError, DriverResult};
use crate::vfio::device::MappedBar;
use crate::vfio::dma::DmaBuffer;

use super::page_tables::{
    populate_instance_block_static, populate_page_tables, populate_runlist_static, write_u32_le,
};
use super::registers::*;

/// Operation ordering for the diagnostic experiment.
#[derive(Debug, Clone, Copy)]
pub enum ExperimentOrdering {
    /// A: bind → enable → runlist (current production path)
    BindEnableRunlist,
    /// B: bind → runlist → enable
    BindRunlistEnable,
    /// C: runlist → bind → enable
    RunlistBindEnable,
    /// D: bind_with_INST_BIND → enable → runlist (force immediate context load)
    BindWithInstBindEnableRunlist,
    /// E: Direct PBDMA register programming — bypass scheduler entirely.
    /// Writes GP_BASE, USERD, SIGNATURE, etc. directly to PBDMA MMIO registers
    /// instead of submitting a runlist and waiting for the scheduler.
    DirectPbdmaProgramming,
    /// F: Direct PBDMA + PCCSR bind (with INST_BIND) — combine both paths.
    DirectPbdmaWithInstBind,
    /// G: Direct PBDMA + activate: reset GP_FETCH, write GPFIFO entry,
    /// set USERD GP_PUT=1, set PBDMA GP_PUT=1. Tests if PBDMA processes work.
    DirectPbdmaActivate,
    /// H: G + doorbell notification via NV_USERMODE_NOTIFY_CHANNEL_PENDING.
    DirectPbdmaActivateDoorbell,
    /// I: G + write PCCSR scheduled bit directly (bit 1).
    DirectPbdmaActivateScheduled,
    /// J: Instance block written to VRAM via PRAMIN, normal runlist submit + doorbell.
    VramInstanceBind,
    /// K: ALL structures in VRAM via PRAMIN — instance block, runlist, GPFIFO,
    /// USERD, page tables, push buffer. Eliminates ALL system memory access.
    AllVram,
    /// L: Hybrid — VRAM structures + direct PBDMA programming (skip INST_BIND).
    /// Combines proven direct-PBDMA-write approach with all-VRAM data.
    AllVramDirectPbdma,
    /// M: PFIFO engine reset + re-init — replicate nouveau's gk104_fifo_init() via
    /// PMC toggle (disable/enable bit 8), then normal scheduler path. Tests whether
    /// a fresh PFIFO reset restores scheduler operation.
    PfifoResetInit,
    /// N: INST_BIND + scheduled + GPFIFO work + doorbell — the full dispatch path.
    /// Combines D's working INST_BIND (which achieves SCHEDULED on warm GPU) with
    /// actual GPFIFO entry + USERD GP_PUT + doorbell notification. Tests whether
    /// the PBDMA processes work once properly scheduled.
    FullDispatchWithInstBind,
    /// O: N + PREEMPT to force context switch into PBDMA.
    /// Nouveau reference shows PREEMPT=0x01000001. The scheduler may need an
    /// explicit preempt to load the newly-scheduled channel into the PBDMA.
    FullDispatchWithPreempt,
    /// P: INST_BIND(SCHEDULED) + direct PBDMA writes + doorbell.
    /// Combines scheduler path (D=SCHEDULED) with manual PBDMA register
    /// injection from the RAMFC, then rings doorbell. If the scheduler can't
    /// load context itself, we'll do it manually.
    ScheduledPlusDirectPbdma,
    /// Q: Instance block in VRAM (via PRAMIN) + full dispatch path.
    /// Nouveau uses VRAM for instance blocks exclusively. On Volta, INST_BIND
    /// for system memory targets faults (0x11000000), and the PBDMA never loads
    /// context from RAMFC. This experiment writes the instance block to VRAM
    /// via the PRAMIN window, uses VRAM target in PCCSR_INST and runlist entry,
    /// then follows the full D+N dispatch path with doorbell.
    VramFullDispatch,
    /// T: I config (direct PBDMA at active offsets 0xD0/0x40) + doorbell AFTER
    /// SCHED bit set. Tests whether doorbell triggers GP_FETCH when channel is
    /// marked scheduled with directly programmed PBDMA registers.
    DirectPbdmaSchedDoorbell,
    /// R: RAMFC-mirror path — write USERD/GP_BASE/SIGNATURE to the
    /// RAMFC-mapped PBDMA context offsets (0x008, 0x048, 0x010) instead of
    /// direct offsets (0xD0, 0x40, 0xC0). + SCHED bit + doorbell.
    /// Tests hypothesis: PBDMA DMA engine reads from context-area registers.
    RamfcMirrorSchedDoorbell,
    /// S: Both register sets — write to RAMFC-mirror AND direct offsets.
    /// + SCHED bit + doorbell. Covers both hypotheses simultaneously.
    BothPathsSchedDoorbell,
    /// V: Pure scheduler path — enable channel, ensure SCHED_EN/SCHED_DISABLE
    /// are correct, submit runlist, ring doorbell. NO INST_BIND, NO direct PBDMA.
    /// Tests whether the GV100 hardware scheduler loads RAMFC context on its own
    /// when the scheduler is explicitly enabled.
    SchedulerPathOnly,
}

/// Configuration for a single experiment in the diagnostic matrix.
#[derive(Debug, Clone)]
pub struct ExperimentConfig {
    /// Human-readable experiment name.
    pub name: &'static str,
    /// PCCSR INST_TARGET: 2=COH, 3=NCOH
    pub pccsr_target: u32,
    /// Runlist channel entry DW0 USERD_TARGET: 2=COH, 3=NCOH
    pub runlist_userd_target: u32,
    /// Runlist channel entry DW2 INST_TARGET: 2=COH, 3=NCOH
    pub runlist_inst_target: u32,
    /// Runlist base register (0x2270) target: 2=COH, 3=NCOH
    pub runlist_base_target: u32,
    /// Operation ordering.
    pub ordering: ExperimentOrdering,
    /// Whether to skip the PFIFO_ENABLE toggle (leave as-is from nouveau).
    pub skip_pfifo_toggle: bool,
}

/// Result snapshot from a single experiment.
#[derive(Debug)]
pub struct ExperimentResult {
    /// Experiment name.
    pub name: String,
    /// PCCSR channel register value.
    pub pccsr_chan: u32,
    /// PCCSR inst register readback.
    pub pccsr_inst_readback: u32,
    /// PBDMA USERD low (offset 0xD0 — direct programming register).
    pub pbdma_userd_lo: u32,
    /// PBDMA USERD high (offset 0xD4 — direct programming register).
    pub pbdma_userd_hi: u32,
    /// PBDMA USERD low from RAMFC context load (offset 0x008).
    pub pbdma_ramfc_userd_lo: u32,
    /// PBDMA USERD high from RAMFC context load (offset 0x00C).
    pub pbdma_ramfc_userd_hi: u32,
    /// PBDMA GP_BASE low word.
    pub pbdma_gp_base_lo: u32,
    /// PBDMA GP_BASE high word.
    pub pbdma_gp_base_hi: u32,
    /// PBDMA GP_PUT register.
    pub pbdma_gp_put: u32,
    /// PBDMA GP_FETCH register — if this changes after we write, the PBDMA is alive.
    pub pbdma_gp_fetch: u32,
    /// PBDMA CHANNEL_STATE register.
    pub pbdma_channel_state: u32,
    /// PBDMA SIGNATURE register.
    pub pbdma_signature: u32,
    /// PFIFO interrupt status.
    pub pfifo_intr: u32,
    /// MMU fault status register.
    pub mmu_fault_status: u32,
    /// ENGN0 status register.
    pub engn0_status: u32,
    /// Whether PBDMA_FAULTED or ENG_FAULTED is set.
    pub faulted: bool,
    /// Whether PCCSR bit 1 (NEXT/scheduled) is set.
    pub scheduled: bool,
    /// Whether PBDMA registers changed from residual state (i.e. our writes stuck).
    pub pbdma_ours: bool,
}

impl ExperimentResult {
    /// Single-line summary for the experiment table.
    pub fn summary_line(&self) -> String {
        let pbdma_tag = if self.pbdma_ours { "OUR" } else { "old" };
        format!(
            "{:<42} | {:08x} | {:<5} | {:<5} | D0={:08x} R8={:08x} | {:>3} | gp={:02x}/{:02x} | {:08x}",
            self.name,
            self.pccsr_chan,
            if self.faulted { "FAULT" } else { "ok" },
            if self.scheduled { "SCHED" } else { "no" },
            self.pbdma_userd_lo,
            self.pbdma_ramfc_userd_lo,
            pbdma_tag,
            self.pbdma_gp_put,
            self.pbdma_gp_fetch,
            self.engn0_status,
        )
    }
}

/// Build the full experiment configuration matrix.
///
/// Generates scheduler-based experiments (A-D × encoding axes) plus
/// direct PBDMA programming experiments (E, F).
pub fn build_experiment_matrix() -> Vec<ExperimentConfig> {
    let mut configs = Vec::new();

    // ── Scheduler-based orderings (A-D) — reduced set ────────────────────
    // Prior runs proved encoding doesn't change outcomes on GV100: the
    // scheduler never loads RAMFC context regardless of target bits.
    // Keep one COH representative per ordering for regression coverage.
    // Exhaustive encoding sweeps can be re-enabled per-card as needed.

    let orderings = [
        (ExperimentOrdering::BindEnableRunlist, "A"),
        (ExperimentOrdering::BindRunlistEnable, "B"),
        (ExperimentOrdering::RunlistBindEnable, "C"),
        (ExperimentOrdering::BindWithInstBindEnableRunlist, "D"),
    ];

    for &(ordering, ord_name) in &orderings {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("{ord_name}_coh").into_boxed_str()),
            pccsr_target: 2,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 1, // GV100 aperture: 1=SYS_MEM_COH (best guess)
            ordering,
            skip_pfifo_toggle: true,
        });
    }

    // ── Q: VRAM instance block + full dispatch — run FIRST on warm GPU ──
    // Hypothesis: Volta PFIFO requires instance blocks in VRAM (like nouveau).
    // INST_BIND for system memory faults; PBDMA never loads RAMFC context.
    // PRAMIN writes to low VRAM offsets are non-destructive to warm state.
    for &(rl_utgt, rl_btgt, suffix) in &[
        (2_u32, 3_u32, "Ucoh"),
        (2, 2, "Ucoh_rlCoh"),
        (3, 3, "Uncoh"),
    ] {
        configs.push(ExperimentConfig {
            name: match suffix {
                "Ucoh" => "Q_vramInst_Ucoh",
                "Ucoh_rlCoh" => "Q_vramInst_Ucoh_rlCoh",
                _ => "Q_vramInst_Uncoh",
            },
            pccsr_target: 0, // VRAM
            runlist_userd_target: rl_utgt,
            runlist_inst_target: 0, // VRAM
            runlist_base_target: rl_btgt,
            ordering: ExperimentOrdering::VramFullDispatch,
            skip_pfifo_toggle: true,
        });
    }

    // ── N: Full dispatch (INST_BIND + GPFIFO + doorbell) — run early ────
    // Must run before VRAM/PRAMIN experiments (J/K/L) which can corrupt state.
    for &(pccsr_tgt, rl_utgt, rl_itgt, rl_btgt, suffix) in &[
        (2_u32, 2_u32, 3_u32, 3_u32, "coh"),
        (3, 2, 3, 2, "ncoh"),
        (2, 2, 2, 2, "allCoh"),
    ] {
        configs.push(ExperimentConfig {
            name: match suffix {
                "coh" => "N_fullDispatch_coh",
                "ncoh" => "N_fullDispatch_ncoh",
                _ => "N_fullDispatch_allCoh",
            },
            pccsr_target: pccsr_tgt,
            runlist_userd_target: rl_utgt,
            runlist_inst_target: rl_itgt,
            runlist_base_target: rl_btgt,
            ordering: ExperimentOrdering::FullDispatchWithInstBind,
            skip_pfifo_toggle: true,
        });
    }

    // ── O: Full dispatch + PREEMPT — force context switch ────────────────
    configs.push(ExperimentConfig {
        name: "O_dispatch_preempt_coh",
        pccsr_target: 2,
        runlist_userd_target: 2,
        runlist_inst_target: 3,
        runlist_base_target: 3,
        ordering: ExperimentOrdering::FullDispatchWithPreempt,
        skip_pfifo_toggle: true,
    });
    configs.push(ExperimentConfig {
        name: "O_dispatch_preempt_ncoh",
        pccsr_target: 3,
        runlist_userd_target: 2,
        runlist_inst_target: 3,
        runlist_base_target: 2,
        ordering: ExperimentOrdering::FullDispatchWithPreempt,
        skip_pfifo_toggle: true,
    });

    // ── P: Scheduled + direct PBDMA inject + doorbell ────────────────────
    configs.push(ExperimentConfig {
        name: "P_sched_directPbdma_coh",
        pccsr_target: 2,
        runlist_userd_target: 2,
        runlist_inst_target: 3,
        runlist_base_target: 3,
        ordering: ExperimentOrdering::ScheduledPlusDirectPbdma,
        skip_pfifo_toggle: true,
    });

    // ── Direct PBDMA experiments (E, F) — register write test ─────────

    for &(pccsr_t, pccsr_name) in &[(3_u32, "ncoh"), (2_u32, "coh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("E_direct_{pccsr_name}_noInstBind").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 3,
            ordering: ExperimentOrdering::DirectPbdmaProgramming,
            skip_pfifo_toggle: true,
        });
        configs.push(ExperimentConfig {
            name: Box::leak(format!("F_direct_{pccsr_name}_instBind").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 3,
            ordering: ExperimentOrdering::DirectPbdmaWithInstBind,
            skip_pfifo_toggle: true,
        });
    }

    // ── Direct PBDMA activation experiments (G, H, I) ───────────────────

    for &(pccsr_t, pccsr_name) in &[(3_u32, "ncoh"), (2_u32, "coh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("G_activate_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 3,
            ordering: ExperimentOrdering::DirectPbdmaActivate,
            skip_pfifo_toggle: true,
        });
        configs.push(ExperimentConfig {
            name: Box::leak(format!("H_activate_doorbell_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 3,
            ordering: ExperimentOrdering::DirectPbdmaActivateDoorbell,
            skip_pfifo_toggle: true,
        });
        configs.push(ExperimentConfig {
            name: Box::leak(format!("I_activate_sched_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 3,
            ordering: ExperimentOrdering::DirectPbdmaActivateScheduled,
            skip_pfifo_toggle: true,
        });
    }

    // ── T: Direct PBDMA + SCHED + doorbell (I + doorbell AFTER) ────────
    for &(pccsr_t, pccsr_name) in &[(2_u32, "coh"), (3_u32, "ncoh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("T_sched_doorbell_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 1,
            ordering: ExperimentOrdering::DirectPbdmaSchedDoorbell,
            skip_pfifo_toggle: true,
        });
    }

    // ── R: RAMFC-mirror PBDMA registers + SCHED + doorbell ───────────
    for &(pccsr_t, pccsr_name) in &[(2_u32, "coh"), (3_u32, "ncoh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("R_ramfc_sched_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 1,
            ordering: ExperimentOrdering::RamfcMirrorSchedDoorbell,
            skip_pfifo_toggle: true,
        });
    }

    // ── S: Both register paths + SCHED + doorbell ────────────────────
    for &(pccsr_t, pccsr_name) in &[(2_u32, "coh"), (3_u32, "ncoh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("S_both_sched_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 1,
            ordering: ExperimentOrdering::BothPathsSchedDoorbell,
            skip_pfifo_toggle: true,
        });
    }

    // ── V: Pure scheduler path (SCHED_EN + runlist, no direct PBDMA) ─
    for &(pccsr_t, pccsr_name) in &[(2_u32, "coh"), (3_u32, "ncoh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("V_scheduler_only_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 1,
            ordering: ExperimentOrdering::SchedulerPathOnly,
            skip_pfifo_toggle: true,
        });
    }

    // ── VRAM instance block experiments (J) ─────────────────────────────

    for &(rl_base_t, rl_name) in &[(3_u32, "rlNcoh"), (2_u32, "rlCoh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("J_vramInst_{rl_name}").into_boxed_str()),
            pccsr_target: 0,
            runlist_userd_target: 2,
            runlist_inst_target: 0,
            runlist_base_target: rl_base_t,
            ordering: ExperimentOrdering::VramInstanceBind,
            skip_pfifo_toggle: true,
        });
    }

    // ── ALL-VRAM experiment (K) — definitive scheduler test ─────────────

    configs.push(ExperimentConfig {
        name: "K_allVram",
        pccsr_target: 0,
        runlist_userd_target: 0,
        runlist_inst_target: 0,
        runlist_base_target: 0,
        ordering: ExperimentOrdering::AllVram,
        skip_pfifo_toggle: true,
    });

    // ── Hybrid VRAM + direct PBDMA (L) ──────────────────────────────────

    configs.push(ExperimentConfig {
        name: "L_vramDirectPbdma",
        pccsr_target: 0,
        runlist_userd_target: 0,
        runlist_inst_target: 0,
        runlist_base_target: 0,
        ordering: ExperimentOrdering::AllVramDirectPbdma,
        skip_pfifo_toggle: true,
    });

    // ── M: PFIFO engine reset + re-init ──────────────────────────────────
    for &(pccsr_tgt, rl_utgt, rl_itgt, rl_btgt, suffix) in &[
        (2_u32, 2_u32, 3_u32, 3_u32, "coh_Ucoh_Incoh_rlNcoh"),
        (3, 2, 3, 2, "ncoh_Ucoh_Incoh_rlCoh"),
    ] {
        configs.push(ExperimentConfig {
            name: match suffix {
                "coh_Ucoh_Incoh_rlNcoh" => "M_pfifoReset_coh",
                _ => "M_pfifoReset_ncoh",
            },
            pccsr_target: pccsr_tgt,
            runlist_userd_target: rl_utgt,
            runlist_inst_target: rl_itgt,
            runlist_base_target: rl_btgt,
            ordering: ExperimentOrdering::PfifoResetInit,
            skip_pfifo_toggle: true,
        });
    }

    configs
}

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

        if pfifo_after == 0xBAD0_DA00 || pfifo_after == 0xFFFF_FFFF {
            eprintln!("║ PFIFO still de-clocked after PMC write — toggling PFIFO 0→1");
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
    {
        let mut seq = 0_usize;
        for pid in 0..32_usize {
            if pbdma_map & (1 << pid) == 0 {
                continue;
            }
            let rl = r(0x2390 + seq * 4);
            if rl == target_runlist {
                target_pbdma = pid;
                break;
            }
            seq += 1;
        }
    }
    let pb = 0x040000 + target_pbdma * 0x2000;
    eprintln!("║ Target PBDMA: {target_pbdma} (base={pb:#x})");

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
        "{:<42} | {:>8} | {:<5} | {:<5} | {:>19} | {:>3} | {:>9} | {:>8}",
        "Config", "PCCSR", "Fault", "Sched", "USERD D0=xD0 R8=x08", "Own", "GP pt/ft", "ENGN0"
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

    for cfg in configs {
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
            eprintln!("║   RAMFC[0x00C] USERD_HI   = {:#010x}", rd(ramfc::USERD_HI));
            eprintln!(
                "║   RAMFC[0x010] SIGNATURE  = {:#010x} (expect 0x0000FACE)",
                rd(ramfc::SIGNATURE)
            );
            eprintln!("║   RAMFC[0x030] ACQUIRE    = {:#010x}", rd(ramfc::ACQUIRE));
            eprintln!("║   RAMFC[0x048] GP_BASE_LO = {:#010x}", rd(ramfc::GP_BASE_LO));
            let rl = runlist.as_slice();
            let rr = |off: usize| u32::from_le_bytes(rl[off..off + 4].try_into().unwrap());
            eprintln!(
                "║   RL[0x010] ChanDW0       = {:#010x} (USERD_PTR|tgts|runq)",
                rr(0x10)
            );
            eprintln!("║   RL[0x018] ChanDW2       = {:#010x} (INST_PTR|CHID)", rr(0x18));
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
        // GV100 runlist register format (per-runlist at stride 16):
        //   RUNLIST_BASE_LO (0x2270): pure address >> 12, no target bits
        //   RUNLIST_BASE_HI (0x2274): upper addr | aperture flag
        //   RUNLIST_SUBMIT  (0x2278): (count << 16) | start
        let rl_stride = target_runlist as usize * 16;
        let rl_base_lo = (RUNLIST_IOVA >> 12) as u32;
        let rl_base_hi = cfg.runlist_base_target; // aperture: 1=COH?, 2=VRAM, 3=NCOH?
        let rl_submit = 2_u32 << 16; // 2 entries (TSG + channel), start=0

        match cfg.ordering {
            ExperimentOrdering::BindEnableRunlist => {
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pfifo::RUNLIST_BASE_LO + rl_stride, rl_base_lo);
                let _ = w(pfifo::RUNLIST_BASE_HI + rl_stride, rl_base_hi);
                let _ = w(pfifo::RUNLIST_SUBMIT + rl_stride, rl_submit);
            }
            ExperimentOrdering::BindRunlistEnable => {
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pfifo::RUNLIST_BASE_LO + rl_stride, rl_base_lo);
                let _ = w(pfifo::RUNLIST_BASE_HI + rl_stride, rl_base_hi);
                let _ = w(pfifo::RUNLIST_SUBMIT + rl_stride, rl_submit);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
            }
            ExperimentOrdering::RunlistBindEnable => {
                let _ = w(pfifo::RUNLIST_BASE_LO + rl_stride, rl_base_lo);
                let _ = w(pfifo::RUNLIST_BASE_HI + rl_stride, rl_base_hi);
                let _ = w(pfifo::RUNLIST_SUBMIT + rl_stride, rl_submit);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
            }
            ExperimentOrdering::BindWithInstBindEnableRunlist => {
                // Clear RAMFC-mapped PBDMA registers to sentinels so we can
                // detect which ones the INST_BIND actually loads from RAMFC.
                let _ = w(pb + 0x08, 0xBEEF_0008); // USERD_LO
                let _ = w(pb + 0x0C, 0xBEEF_000C); // USERD_HI
                let _ = w(pb + 0x10, 0xBEEF_0010); // SIGNATURE
                let _ = w(pb + 0x30, 0xBEEF_0030); // ACQUIRE
                let _ = w(pb + 0x48, 0xBEEF_0048); // GP_BASE_LO
                let _ = w(pb + 0x4C, 0xBEEF_004C); // GP_BASE_HI

                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(10));

                // Snapshot immediately after INST_BIND (before enable/runlist)
                let ib_userd_lo = r(pb + 0x08);
                let ib_sig = r(pb + 0x10);
                let ib_gpb = r(pb + 0x48);
                eprintln!(
                    "║   D INST_BIND: R8={ib_userd_lo:#010x} SIG={ib_sig:#010x} GPB={ib_gpb:#010x} (BEEF=sentinel)"
                );

                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pfifo::RUNLIST_BASE_LO + rl_stride, rl_base_lo);
                let _ = w(pfifo::RUNLIST_BASE_HI + rl_stride, rl_base_hi);
                let _ = w(pfifo::RUNLIST_SUBMIT + rl_stride, rl_submit);
            }
            ExperimentOrdering::DirectPbdmaProgramming
            | ExperimentOrdering::DirectPbdmaWithInstBind => {
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(5));

                let _ = w(pb + 0x40, gpfifo_iova as u32);
                let _ = w(
                    pb + 0x44,
                    (gpfifo_iova >> 32) as u32
                        | (limit2 << 16)
                        | (PBDMA_TARGET_SYS_MEM_COHERENT << 28),
                );
                let _ = w(
                    pb + 0xD0,
                    (userd_iova as u32 & 0xFFFF_FE00) | PBDMA_TARGET_SYS_MEM_COHERENT,
                );
                let _ = w(pb + 0xD4, (userd_iova >> 32) as u32);
                let _ = w(pb + 0xC0, 0x0000_FACE);
                let _ = w(pb + 0xAC, 0x1000_3080);
                let _ = w(pb + 0xA8, 0x0000_1100);
                let _ = w(pb + 0x54, 0);
            }
            ExperimentOrdering::DirectPbdmaActivate
            | ExperimentOrdering::DirectPbdmaActivateDoorbell
            | ExperimentOrdering::DirectPbdmaActivateScheduled => {
                // Step 1: Bind instance to PCCSR + enable channel
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(5));

                // Step 2: Write PBDMA registers (same as E)
                let _ = w(pb + 0x40, gpfifo_iova as u32);
                let _ = w(
                    pb + 0x44,
                    (gpfifo_iova >> 32) as u32
                        | (limit2 << 16)
                        | (PBDMA_TARGET_SYS_MEM_COHERENT << 28),
                );
                let _ = w(
                    pb + 0xD0,
                    (userd_iova as u32 & 0xFFFF_FE00) | PBDMA_TARGET_SYS_MEM_COHERENT,
                );
                let _ = w(pb + 0xD4, (userd_iova >> 32) as u32);
                let _ = w(pb + 0xC0, 0x0000_FACE);
                let _ = w(pb + 0xAC, 0x1000_3080);
                let _ = w(pb + 0xA8, 0x0000_1100);

                // Step 3: Reset GP_FETCH and GP_STATE to 0
                let _ = w(pb + 0x48, 0);
                let _ = w(pb + 0x4C, 0);

                // Step 4: Write a GPFIFO entry into the ring buffer (slot 0).
                // Points to RUNLIST_IOVA (filled with zeros = NOP pushbuffer).
                // GPFIFO entry: DW0 = VA[31:2]|TYPE=0, DW1 = VA_HI|LEN_DWORDS<<10
                let gp_entry: u64 = (RUNLIST_IOVA & 0xFFFF_FFFC) | ((1_u64) << (32 + 10)); // 1 dword of NOP
                gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());

                // Step 5: Write USERD GP_PUT = 1 (host DMA memory)
                write_u32_le(userd_page, ramuserd::GP_PUT, 1);
                // Also zero USERD GP_GET
                write_u32_le(userd_page, ramuserd::GP_GET, 0);

                // Memory fence: ensure DMA writes are visible before MMIO
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

                // Step 6: Write PBDMA GP_PUT = 1 (via BAR0 MMIO)
                let _ = w(pb + 0x54, 1);

                // Step 7: Variant-specific activation
                if matches!(
                    cfg.ordering,
                    ExperimentOrdering::DirectPbdmaActivateDoorbell
                ) {
                    let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                }
                if matches!(
                    cfg.ordering,
                    ExperimentOrdering::DirectPbdmaActivateScheduled
                ) {
                    let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET | 0x2);
                }
            }
            ExperimentOrdering::VramInstanceBind => {
                const PRAMIN_BASE: usize = 0x0070_0000;
                const BAR0_WINDOW: usize = 0x0000_1700;
                const VRAM_INST_OFF: usize = 0x3000;

                let _ = w(BAR0_WINDOW, 0);
                std::thread::sleep(std::time::Duration::from_millis(1));

                // Copy instance block to VRAM
                let inst_bytes = instance.as_slice();
                for off in (0..inst_bytes.len()).step_by(4) {
                    let val = u32::from_le_bytes([
                        inst_bytes[off],
                        inst_bytes[off + 1],
                        inst_bytes[off + 2],
                        inst_bytes[off + 3],
                    ]);
                    let _ = w(PRAMIN_BASE + VRAM_INST_OFF + off, val);
                }

                let vram_sig = r(PRAMIN_BASE + VRAM_INST_OFF + ramfc::SIGNATURE);
                let vram_gpb = r(PRAMIN_BASE + VRAM_INST_OFF + ramfc::GP_BASE_LO);
                eprintln!("║   VRAM verify: SIG={vram_sig:#010x} GP_BASE={vram_gpb:#010x}");

                // Reset PBDMA registers to sentinel (0xBEEF) so we can
                // detect if the scheduler overwrites them with RAMFC values.
                let _ = w(pb + 0x40, 0xBEEF_0040); // GP_BASE_LO
                let _ = w(pb + 0x44, 0xBEEF_0044); // GP_BASE_HI
                let _ = w(pb + 0x48, 0); // GP_FETCH
                let _ = w(pb + 0x4C, 0); // GP_STATE
                let _ = w(pb + 0x54, 0); // GP_PUT
                let _ = w(pb + 0xD0, 0xBEEF_00D0); // USERD_LO
                let _ = w(pb + 0xD4, 0xBEEF_00D4); // USERD_HI
                let _ = w(pb + 0xC0, 0xBEEF_00C0); // SIGNATURE
                let _ = w(pb + 0xAC, 0xBEEF_00AC); // CHANNEL_INFO

                // Runlist: INST_TARGET=0 (VID_MEM)
                runlist.as_mut_slice().fill(0);
                populate_runlist_static(
                    runlist.as_mut_slice(),
                    userd_iova,
                    channel_id,
                    cfg.runlist_userd_target,
                    0, // INST_TARGET = VID_MEM
                    0,
                );

                // Flush GPU L2 cache so engines see our PRAMIN writes
                let _ = w(0x70010, 0x0000_0001);
                for _ in 0..2000_u32 {
                    if r(0x70010) & 3 == 0 {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }

                // PCCSR: TARGET=0 (VID_MEM), NO BIND — let scheduler load context
                let vram_pccsr = VRAM_INST_OFF as u32 >> 12;
                let _ = w(pccsr::inst(channel_id), vram_pccsr);
                std::thread::sleep(std::time::Duration::from_millis(5));

                let post_inst = r(pccsr::channel(channel_id));
                eprintln!("║   post-INST(noBind): {post_inst:#010x}");

                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(5));

                // Submit runlist
                let _ = w(pfifo::RUNLIST_BASE_LO + rl_stride, rl_base_lo);
                let _ = w(pfifo::RUNLIST_BASE_HI + rl_stride, rl_base_hi);
                let _ = w(pfifo::RUNLIST_SUBMIT + rl_stride, rl_submit);
                std::thread::sleep(std::time::Duration::from_millis(50));

                // Check PBDMA after scheduler should have loaded context
                let sched_gpb = r(pb + 0x40);
                let sched_userd = r(pb + 0xD0);
                let sched_sig = r(pb + 0xC0);
                let sched_state = r(pb + 0xB0);
                eprintln!(
                    "║   post-sched PBDMA: GP_BASE={sched_gpb:#010x} USERD={sched_userd:#010x} SIG={sched_sig:#010x} STATE={sched_state:#010x}"
                );

                // Set up GPFIFO entry + USERD GP_PUT + doorbell
                let gp_entry: u64 = (RUNLIST_IOVA & 0xFFFF_FFFC) | ((1_u64) << (32 + 10));
                gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
                write_u32_le(userd_page, ramuserd::GP_PUT, 1);
                write_u32_le(userd_page, ramuserd::GP_GET, 0);
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

                let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
            }
            ExperimentOrdering::AllVram => {
                // Put EVERY structure in VRAM via PRAMIN.
                // VRAM layout (all within 64KB PRAMIN window at BAR0_WINDOW=0):
                //   0x0000 PD3, 0x1000 PD2, 0x2000 PD1, 0x3000 PD0,
                //   0x4000 PT0, 0x8000 Instance, 0x9000 GPFIFO,
                //   0xA000 USERD, 0xB000 NOP pushbuf, 0xC000 Runlist
                const PM: usize = 0x0070_0000;
                const BW: usize = 0x0000_1700;
                let _ = w(BW, 0);
                std::thread::sleep(std::time::Duration::from_millis(1));

                // Helper: write u32 to VRAM offset via PRAMIN
                let wv = |off: usize, val: u32| -> DriverResult<()> {
                    w(PM + off, val)
                };

                // Zero critical regions first (64KB is too big; just zero what we use)
                for off in (0..0xD000_usize).step_by(4) {
                    let _ = wv(off, 0);
                }

                // ── Page tables (all VRAM, VID_MEM aperture) ──
                // PDE format: bits[1:0]=aperture(1=VID_MEM), bit[2]=VOL
                // PTE format: bit[0]=VALID, bits[2:1]=aperture(0=VID_MEM), bit[3]=VOL
                let vram_pde = |addr: u64| -> u64 {
                    (addr >> 4) | 1 // aperture=VID_MEM(1)
                };
                let vram_pte = |addr: u64| -> u64 {
                    (addr >> 4) | 1 // VALID=1, aperture=VID_MEM(0)
                };

                // PD3[0] → PD2 at VRAM 0x1000
                let e = vram_pde(0x1000);
                let _ = wv(0x0000, e as u32);
                let _ = wv(0x0004, (e >> 32) as u32);

                // PD2[0] → PD1 at VRAM 0x2000
                let e = vram_pde(0x2000);
                let _ = wv(0x1000, e as u32);
                let _ = wv(0x1004, (e >> 32) as u32);

                // PD1[0] → PD0 at VRAM 0x3000
                let e = vram_pde(0x3000);
                let _ = wv(0x2000, e as u32);
                let _ = wv(0x2004, (e >> 32) as u32);

                // PD0 dual entry: [0]=big page PDE(invalid), [8]=small page PDE → PT0
                let e = vram_pde(0x4000);
                let _ = wv(0x3008, e as u32);
                let _ = wv(0x300C, (e >> 32) as u32);

                // PT0: identity map VRAM pages 0..0xC (covering 0x0000-0xCFFF)
                for page in 0..13_usize {
                    let phys = (page as u64) * 4096;
                    let e = vram_pte(phys);
                    let off = 0x4000 + page * 8;
                    let _ = wv(off, e as u32);
                    let _ = wv(off + 4, (e >> 32) as u32);
                }

                // ── Instance block at VRAM 0x8000 ──
                // RAMFC: all addresses are VRAM physical, PBDMA target=0 (VID_MEM)
                let inst_base = 0x8000_usize;

                // RAMIN: PDB → PD3 at VRAM 0x0000
                // Matches populate_instance_block_static format but with VID_MEM target
                let pdb_lo: u32 = ((0_u64 >> 12) as u32) << 12
                    | (1 << 11)             // fault replay
                    | (1 << 10)             // fault replay
                    | (1 << 2)              // VOL
                    | 1;                    // aperture=VID_MEM(1)
                let _ = wv(inst_base + ramin::PAGE_DIR_BASE_LO, pdb_lo);
                let _ = wv(inst_base + ramin::PAGE_DIR_BASE_HI, 0);
                let _ = wv(inst_base + ramin::ENGINE_WFI_VEID, 0);
                let _ = wv(inst_base + ramin::SC_PDB_VALID, 1);
                let _ = wv(inst_base + ramin::SC0_PAGE_DIR_BASE_LO, pdb_lo);
                let _ = wv(inst_base + ramin::SC0_PAGE_DIR_BASE_HI, 0);

                // RAMFC fields (PBDMA target 0 = VID_MEM)
                let gpfifo_vram: u64 = 0x9000;
                let userd_vram: u64 = 0xA000;
                let _ = wv(inst_base + ramfc::GP_BASE_LO, gpfifo_vram as u32);
                let _ = wv(inst_base + ramfc::GP_BASE_HI,
                    (gpfifo_vram >> 32) as u32
                    | (limit2 << 16)
                    | (0_u32 << 28)); // PBDMA_TARGET_VID_MEM = 0
                let _ = wv(inst_base + ramfc::USERD_LO,
                    (userd_vram as u32 & 0xFFFF_FE00) | 0); // target=VID_MEM
                let _ = wv(inst_base + ramfc::USERD_HI, 0);
                let _ = wv(inst_base + ramfc::SIGNATURE, 0x0000_FACE);
                let _ = wv(inst_base + ramfc::ACQUIRE, 0x7FFF_F902);
                let _ = wv(inst_base + ramfc::PB_HEADER, 0x2040_0000);
                let _ = wv(inst_base + ramfc::SUBDEVICE, 0x3000_0000 | 0xFFF);
                let _ = wv(inst_base + ramfc::HCE_CTRL, 0x0000_0020);
                let _ = wv(inst_base + ramfc::CHID, channel_id);
                let _ = wv(inst_base + ramfc::CONFIG, 0x0000_1100);
                let _ = wv(inst_base + ramfc::CHANNEL_INFO, 0x1000_3080);

                // Verify instance block
                let v_sig = r(PM + inst_base + ramfc::SIGNATURE);
                let v_gpb = r(PM + inst_base + ramfc::GP_BASE_LO);
                let v_pdb = r(PM + inst_base + ramin::PAGE_DIR_BASE_LO);
                let v_usr = r(PM + inst_base + ramfc::USERD_LO);
                eprintln!("║   VRAM inst: SIG={v_sig:#010x} GP={v_gpb:#010x} PDB={v_pdb:#010x} USR={v_usr:#010x}");

                // ── GPFIFO entry at VRAM 0x9000 ──
                // Points to NOP push buffer at GPU VA 0xB000 (identity-mapped to VRAM 0xB000)
                let gp_entry: u64 = (0xB000_u64 & 0xFFFF_FFFC)
                    | ((1_u64) << (32 + 10)); // 1 dword
                let _ = wv(0x9000, gp_entry as u32);
                let _ = wv(0x9004, (gp_entry >> 32) as u32);

                // ── USERD page at VRAM 0xA000 ──
                let _ = wv(0xA000 + ramuserd::GP_PUT, 1); // GP_PUT = 1
                let _ = wv(0xA000 + ramuserd::GP_GET, 0); // GP_GET = 0

                // ── NOP push buffer at VRAM 0xB000 ──
                // Already zeroed (NOP method: count=0, method=0)

                // ── Runlist at VRAM 0xC000 ──
                // TSG group entry (16 bytes): TYPE=1, TIMESLICE_SCALE=3, TIMEOUT=128, LEN=1
                let _ = wv(0xC000, (128 << 24) | (3 << 16) | 1);
                let _ = wv(0xC004, 1); // TSG_LENGTH = 1 channel
                let _ = wv(0xC008, 0); // TSG_ID = 0
                let _ = wv(0xC00C, 0);
                // Channel entry: USERD_PTR[31:8]|USERD_TGT[7:6]|INST_TGT[5:4]|RQ[1]|TYPE=0
                let chan_dw0 = userd_vram as u32 & 0xFFFF_FF00;
                let _ = wv(0xC010, chan_dw0);
                let _ = wv(0xC014, 0); // USERD_PTR_HI
                let chan_dw2 = (0x8000_u32 & 0xFFFF_F000) | channel_id; // INST@0x8000, CH=0
                let _ = wv(0xC018, chan_dw2);
                let _ = wv(0xC01C, 0); // INST_PTR_HI

                // Reset PBDMA to sentinel
                let _ = w(pb + 0x40, 0xBEEF_0040);
                let _ = w(pb + 0x44, 0xBEEF_0044);
                let _ = w(pb + 0x48, 0);
                let _ = w(pb + 0x54, 0);
                let _ = w(pb + 0xD0, 0xBEEF_00D0);
                let _ = w(pb + 0xD4, 0xBEEF_00D4);
                let _ = w(pb + 0xC0, 0xBEEF_00C0);

                // Flush GPU L2 cache so engines see our PRAMIN writes
                let _ = w(0x70010, 0x0000_0001);
                for _ in 0..2000_u32 {
                    if r(0x70010) & 3 == 0 { break; }
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }

                // PCCSR: INST_PTR = 0x8000>>12 = 8, TARGET=0 (VID_MEM), BIND=TRUE
                let vram_inst = (0x8000_u32 >> 12) | pccsr::INST_BIND_TRUE;
                let _ = w(pccsr::inst(channel_id), vram_inst);
                std::thread::sleep(std::time::Duration::from_millis(10));

                let post_bind = r(pccsr::channel(channel_id));
                eprintln!("║   post-BIND(allVram): {post_bind:#010x}");

                // Clear any faults from INST_BIND
                if post_bind & 0x11000000 != 0 {
                    let _ = w(pccsr::channel(channel_id),
                        pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }

                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(5));

                // Submit runlist: base=VRAM 0xC000, target=VID_MEM(0), 2 entries
                let _ = w(pfifo::RUNLIST_BASE_LO + rl_stride, 0xC000_u32 >> 12);
                let _ = w(pfifo::RUNLIST_BASE_HI + rl_stride, 0); // VID_MEM aperture
                let _ = w(pfifo::RUNLIST_SUBMIT + rl_stride, 2 << 16);
                std::thread::sleep(std::time::Duration::from_millis(50));

                // Check scheduler status
                let post_rl = r(pccsr::channel(channel_id));
                let eng_rl_base = r(0x2288); // ENG_RUNLIST_BASE(1)
                let eng_rl = r(0x228C);      // ENG_RUNLIST(1)
                let sched_gpb = r(pb + 0x40);
                let sched_usr = r(pb + 0xD0);
                let sched_sig = r(pb + 0xC0);
                let sched_state = r(pb + 0xB0);
                eprintln!("║   post-submit: PCCSR={post_rl:#010x} ENG_RL_BASE={eng_rl_base:#010x} ENG_RL={eng_rl:#010x}");
                eprintln!("║   PBDMA: GP_BASE={sched_gpb:#010x} USERD={sched_usr:#010x} SIG={sched_sig:#010x} STATE={sched_state:#010x}");

                // Ring doorbell
                let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let post_db = r(pccsr::channel(channel_id));
                let gpb_post = r(pb + 0x40);
                let usr_post = r(pb + 0xD0);
                let sig_post = r(pb + 0xC0);
                let gp_put_post = r(pb + 0x54);
                let gp_fetch_post = r(pb + 0x48);
                eprintln!("║   post-doorbell: PCCSR={post_db:#010x} GP_BASE={gpb_post:#010x} USERD={usr_post:#010x} SIG={sig_post:#010x} GP_PUT={gp_put_post} GP_FETCH={gp_fetch_post}");
            }
            ExperimentOrdering::AllVramDirectPbdma => {
                // Same VRAM setup as K but with direct PBDMA programming
                const PM: usize = 0x0070_0000;
                const BW: usize = 0x0000_1700;
                let _ = w(BW, 0);
                std::thread::sleep(std::time::Duration::from_millis(1));

                let wv = |off: usize, val: u32| -> DriverResult<()> { w(PM + off, val) };

                // Zero VRAM region
                for off in (0..0xD000_usize).step_by(4) { let _ = wv(off, 0); }

                // Page tables (VRAM, VID_MEM aperture)
                let vram_pde = |addr: u64| -> u64 { (addr >> 4) | 1 };
                let vram_pte = |addr: u64| -> u64 { (addr >> 4) | 1 };

                let e = vram_pde(0x1000); let _ = wv(0x0000, e as u32); let _ = wv(0x0004, (e >> 32) as u32);
                let e = vram_pde(0x2000); let _ = wv(0x1000, e as u32); let _ = wv(0x1004, (e >> 32) as u32);
                let e = vram_pde(0x3000); let _ = wv(0x2000, e as u32); let _ = wv(0x2004, (e >> 32) as u32);
                let e = vram_pde(0x4000); let _ = wv(0x3008, e as u32); let _ = wv(0x300C, (e >> 32) as u32);
                for page in 0..13_usize {
                    let e = vram_pte((page as u64) * 4096);
                    let off = 0x4000 + page * 8;
                    let _ = wv(off, e as u32); let _ = wv(off + 4, (e >> 32) as u32);
                }

                // Instance block at VRAM 0x8000 (for PCCSR only — PBDMA gets direct writes)
                let ib = 0x8000_usize;
                let pdb_lo: u32 = (1 << 11) | (1 << 10) | (1 << 2) | 1;
                let _ = wv(ib + ramin::PAGE_DIR_BASE_LO, pdb_lo);
                let _ = wv(ib + ramin::PAGE_DIR_BASE_HI, 0);
                let _ = wv(ib + ramin::ENGINE_WFI_VEID, 0);
                let _ = wv(ib + ramin::SC_PDB_VALID, 1);
                let _ = wv(ib + ramin::SC0_PAGE_DIR_BASE_LO, pdb_lo);
                let _ = wv(ib + ramin::SC0_PAGE_DIR_BASE_HI, 0);
                let gpfifo_vram: u64 = 0x9000;
                let userd_vram: u64 = 0xA000;
                let _ = wv(ib + ramfc::GP_BASE_LO, gpfifo_vram as u32);
                let _ = wv(ib + ramfc::GP_BASE_HI, limit2 << 16);
                let _ = wv(ib + ramfc::USERD_LO, userd_vram as u32 & 0xFFFF_FE00);
                let _ = wv(ib + ramfc::USERD_HI, 0);
                let _ = wv(ib + ramfc::SIGNATURE, 0x0000_FACE);
                let _ = wv(ib + ramfc::ACQUIRE, 0x7FFF_F902);
                let _ = wv(ib + ramfc::PB_HEADER, 0x2040_0000);
                let _ = wv(ib + ramfc::SUBDEVICE, 0x3000_0FFF);
                let _ = wv(ib + ramfc::HCE_CTRL, 0x0000_0020);
                let _ = wv(ib + ramfc::CHID, channel_id);
                let _ = wv(ib + ramfc::CONFIG, 0x0000_1100);
                let _ = wv(ib + ramfc::CHANNEL_INFO, 0x1000_3080);

                // GPFIFO entry → NOP at GPU VA 0xB000
                let gp_entry: u64 = (0xB000_u64 & 0xFFFF_FFFC) | ((1_u64) << (32 + 10));
                let _ = wv(0x9000, gp_entry as u32);
                let _ = wv(0x9004, (gp_entry >> 32) as u32);

                // USERD: GP_PUT=1
                let _ = wv(0xA000 + ramuserd::GP_PUT, 1);
                let _ = wv(0xA000 + ramuserd::GP_GET, 0);

                // Runlist at VRAM 0xC000
                let _ = wv(0xC000, (128 << 24) | (3 << 16) | 1);
                let _ = wv(0xC004, 1);
                let _ = wv(0xC008, 0);
                let _ = wv(0xC00C, 0);
                let _ = wv(0xC010, (userd_vram as u32) & 0xFFFF_FF00);
                let _ = wv(0xC014, 0);
                let _ = wv(0xC018, (0x8000_u32 & 0xFFFF_F000) | channel_id);
                let _ = wv(0xC01C, 0);

                // L2 flush
                let _ = w(0x70010, 1);
                for _ in 0..2000_u32 {
                    if r(0x70010) & 3 == 0 { break; }
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }

                // PCCSR: set INST to VRAM 0x8000, NO BIND
                let _ = w(pccsr::inst(channel_id), 0x8000_u32 >> 12);
                std::thread::sleep(std::time::Duration::from_millis(5));

                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(5));

                // Directly program PBDMA with VRAM pointers
                let _ = w(pb + 0x40, gpfifo_vram as u32); // GP_BASE_LO
                let _ = w(pb + 0x44, limit2 << 16);
                let _ = w(pb + 0x48, 0); // GP_FETCH = 0
                let _ = w(pb + 0x4C, 0); // GP_STATE = 0
                let _ = w(pb + 0xD0, userd_vram as u32 & 0xFFFF_FE00); // USERD_LO (VID_MEM=0)
                let _ = w(pb + 0xD4, 0); // USERD_HI
                let _ = w(pb + 0xC0, 0x0000_FACE); // SIGNATURE
                let _ = w(pb + 0xAC, 0x1000_3080); // CHANNEL_INFO
                let _ = w(pb + 0xA8, 0x0000_1100); // CONFIG

                // Submit runlist
                let _ = w(pfifo::RUNLIST_BASE_LO + rl_stride, 0xC000_u32 >> 12);
                let _ = w(pfifo::RUNLIST_BASE_HI + rl_stride, 0); // VID_MEM
                let _ = w(pfifo::RUNLIST_SUBMIT + rl_stride, 2 << 16);
                std::thread::sleep(std::time::Duration::from_millis(20));

                // GP_PUT = 1 (both PBDMA register and doorbell)
                let _ = w(pb + 0x54, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let post = r(pccsr::channel(channel_id));
                let gp_put = r(pb + 0x54);
                let gp_fetch = r(pb + 0x48);
                let userd_lo = r(pb + 0xD0);
                let sig = r(pb + 0xC0);
                let state = r(pb + 0xB0);
                eprintln!("║   L result: PCCSR={post:#010x} GP_PUT={gp_put} GP_FETCH={gp_fetch} USERD={userd_lo:#010x} SIG={sig:#010x} STATE={state:#010x}");
            }
            ExperimentOrdering::PfifoResetInit => {
                // PFIFO engine reset via PMC bit 8 toggle — DESTRUCTIVE!
                // Skip on warm GPU to avoid killing PFIFO for other experiments.
                let pmc_cur = r(pmc::ENABLE);
                if !gpu_warm {
                    eprintln!("║   M: GPU cold, performing PMC reset...");
                    let _ = w(pmc::ENABLE, pmc_cur & !0x100);
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    let _ = w(pmc::ENABLE, pmc_cur | 0x100);
                    std::thread::sleep(std::time::Duration::from_millis(10));
                } else {
                    eprintln!("║   M: GPU warm, skipping PMC toggle to preserve state");
                }

                let pmc_post = r(pmc::ENABLE);
                let pfifo_post = r(pfifo::ENABLE);
                let pbdma_post = r(pfifo::PBDMA_MAP);
                let sched_post = r(0x2630);
                eprintln!("║   M state: PMC={pmc_post:#010x} PFIFO={pfifo_post:#010x} PBDMA_MAP={pbdma_post:#010x} SCHED_DIS={sched_post:#010x}");

                // Follow the D path: INST_BIND + enable + runlist
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pfifo::RUNLIST_BASE_LO + rl_stride, rl_base_lo);
                let _ = w(pfifo::RUNLIST_BASE_HI + rl_stride, rl_base_hi);
                let _ = w(pfifo::RUNLIST_SUBMIT + rl_stride, rl_submit);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let post = r(pccsr::channel(channel_id));
                let gp_fetch = r(pb + 0x48);
                let userd_rd = r(pb + 0xD0);
                eprintln!("║   M result: PCCSR={post:#010x} GP_FETCH={gp_fetch} USERD={userd_rd:#010x}");
            }
            ExperimentOrdering::FullDispatchWithInstBind => {
                // Step 1: Prepare GPFIFO entry BEFORE bind so DMA is ready
                let gp_entry: u64 = (RUNLIST_IOVA & 0xFFFF_FFFC)
                    | ((1_u64) << (32 + 10));
                gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
                write_u32_le(userd_page, ramuserd::GP_PUT, 1);
                write_u32_le(userd_page, ramuserd::GP_GET, 0);
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

                // Step 2: INST_BIND (D path — proven to achieve SCHEDULED on warm GPU)
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let post_bind = r(pccsr::channel(channel_id));
                eprintln!("║   N post-INST_BIND: {post_bind:#010x} (inst_val={pccsr_inst_val:#010x})");

                // Detailed fault diagnostics BEFORE clearing anything
                if post_bind & 0x11000000 != 0 {
                    let bind_err = r(0x252C);
                    let pfifo_intr = r(pfifo::INTR);
                    let mmu_fault_status = r(0x100E34);
                    let mmu_fault_addr_lo = r(0x100E38);
                    let mmu_fault_addr_hi = r(0x100E3C);
                    let mmu_fault_inst_lo = r(0x100E40);
                    let mmu_fault_inst_hi = r(0x100E44);
                    let _mmu_buf0_put = r(0x100E34 - 4);
                    eprintln!("║   N FAULT DIAG: BIND_ERR={bind_err:#010x} PFIFO_INTR={pfifo_intr:#010x}");
                    eprintln!("║   N FAULT DIAG: MMU_STATUS={mmu_fault_status:#010x} ADDR={mmu_fault_addr_hi:#010x}_{mmu_fault_addr_lo:#010x}");
                    eprintln!("║   N FAULT DIAG: MMU_INST={mmu_fault_inst_hi:#010x}_{mmu_fault_inst_lo:#010x}");
                    // Check all PBDMA interrupt registers
                    for pid in [1_usize, 2] {
                        let intr = r(pbdma::intr(pid));
                        let status = r(0x40000 + pid * 0x2000 + 0xB0);
                        let method = r(0x40000 + pid * 0x2000 + 0x1C0);
                        eprintln!("║   N PBDMA{pid} INTR={intr:#010x} STATE={status:#010x} METHOD={method:#010x}");
                    }
                    // Non-replayable fault buffer (may have fault entry)
                    let nrfb_get = r(0x100E4C);
                    let nrfb_put = r(0x100E50);
                    eprintln!("║   N NR_FAULT_BUF: GET={nrfb_get:#010x} PUT={nrfb_put:#010x}");
                }

                // Step 3: Enable channel
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(2));

                // Step 4: Submit runlist
                let _ = w(pfifo::RUNLIST_BASE_LO + rl_stride, rl_base_lo);
                let _ = w(pfifo::RUNLIST_BASE_HI + rl_stride, rl_base_hi);
                let _ = w(pfifo::RUNLIST_SUBMIT + rl_stride, rl_submit);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let post_rl = r(pccsr::channel(channel_id));
                let scheduled = (post_rl & 2) != 0;
                eprintln!("║   N post-runlist: {post_rl:#010x} scheduled={scheduled}");

                // Step 5: Check PBDMA state — did the scheduler load our context?
                let pbdma_userd = r(pb + 0xD0);
                let pbdma_gpbase = r(pb + 0x40);
                let pbdma_sig = r(pb + 0xC0);
                let pbdma_gp_put = r(pb + 0x54);
                let pbdma_gp_fetch = r(pb + 0x48);
                let pbdma_state = r(pb + 0xB0);
                eprintln!("║   N pre-doorbell PBDMA: USERD={pbdma_userd:#010x} GP_BASE={pbdma_gpbase:#010x} SIG={pbdma_sig:#010x} GP_PUT={pbdma_gp_put} GP_FETCH={pbdma_gp_fetch} STATE={pbdma_state:#010x}");

                // Step 6: Ring doorbell
                let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Step 7: Check PBDMA again — did doorbell trigger context load + fetch?
                let post_db = r(pccsr::channel(channel_id));
                let db_userd = r(pb + 0xD0);
                let db_gpbase = r(pb + 0x40);
                let db_sig = r(pb + 0xC0);
                let db_gp_put = r(pb + 0x54);
                let db_gp_fetch = r(pb + 0x48);
                let db_state = r(pb + 0xB0);
                let db_gp_state = r(pb + 0x4C);
                eprintln!("║   N post-doorbell: PCCSR={post_db:#010x} USERD={db_userd:#010x} GP_BASE={db_gpbase:#010x} SIG={db_sig:#010x}");
                eprintln!("║   N post-doorbell: GP_PUT={db_gp_put} GP_FETCH={db_gp_fetch} STATE={db_state:#010x} GP_STATE={db_gp_state:#010x}");

                // Check ALL PBDMAs on this runlist (scheduler may assign to PBDMA 2!)
                {
                    let mut seq = 0_usize;
                    for pid in 0..32_usize {
                        if pbdma_map & (1 << pid) == 0 { continue; }
                        let rl = r(0x2390 + seq * 4);
                        seq += 1;
                        if rl != target_runlist { continue; }
                        let pbx = 0x40000 + pid * 0x2000;
                        let userd = r(pbx + 0xD0);
                        let gpbase = r(pbx + 0x40);
                        let sig = r(pbx + 0xC0);
                        let gp_put = r(pbx + 0x54);
                        let gp_fetch = r(pbx + 0x48);
                        let state = r(pbx + 0xB0);
                        eprintln!("║   N PBDMA{pid}: USERD={userd:#010x} GP_BASE={gpbase:#010x} SIG={sig:#010x} GP_PUT={gp_put} GP_FETCH={gp_fetch} STATE={state:#010x}");
                    }
                }

                // Step 8: If still not fetching, try a longer wait + second doorbell
                if db_gp_fetch == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    let final_pccsr = r(pccsr::channel(channel_id));
                    let final_intr = r(pfifo::INTR);
                    // Check all PBDMAs again after retry
                    let mut any_fetch = false;
                    let mut seqr = 0_usize;
                    for pid in 0..32_usize {
                        if pbdma_map & (1 << pid) == 0 { continue; }
                        let rl = r(0x2390 + seqr * 4);
                        seqr += 1;
                        if rl != target_runlist { continue; }
                        let pbx = 0x40000 + pid * 0x2000;
                        let gp_fetch = r(pbx + 0x48);
                        let userd = r(pbx + 0xD0);
                        let state = r(pbx + 0xB0);
                        if gp_fetch != 0 { any_fetch = true; }
                        eprintln!("║   N retry PBDMA{pid}: GP_FETCH={gp_fetch} USERD={userd:#010x} STATE={state:#010x}");
                    }
                    eprintln!("║   N retry: PCCSR={final_pccsr:#010x} PFIFO_INTR={final_intr:#010x} any_fetch={any_fetch}");
                }
            }
            ExperimentOrdering::FullDispatchWithPreempt => {
                // Same as N but with PREEMPT to force context load

                // Prepare GPFIFO + USERD
                let gp_entry: u64 = (RUNLIST_IOVA & 0xFFFF_FFFC)
                    | ((1_u64) << (32 + 10));
                gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
                write_u32_le(userd_page, ramuserd::GP_PUT, 1);
                write_u32_le(userd_page, ramuserd::GP_GET, 0);
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

                // INST_BIND + enable + runlist (D path)
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pfifo::RUNLIST_BASE_LO + rl_stride, rl_base_lo);
                let _ = w(pfifo::RUNLIST_BASE_HI + rl_stride, rl_base_hi);
                let _ = w(pfifo::RUNLIST_SUBMIT + rl_stride, rl_submit);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let post_rl = r(pccsr::channel(channel_id));
                eprintln!("║   O post-runlist: {post_rl:#010x} sched={}", post_rl & 2 != 0);

                // PREEMPT: force context switch to our channel
                // Nouveau reference shows PREEMPT=0x01000001 at 0x2634
                // Format: bit[24]=PENDING, bits[11:0]=channel_id (type 1 = channel preempt)
                // Also try runlist preempt: bit[20]=1, bits[15:12]=runlist_id
                let preempt_ch = (1_u32 << 24) | channel_id;
                let _ = w(0x2634, preempt_ch);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let post_preempt = r(pccsr::channel(channel_id));
                let preempt_rb = r(0x2634);
                eprintln!("║   O post-preempt(ch): PCCSR={post_preempt:#010x} PREEMPT={preempt_rb:#010x}");

                // Also try runlist preempt
                let preempt_rl = (1_u32 << 20) | (target_runlist << 16);
                let _ = w(0x2634, preempt_rl);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let post_rl_preempt = r(pccsr::channel(channel_id));
                eprintln!("║   O post-preempt(rl): PCCSR={post_rl_preempt:#010x} PREEMPT={:#010x}", r(0x2634));

                // Ring doorbell
                let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                std::thread::sleep(std::time::Duration::from_millis(100));

                let post_db = r(pccsr::channel(channel_id));
                let gp_fetch = r(pb + 0x48);
                let gp_put = r(pb + 0x54);
                let userd_lo = r(pb + 0xD0);
                let sig = r(pb + 0xC0);
                let state = r(pb + 0xB0);
                let gpbase = r(pb + 0x40);
                eprintln!("║   O final: PCCSR={post_db:#010x} GP_PUT={gp_put} GP_FETCH={gp_fetch} USERD={userd_lo:#010x} GP_BASE={gpbase:#010x} SIG={sig:#010x} STATE={state:#010x}");
            }
            ExperimentOrdering::ScheduledPlusDirectPbdma => {
                // Phase 0: Enable PFIFO + PBDMA interrupts (nouveau values)
                let _ = w(pfifo::INTR_EN, 0x6181_0101);
                let _ = w(pbdma::intr_en(1), 0xFFFF_FFFF);
                let _ = w(pbdma::intr_en(2), 0xFFFF_FFFF);

                // Phase 1: Prepare GPFIFO + USERD in host memory BEFORE bind
                let gp_entry: u64 = (RUNLIST_IOVA & 0xFFFF_FFFC)
                    | ((1_u64) << (32 + 10));
                gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
                write_u32_le(userd_page, ramuserd::GP_PUT, 1);
                write_u32_le(userd_page, ramuserd::GP_GET, 0);
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

                // Phase 2: INST_BIND + enable + runlist (D path, NO direct PBDMA writes)
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let post_bind = r(pccsr::channel(channel_id));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pfifo::RUNLIST_BASE_LO + rl_stride, rl_base_lo);
                let _ = w(pfifo::RUNLIST_BASE_HI + rl_stride, rl_base_hi);
                let _ = w(pfifo::RUNLIST_SUBMIT + rl_stride, rl_submit);
                std::thread::sleep(std::time::Duration::from_millis(100));

                let post_rl = r(pccsr::channel(channel_id));
                eprintln!("║   P phase2: post_bind={post_bind:#010x} post_rl={post_rl:#010x} sched={}", post_rl & 2 != 0);

                // Check PBDMA state — did the scheduler load context from RAMFC?
                for pid in [1_usize, 2] {
                    let pbx = 0x40000 + pid * 0x2000;
                    let userd = r(pbx + 0xD0);
                    let gpbase = r(pbx + 0x40);
                    let sig = r(pbx + 0xC0);
                    let gp_put = r(pbx + 0x50);
                    let gp_fetch = r(pbx + 0x48);
                    let state = r(pbx + 0xB0);
                    let intr = r(pbdma::intr(pid));
                    eprintln!("║   P pre-db PBDMA{pid}: USERD={userd:#010x} GP_BASE={gpbase:#010x} SIG={sig:#010x} GP_PUT={gp_put} GP_FETCH={gp_fetch} STATE={state:#010x} INTR={intr:#010x}");
                }

                // Phase 4: Ring doorbell
                let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                std::thread::sleep(std::time::Duration::from_millis(100));

                let post_db = r(pccsr::channel(channel_id));
                let pfifo_intr = r(pfifo::INTR);
                eprintln!("║   P post-doorbell: PCCSR={post_db:#010x} PFIFO_INTR={pfifo_intr:#010x}");

                // Read detailed fault info
                let mmu_fault_status = r(0x100E34);
                let mmu_fault_lo = r(0x100E38);
                let mmu_fault_hi = r(0x100E3C);
                let mmu_fault_inst_lo = r(0x100E40);
                let bind_err = r(0x252C);
                eprintln!("║   P FAULT: MMU_STATUS={mmu_fault_status:#010x} ADDR={mmu_fault_hi:#010x}_{mmu_fault_lo:#010x} INST={mmu_fault_inst_lo:#010x} BIND_ERR={bind_err:#010x}");

                // Read PBDMA interrupt and fault details using correct register offsets
                for pid in [1_usize, 2] {
                    let pbx = 0x40000 + pid * 0x2000;
                    let intr = r(pbdma::intr(pid));
                    let hce_intr = r(pbdma::hce_intr(pid));
                    let userd = r(pbx + 0xD0);
                    let gpbase = r(pbx + 0x40);
                    let gp_put = r(pbx + 0x50);
                    let gp_fetch = r(pbx + 0x48);
                    let state = r(pbx + 0xB0);
                    let gp_state = r(pbx + 0x4C);
                    eprintln!("║   P PBDMA{pid}: INTR={intr:#010x} HCE_INTR={hce_intr:#010x}");
                    eprintln!("║   P PBDMA{pid}: USERD={userd:#010x} GP_BASE={gpbase:#010x} GP_PUT={gp_put} GP_FETCH={gp_fetch} STATE={state:#010x} GP_STATE={gp_state:#010x}");
                }

                // Check non-replayable fault buffer
                let nrfb_get = r(0x100E4C);
                let nrfb_put = r(0x100E50);
                let rfb_get = r(0x100E30);
                let rfb_put = r(0x100E34);
                eprintln!("║   P FAULTBUF: NR_GET={nrfb_get:#010x} NR_PUT={nrfb_put:#010x} R_GET={rfb_get:#010x} R_PUT={rfb_put:#010x}");

                eprintln!("║   P final: PCCSR={post_db:#010x}");

                // Phase 5: Clear faults and retry doorbell
                let _ = w(pccsr::channel(channel_id),
                    pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET);
                let _ = w(pfifo::INTR, 0xFFFF_FFFF);
                let _ = w(pbdma::intr(1), 0xFFFF_FFFF);
                let _ = w(pbdma::intr(2), 0xFFFF_FFFF);
                std::thread::sleep(std::time::Duration::from_millis(50));
                let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                std::thread::sleep(std::time::Duration::from_millis(300));
                let final_pccsr = r(pccsr::channel(channel_id));
                let final_fetch1 = r(pb + 0x48);
                let final_fetch2 = r(0x44000 + 0x48);
                let final_pfifo_intr = r(pfifo::INTR);
                eprintln!("║   P retry: PCCSR={final_pccsr:#010x} PBDMA1_FETCH={final_fetch1} PBDMA2_FETCH={final_fetch2} PFIFO_INTR={final_pfifo_intr:#010x}");
            }
            ExperimentOrdering::VramFullDispatch => {
                const PM: usize = 0x0070_0000; // PRAMIN base in BAR0
                const BW: usize = 0x0000_1700; // BAR0_WINDOW register
                const VI: usize = 0x3000;       // VRAM offset for instance block

                // Set BAR0 window → page 0 so PRAMIN maps VRAM[0..64K]
                let _ = w(BW, 0);
                std::thread::sleep(std::time::Duration::from_millis(1));

                // Copy instance block (already populated) to VRAM via PRAMIN
                let inst_bytes = instance.as_slice();
                for off in (0..inst_bytes.len()).step_by(4) {
                    let val = u32::from_le_bytes([
                        inst_bytes[off], inst_bytes[off+1],
                        inst_bytes[off+2], inst_bytes[off+3],
                    ]);
                    if val != 0 {
                        let _ = w(PM + VI + off, val);
                    }
                }

                // Keep SIGNATURE = 0xFACE (PBDMA validates this; 0xDEAD causes
                // SIGNATURE error at PBDMA INTR bit 31).
                // The ectx binding below is the real fix for CTXNOTVALID.

                // GR engine context binding (gv100_ectx_bind):
                // The PBDMA fires CTXNOTVALID (INTR=0x80000000) without this.
                // Set inst[0x0AC] bit 16 = engine context valid.
                // Set inst[0x210/0x214] = engine context GPU VA (bit 2 = valid).
                // Use PT0_IOVA as a placeholder — the PBDMA should at least
                // start fetching GPFIFO before the GR engine tries to load context.
                let _ = w(PM + VI + 0x0AC, 0x0001_0000);
                let _ = w(PM + VI + 0x210, (PT0_IOVA as u32) | 4);
                let _ = w(PM + VI + 0x214, (PT0_IOVA >> 32) as u32);

                // Verify key RAMFC fields in VRAM
                let v_sig = r(PM + VI + ramfc::SIGNATURE);
                let v_gpb = r(PM + VI + ramfc::GP_BASE_LO);
                let v_usr = r(PM + VI + ramfc::USERD_LO);
                let v_pdb = r(PM + VI + ramin::PAGE_DIR_BASE_LO);
                eprintln!("║   Q VRAM inst: SIG={v_sig:#010x} GP_BASE={v_gpb:#010x} USERD={v_usr:#010x} PDB={v_pdb:#010x}");

                // USERD in VRAM: write GP_PUT=1, GP_GET=0 at VRAM offset 0x0000
                // The PBDMA may not be able to read USERD from system memory
                // (IOMMU access issue), so put it in VRAM alongside the instance block.
                const VRAM_USERD: usize = 0x0000;
                let _ = w(PM + VRAM_USERD + ramuserd::GP_PUT, 1);
                let _ = w(PM + VRAM_USERD + ramuserd::GP_GET, 0);
                // Verify USERD in VRAM
                let vram_gp_put = r(PM + VRAM_USERD + ramuserd::GP_PUT);
                eprintln!("║   Q VRAM USERD: GP_PUT={vram_gp_put} (at 0x{:04x})", VRAM_USERD + ramuserd::GP_PUT);

                // Override RAMFC USERD to point to VRAM (target=0 for VID_MEM)
                // PBDMA_TARGET encoding: 0=VID_MEM, 1=SYS_MEM_COH, 2=SYS_MEM_NCOH
                let _ = w(PM + VI + ramfc::USERD_LO,
                    (VRAM_USERD as u32 & 0xFFFF_FE00) | 0); // target=0 VID_MEM
                let _ = w(PM + VI + ramfc::USERD_HI, 0);

                // Also prepare GPFIFO entry in host memory (still at system IOVA)
                let gp_entry: u64 = (RUNLIST_IOVA & 0xFFFF_FFFC)
                    | ((1_u64) << (32 + 10));
                gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
                // Keep host USERD updated too (for completeness)
                write_u32_le(userd_page, ramuserd::GP_PUT, 1);
                write_u32_le(userd_page, ramuserd::GP_GET, 0);
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

                // GV100 runlist preempt BEFORE we submit — force stale context off PBDMA
                let _ = w(0x002638, 1 << target_runlist);
                std::thread::sleep(std::time::Duration::from_millis(50));
                let preempt_pending = r(0x002284 + (target_runlist as usize) * 8);
                eprintln!("║   Q pre-preempt: pending={preempt_pending:#010x}");

                // INST_BIND with VRAM target + enable + runlist (D path)
                eprintln!("║   Q pccsr_inst_val={pccsr_inst_val:#010x} rl_lo={rl_base_lo:#010x} rl_hi={rl_base_hi:#010x} rl_submit={rl_submit:#010x}");
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let post_bind = r(pccsr::channel(channel_id));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pfifo::RUNLIST_BASE_LO + rl_stride, rl_base_lo);
                let _ = w(pfifo::RUNLIST_BASE_HI + rl_stride, rl_base_hi);
                let _ = w(pfifo::RUNLIST_SUBMIT + rl_stride, rl_submit);
                std::thread::sleep(std::time::Duration::from_millis(50));

                // Check if runlist update was processed (pending bit should clear)
                let rl_pending = r(0x002284 + (target_runlist as usize) * 8);
                eprintln!("║   Q runlist_pending={rl_pending:#010x} (bit20={})",
                    rl_pending & 0x00100000 != 0);

                std::thread::sleep(std::time::Duration::from_millis(100));
                let post_rl = r(pccsr::channel(channel_id));
                eprintln!("║   Q bind={post_bind:#010x} sched={post_rl:#010x} ok={}",
                    post_rl & 2 != 0);

                // Interrupt-driven runlist acknowledgment + PBDMA dispatch
                let pb2: usize = 0x44000; // PBDMA2 base

                // Phase A: Acknowledge runlist interrupt
                let pfifo_intr = r(pfifo::INTR);
                let rl_mask = r(0x002A00);
                eprintln!("║   Q ack: PFIFO_INTR={pfifo_intr:#010x} RL_MASK={rl_mask:#010x}");
                if rl_mask != 0 {
                    let _ = w(0x002A00, rl_mask);
                }
                // Clear all PFIFO + PBDMA interrupts
                let _ = w(pfifo::INTR, 0xFFFF_FFFF);
                let _ = w(pbdma::intr(1), 0xFFFF_FFFF);
                let _ = w(pbdma::intr(2), 0xFFFF_FFFF);
                let _ = w(pccsr::channel(channel_id),
                    pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET);
                std::thread::sleep(std::time::Duration::from_millis(20));

                // Phase B: Check PBDMA2 context load (RAMFC offsets)
                let p2_sig = r(pb2 + 0x010);
                let p2_userd = r(pb2 + 0x008);
                let p2_gpbase = r(pb2 + 0x048);
                let p2_chid = r(pb2 + 0x0E8);
                eprintln!("║   Q ctx: SIG={p2_sig:#010x} USERD={p2_userd:#010x} GP_BASE={p2_gpbase:#010x} CHID={p2_chid:#010x}");

                // Phase C: Ring doorbell and check for PBDMA activity
                let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Scan PBDMA2 for ALL changes
                eprint!("║   Q pb2-post-db:");
                for off in (0x000..=0x0FF_usize).step_by(4) {
                    let val = r(pb2 + off);
                    if val != 0 {
                        eprint!(" [{off:#05x}]={val:#010x}");
                    }
                }
                eprintln!();

                let pccsr_post = r(pccsr::channel(channel_id));
                let intr_post = r(pfifo::INTR);
                let p2_intr = r(pbdma::intr(2));
                let p2_hce = r(pbdma::hce_intr(2));
                eprintln!("║   Q post-db: PCCSR={pccsr_post:#010x} PFIFO_INTR={intr_post:#010x} PBDMA2_INTR={p2_intr:#010x} HCE={p2_hce:#010x}");

                // Phase D: If PBDMA2 has CTXNOTVALID or SIGNATURE error, handle it
                if p2_intr != 0 || p2_hce != 0 {
                    let _ = w(pbdma::intr(2), 0xFFFF_FFFF);
                    let _ = w(pbdma::hce_intr(2), 0xFFFF_FFFF);
                    let _ = w(pfifo::INTR, 0xFFFF_FFFF);
                    let _ = w(pccsr::channel(channel_id),
                        pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET);
                    std::thread::sleep(std::time::Duration::from_millis(20));

                    // Second doorbell after clearing interrupts
                    let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                    std::thread::sleep(std::time::Duration::from_millis(200));

                    eprint!("║   Q pb2-retry:");
                    for off in (0x000..=0x0FF_usize).step_by(4) {
                        let val = r(pb2 + off);
                        if val != 0 {
                            eprint!(" [{off:#05x}]={val:#010x}");
                        }
                    }
                    eprintln!();

                    let retry_intr = r(pbdma::intr(2));
                    let retry_hce = r(pbdma::hce_intr(2));
                    let retry_pccsr = r(pccsr::channel(channel_id));
                    eprintln!("║   Q retry: PCCSR={retry_pccsr:#010x} PBDMA2_INTR={retry_intr:#010x} HCE={retry_hce:#010x}");
                }

                let final_pccsr = r(pccsr::channel(channel_id));
                let final_intr = r(pfifo::INTR);
                eprintln!("║   Q final: PCCSR={final_pccsr:#010x} PFIFO_INTR={final_intr:#010x}");
            }
        }

        // Wait for hardware to process
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Capture snapshot
        let pccsr_chan = r(pccsr::channel(channel_id));
        let pccsr_inst_rb = r(pccsr::inst(channel_id));
        let cur_userd_lo = r(pb + 0xD0);
        let cur_userd_hi = r(pb + 0xD4);
        let cur_ramfc_userd_lo = r(pb + 0x08);
        let cur_ramfc_userd_hi = r(pb + 0x0C);
        let cur_gp_base_lo = r(pb + 0x40);

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
            faulted: (pccsr_chan >> 24) & 1 != 0 || (pccsr_chan >> 28) & 1 != 0,
            scheduled: (pccsr_chan & 2) != 0,
            pbdma_ours: cur_userd_lo != residual_userd_lo
                || cur_ramfc_userd_lo != residual_ramfc_userd_lo
                || cur_gp_base_lo != residual_gp_base_lo,
        };

        eprintln!("║ {}", result.summary_line());

        // Tear down
        let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let _ = w(pccsr::channel(channel_id),
            pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let _ = w(pccsr::inst(channel_id), 0);
        std::thread::sleep(std::time::Duration::from_millis(5));

        // Reset GPFIFO/USERD DMA buffers for next experiment
        gpfifo_ring.iter_mut().take(16).for_each(|b| *b = 0);
        write_u32_le(userd_page, ramuserd::GP_PUT, 0);
        write_u32_le(userd_page, ramuserd::GP_GET, 0);

        runlist.as_mut_slice().fill(0);
        let _ = w(pfifo::RUNLIST_BASE_LO + rl_stride, (RUNLIST_IOVA >> 12) as u32);
        let _ = w(pfifo::RUNLIST_BASE_HI + rl_stride, rl_base_hi);
        let _ = w(pfifo::RUNLIST_SUBMIT + rl_stride, 0); // count=0 → empty runlist
        std::thread::sleep(std::time::Duration::from_millis(10));

        results.push(result);
    }

    eprintln!("╚═══════════════════════════════════════════════════════════╝");
    Ok(results)
}
