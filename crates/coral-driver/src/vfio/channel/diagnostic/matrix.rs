// SPDX-License-Identifier: AGPL-3.0-only

use super::types::{ExperimentConfig, ExperimentOrdering};

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
            runlist_base_target: 2, // GV100 aperture: 1=SYS_MEM_COH (best guess)
            ordering,
            skip_pfifo_toggle: true,
            requires_sm: None,
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
            requires_sm: None,
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
            requires_sm: None,
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
        requires_sm: None,
    });
    configs.push(ExperimentConfig {
        name: "O_dispatch_preempt_ncoh",
        pccsr_target: 3,
        runlist_userd_target: 2,
        runlist_inst_target: 3,
        runlist_base_target: 2,
        ordering: ExperimentOrdering::FullDispatchWithPreempt,
        skip_pfifo_toggle: true,
        requires_sm: None,
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
        requires_sm: None,
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
            requires_sm: None,
        });
        configs.push(ExperimentConfig {
            name: Box::leak(format!("F_direct_{pccsr_name}_instBind").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 3,
            ordering: ExperimentOrdering::DirectPbdmaWithInstBind,
            skip_pfifo_toggle: true,
            requires_sm: None,
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
            requires_sm: None,
        });
        configs.push(ExperimentConfig {
            name: Box::leak(format!("H_activate_doorbell_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 3,
            ordering: ExperimentOrdering::DirectPbdmaActivateDoorbell,
            skip_pfifo_toggle: true,
            requires_sm: None,
        });
        configs.push(ExperimentConfig {
            name: Box::leak(format!("I_activate_sched_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 3,
            ordering: ExperimentOrdering::DirectPbdmaActivateScheduled,
            skip_pfifo_toggle: true,
            requires_sm: None,
        });
    }

    // ── T: Direct PBDMA + SCHED + doorbell (I + doorbell AFTER) ────────
    for &(pccsr_t, pccsr_name) in &[(2_u32, "coh"), (3_u32, "ncoh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("T_sched_doorbell_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 2,
            ordering: ExperimentOrdering::DirectPbdmaSchedDoorbell,
            skip_pfifo_toggle: true,
            requires_sm: None,
        });
    }

    // ── R: RAMFC-mirror PBDMA registers + SCHED + doorbell ───────────
    for &(pccsr_t, pccsr_name) in &[(2_u32, "coh"), (3_u32, "ncoh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("R_ramfc_sched_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 2,
            ordering: ExperimentOrdering::RamfcMirrorSchedDoorbell,
            skip_pfifo_toggle: true,
            requires_sm: None,
        });
    }

    // ── S: Both register paths + SCHED + doorbell ────────────────────
    for &(pccsr_t, pccsr_name) in &[(2_u32, "coh"), (3_u32, "ncoh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("S_both_sched_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 2,
            ordering: ExperimentOrdering::BothPathsSchedDoorbell,
            skip_pfifo_toggle: true,
            requires_sm: None,
        });
    }

    // ── U: Clean scheduling — GP_PUT=0, no GPFIFO work ────────────────
    // Control experiment: if scheduling works without faults when idle,
    // it proves the PFIFO/PBDMA pipeline is healthy and faults (if any)
    // come from GPFIFO content, not channel setup.
    for &(pccsr_t, pccsr_name) in &[(2_u32, "coh"), (3_u32, "ncoh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("U_cleanSched_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 2,
            ordering: ExperimentOrdering::CleanSchedNoWork,
            skip_pfifo_toggle: true,
            requires_sm: None,
        });
    }

    // ── U2: Scheduling with valid NOP push buffer ────────────────────
    // If U (idle) is clean, U2 tests whether PBDMA can execute a real
    // GPU method (NOP = subchan 0, method 0x0100, data 0) after scheduling.
    for &(pccsr_t, pccsr_name) in &[(2_u32, "coh"), (3_u32, "ncoh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("U2_nopPushbuf_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 2,
            ordering: ExperimentOrdering::SchedWithNopPushbuf,
            skip_pfifo_toggle: true,
            requires_sm: None,
        });
    }

    // ── V: Pure scheduler path (SCHED_EN + runlist, no direct PBDMA) ─
    for &(pccsr_t, pccsr_name) in &[(2_u32, "coh"), (3_u32, "ncoh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("V_scheduler_only_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 2,
            ordering: ExperimentOrdering::SchedulerPathOnly,
            skip_pfifo_toggle: true,
            requires_sm: None,
        });
    }

    // ── W: Runlist ACK protocol (missing handshake discovery) ──────────
    for &(pccsr_t, pccsr_name) in &[(2_u32, "coh"), (3_u32, "ncoh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("W_rl_ack_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 2,
            ordering: ExperimentOrdering::RunlistAckProtocol,
            skip_pfifo_toggle: true,
            requires_sm: None,
        });
    }

    // ── X: INST_BIND + Runlist ACK (full nouveau-style) ──────────────
    for &(pccsr_t, pccsr_name) in &[(2_u32, "coh"), (3_u32, "ncoh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("X_bind_ack_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 2,
            ordering: ExperimentOrdering::InstBindWithRunlistAck,
            skip_pfifo_toggle: true,
            requires_sm: None,
        });
    }

    // ── Y: Preempt + INST_BIND + ACK (full sequence) ─────────────────
    for &(pccsr_t, pccsr_name) in &[(2_u32, "coh"), (3_u32, "ncoh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("Y_preempt_bind_ack_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 2,
            ordering: ExperimentOrdering::PreemptInstBindAck,
            skip_pfifo_toggle: true,
            requires_sm: None,
        });
    }

    // ── Z: Full PFIFO reinit + dispatch (nuke and pave) ────────────────
    configs.push(ExperimentConfig {
        name: "Z_full_reinit_coh",
        pccsr_target: 2,
        runlist_userd_target: 2,
        runlist_inst_target: 2,
        runlist_base_target: 2,
        ordering: ExperimentOrdering::FullPfifoReinitDispatch,
        skip_pfifo_toggle: true,
        requires_sm: None,
    });
    configs.push(ExperimentConfig {
        name: "Z2_reinit_directPbdma",
        pccsr_target: 2,
        runlist_userd_target: 2,
        runlist_inst_target: 2,
        runlist_base_target: 2,
        ordering: ExperimentOrdering::FullPfifoReinitDirectPbdma,
        skip_pfifo_toggle: true,
        requires_sm: None,
    });

    // ── Z3: No PMC reset, fast poll ───────────────────────────────────
    configs.push(ExperimentConfig {
        name: "Z3_noPmcReset_fastPoll",
        pccsr_target: 2,
        runlist_userd_target: 2,
        runlist_inst_target: 2,
        runlist_base_target: 2,
        ordering: ExperimentOrdering::NoPmcResetFastPoll,
        skip_pfifo_toggle: true,
        requires_sm: None,
    });

    // ── Z4/Z5/Z6: VID_MEM PCCSR INST_BIND hypothesis ──────────────────
    // Q experiments (VID_MEM pccsr_target=0) get BIT30; X/Z (SYS_MEM pccsr_target=2) don't.
    // These isolate whether PCCSR INST target controls scheduler channel registration.

    // Z4: INST_BIND + ACK with VID_MEM pccsr_target, SYS_MEM runlist entry
    configs.push(ExperimentConfig {
        name: "Z4_vidmemBind_sysInst",
        pccsr_target: 0,
        runlist_userd_target: 2,
        runlist_inst_target: 2,
        runlist_base_target: 2,
        ordering: ExperimentOrdering::InstBindWithRunlistAck,
        skip_pfifo_toggle: true,
        requires_sm: None,
    });

    // Z5: Z4 + VID_MEM in runlist entry inst_target (matches Q exactly)
    configs.push(ExperimentConfig {
        name: "Z5_vidmemBind_vidInst",
        pccsr_target: 0,
        runlist_userd_target: 2,
        runlist_inst_target: 0,
        runlist_base_target: 2,
        ordering: ExperimentOrdering::InstBindWithRunlistAck,
        skip_pfifo_toggle: true,
        requires_sm: None,
    });

    // Z6: Z5 + NCOH runlist base (full Q replication without PRAMIN)
    configs.push(ExperimentConfig {
        name: "Z6_fullQ_ncoh",
        pccsr_target: 0,
        runlist_userd_target: 3,
        runlist_inst_target: 0,
        runlist_base_target: 3,
        ordering: ExperimentOrdering::InstBindWithRunlistAck,
        skip_pfifo_toggle: true,
        requires_sm: None,
    });

    // Z7: VID_MEM bind + full PFIFO reinit (tests if PMC toggle was the Z blocker)
    configs.push(ExperimentConfig {
        name: "Z7_vidmemBind_reinit",
        pccsr_target: 0,
        runlist_userd_target: 2,
        runlist_inst_target: 2,
        runlist_base_target: 2,
        ordering: ExperimentOrdering::FullPfifoReinitDispatch,
        skip_pfifo_toggle: true,
        requires_sm: None,
    });

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
            requires_sm: None,
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
        requires_sm: None,
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
        requires_sm: None,
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
            requires_sm: None,
        });
    }

    configs
}

