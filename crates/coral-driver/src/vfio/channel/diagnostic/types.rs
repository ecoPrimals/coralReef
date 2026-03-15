// SPDX-License-Identifier: AGPL-3.0-only

use super::super::registers::*;

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
    /// W: Runlist ACK protocol — submit runlist, poll PFIFO_INTR for bit 30
    /// (runlist completion), read RUNLIST_ACK (0x2A00), write BIT(runl_id) to
    /// acknowledge, clear PFIFO_INTR. Without this handshake, the GV100
    /// scheduler never dispatches channels. NO INST_BIND.
    RunlistAckProtocol,
    /// X: INST_BIND + Runlist ACK — full nouveau-style init:
    /// INST_BIND → enable → runlist submit → poll PFIFO_INTR bit 30 → ACK →
    /// doorbell. This is the closest to nouveau's actual sequence.
    InstBindWithRunlistAck,
    /// Y: GV100 preempt + INST_BIND + ACK — evict stale contexts first:
    /// preempt(0x2638) → INST_BIND → enable → runlist → ACK → doorbell.
    PreemptInstBindAck,
    /// Z: Full PFIFO nuke-and-pave — complete reinit from scratch:
    /// PMC toggle bit 8 → re-init all PBDMAs → set INTR_EN → flush empty
    /// runlists with ACK → setup channel → submit real runlist → ACK → doorbell.
    /// Replicates the full nouveau init sequence in a single experiment.
    FullPfifoReinitDispatch,
    /// Z2: Z + direct PBDMA writes — full reinit followed by manual PBDMA
    /// register programming before doorbell. Tests if the scheduler processes
    /// runlists after a clean reinit, and if direct PBDMA writes help dispatch.
    FullPfifoReinitDirectPbdma,
    /// Z3: No PMC reset — rely on pfifo_init state. Just refresh INTR_EN,
    /// setup channel (no INST_BIND), submit runlist with microsecond-level
    /// polling. Tests if the scheduler responds faster than our previous polls.
    NoPmcResetFastPoll,
    /// U: Clean scheduler dispatch with GP_PUT=0 — NO GPFIFO work.
    /// RAMFC context has GP_PUT=0, GP_GET=0. Scheduler loads channel, PBDMA
    /// sees empty ring. Tests if scheduling works without faulting when idle.
    CleanSchedNoWork,
    /// U2: Scheduler dispatch with valid NOP GPFIFO entry.
    /// Same as R but GPFIFO slot 0 points to a NOP push buffer (method 0x0100
    /// = NOP, subchannel 0). Tests if PBDMA executes a real command after
    /// scheduling + RAMFC load.
    SchedWithNopPushbuf,

    // ── Metal capability discovery experiments ────────────────────────

    /// Cycle through D0/D3hot/D3cold, snapshot registers before/after.
    /// Discovers what state persists across power transitions.
    PowerStateSweep,
    /// Systematic BAR0 scan with register classification (RO/RW/WO/Trigger/Dead).
    /// Discovers the register space topology of an unknown GPU.
    RegisterCartography,
    /// Test all CPU→GPU, GPU→VRAM, GPU→GPU memory paths.
    /// Validates every aperture and DMA path is functional.
    MemoryPathMatrix,
    /// Enable/disable each PMC bit individually, measure which engines
    /// respond and which registers change. Maps the clock domain topology.
    ClockDomainSweep,
    /// Enumerate engines via PTOP table, test firmware status, probe
    /// scheduler and FALCON microcontrollers.
    EngineProbe,

    // ── HBM2 training experiments ────────────────────────────────────

    /// Read all FBPA registers across 4 stacks, classify as trained/untrained/dead.
    Hbm2PhyProbe,
    /// Capture timing register values from a trained card for replay.
    Hbm2TimingCapture,
    /// Execute the typestate training sequence, record per-phase snapshots.
    Hbm2TrainingAttempt,
    /// Binary search for the minimal register write set that trains HBM2.
    Hbm2MinimalSet,
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
    /// Whether PBDMA_FAULTED (bit 22) or ENG_FAULTED (bit 23) is set.
    pub faulted: bool,
    /// Whether PCCSR bit 1 (NEXT/scheduled) is set.
    pub scheduled: bool,
    /// PCCSR STATUS[27:24] — 0=IDLE, 5=ON_PBDMA, 6=ON_PBDMA+ENG, 7=ON_ENG.
    pub status: u32,
    /// PBDMA interrupt register for the target PBDMA.
    pub pbdma_intr: u32,
    /// Alt PBDMA GP_PUT (if alt PBDMA exists for same runlist).
    pub alt_gp_put: u32,
    /// Alt PBDMA GP_FETCH.
    pub alt_gp_fetch: u32,
    /// Alt PBDMA GP_STATE.
    pub alt_gp_state: u32,
    /// Alt PBDMA CTX USERD_LO.
    pub alt_ctx_userd: u32,
    /// Whether PBDMA registers changed from residual state (i.e. our writes stuck).
    pub pbdma_ours: bool,
    /// PFIFO CHSW_ERROR (0x256C) — channel switch error detail.
    /// 0=NO_ERROR, 1=REQ_TIMEOUT, 2=ACK_TIMEOUT, 3=ACK_EXTRA, 4=RDAT_TIMEOUT, 5=RDAT_EXTRA.
    pub chsw_error: u32,
    /// Host USERD page GP_GET (offset 0x88) — written by GPU when GPFIFO entry is consumed.
    /// If this advances from 0, the PBDMA successfully wrote back to host memory.
    pub userd_gp_get: u32,
    /// Host USERD page GP_PUT (offset 0x8C) — written by host, read by GPU.
    pub userd_gp_put: u32,
}

impl ExperimentResult {
    /// Single-line summary for the experiment table.
    pub fn summary_line(&self) -> String {
        let pbdma_tag = if self.pbdma_ours { "OUR" } else { "old" };
        let fault_tag = if self.faulted { "FAULT" } else { "ok" };
        let sched_tag = if self.scheduled { "SCHED" } else { "no" };
        let status_name = pccsr::status_name(self.pccsr_chan);
        let chsw_tag = match self.chsw_error {
            0 => "",
            1 => " CHSW:REQ_TMO",
            2 => " CHSW:ACK_TMO",
            3 => " CHSW:ACK_XTR",
            4 => " CHSW:RDAT_TMO",
            5 => " CHSW:RDAT_XTR",
            _ => " CHSW:UNK",
        };
        let gp_get_tag = if self.userd_gp_get > 0 { "GET!" } else { "" };
        format!(
            "{:<42} | {:08x} | {:<5} | {:<5} | {:<14} | D0={:08x} R8={:08x} | {:>3} | gp={:02x}/{:02x} GET={} | {:08x}{} {}",
            self.name,
            self.pccsr_chan,
            fault_tag,
            sched_tag,
            status_name,
            self.pbdma_userd_lo,
            self.pbdma_ramfc_userd_lo,
            pbdma_tag,
            self.pbdma_gp_put,
            self.pbdma_gp_fetch,
            self.userd_gp_get,
            self.engn0_status,
            chsw_tag,
            gp_get_tag,
        )
    }

    /// Human-readable name for CHSW_ERROR code.
    pub fn chsw_error_name(&self) -> &'static str {
        match self.chsw_error {
            0 => "NO_ERROR",
            1 => "REQ_TIMEOUT",
            2 => "ACK_TIMEOUT",
            3 => "ACK_EXTRA",
            4 => "RDAT_TIMEOUT",
            5 => "RDAT_EXTRA",
            _ => "UNKNOWN",
        }
    }
}