/// Build metal capability discovery experiments.
///
/// These experiments are separate from the PFIFO dispatch matrix — they
/// probe hardware capabilities rather than channel creation orderings.
/// Run them after the GPU is warm (GlowPlug has succeeded).
pub fn build_metal_discovery_matrix() -> Vec<ExperimentConfig> {
    vec![
        ExperimentConfig {
            name: "METAL_PowerStateSweep",
            pccsr_target: 0,
            runlist_userd_target: 0,
            runlist_inst_target: 0,
            runlist_base_target: 0,
            ordering: ExperimentOrdering::PowerStateSweep,
            skip_pfifo_toggle: true,
            requires_sm: None,
        },
        ExperimentConfig {
            name: "METAL_RegisterCartography",
            pccsr_target: 0,
            runlist_userd_target: 0,
            runlist_inst_target: 0,
            runlist_base_target: 0,
            ordering: ExperimentOrdering::RegisterCartography,
            skip_pfifo_toggle: true,
            requires_sm: None,
        },
        ExperimentConfig {
            name: "METAL_MemoryPathMatrix",
            pccsr_target: 0,
            runlist_userd_target: 0,
            runlist_inst_target: 0,
            runlist_base_target: 0,
            ordering: ExperimentOrdering::MemoryPathMatrix,
            skip_pfifo_toggle: true,
            requires_sm: None,
        },
        ExperimentConfig {
            name: "METAL_ClockDomainSweep",
            pccsr_target: 0,
            runlist_userd_target: 0,
            runlist_inst_target: 0,
            runlist_base_target: 0,
            ordering: ExperimentOrdering::ClockDomainSweep,
            skip_pfifo_toggle: true,
            requires_sm: None,
        },
        ExperimentConfig {
            name: "METAL_EngineProbe",
            pccsr_target: 0,
            runlist_userd_target: 0,
            runlist_inst_target: 0,
            runlist_base_target: 0,
            ordering: ExperimentOrdering::EngineProbe,
            skip_pfifo_toggle: true,
            requires_sm: None,
        },
        ExperimentConfig {
            name: "HBM2_PhyProbe",
            pccsr_target: 0,
            runlist_userd_target: 0,
            runlist_inst_target: 0,
            runlist_base_target: 0,
            ordering: ExperimentOrdering::Hbm2PhyProbe,
            skip_pfifo_toggle: true,
            requires_sm: None,
        },
        ExperimentConfig {
            name: "HBM2_TimingCapture",
            pccsr_target: 0,
            runlist_userd_target: 0,
            runlist_inst_target: 0,
            runlist_base_target: 0,
            ordering: ExperimentOrdering::Hbm2TimingCapture,
            skip_pfifo_toggle: true,
            requires_sm: None,
        },
        ExperimentConfig {
            name: "HBM2_TrainingAttempt",
            pccsr_target: 0,
            runlist_userd_target: 0,
            runlist_inst_target: 0,
            runlist_base_target: 0,
            ordering: ExperimentOrdering::Hbm2TrainingAttempt,
            skip_pfifo_toggle: true,
            requires_sm: None,
        },
        ExperimentConfig {
            name: "HBM2_MinimalSet",
            pccsr_target: 0,
            runlist_userd_target: 0,
            runlist_inst_target: 0,
            runlist_base_target: 0,
            ordering: ExperimentOrdering::Hbm2MinimalSet,
            skip_pfifo_toggle: true,
            requires_sm: None,
        },
    ]
}
