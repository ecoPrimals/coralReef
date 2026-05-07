// SPDX-License-Identifier: AGPL-3.0-or-later
//! GK110 PGOB (Power Gate Off Block) control — powers GPC compute domains.

/// Post-PGOB state summary returned from each disable variant.
#[derive(Debug, Clone)]
pub struct PgobOutcome {
    /// Whether GPCCS0 reads indicate a powered, non-sentinel falcon.
    pub gpc_alive: bool,
    /// Raw GPCCS0 CPUCTL register value after the sequence.
    pub gpccs0_cpuctl: u32,
    /// PG_STATUS (0x020008) register value after the sequence.
    pub pg_status: u32,
}

pub(super) static PGOB_POWER_STEPS: &[(u32, u32)] = &[
    (0x02_0520, 0xFFFF_FFFC),
    (0x02_0524, 0xFFFF_FFFE),
    (0x02_0524, 0xFFFF_FFFC),
    (0x02_0524, 0xFFFF_FFF8),
    (0x02_0524, 0xFFFF_FFE0),
    (0x02_0530, 0xFFFF_FFFE),
    (0x02_052C, 0xFFFF_FFFA),
    (0x02_052C, 0xFFFF_FFF0),
    (0x02_052C, 0xFFFF_FFC0),
    (0x02_052C, 0xFFFF_FF00),
    (0x02_052C, 0xFFFF_FC00),
    (0x02_052C, 0xFFFC_FC00),
    (0x02_052C, 0xFFF0_FC00),
    (0x02_052C, 0xFF80_FC00),
    (0x02_0528, 0xFFFF_FFFE),
    (0x02_0528, 0xFFFF_FFFC),
];

/// Diagnostic: verify preconditions for PGOB disable and report GPC
/// enrollment status.
///
/// Call before and after `gk110_pgob_disable` to bracket the transition.
pub fn pgob_diagnostic(guard: &super::hardware_guard::GuardedBar<'_>, label: &str) {
    let bar0 = guard.inner();
    let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };

    let pmc_enable = rd(0x000200);
    let pmc_enable2 = rd(0x000640);
    let psw_ctrl = rd(0x10_a78c);
    let pmu_cpuctl = rd(0x10_a100);
    let pg_elpg = rd(0x02_0000);
    let pg_ctrl = rd(0x02_0004);
    let pg_stat = rd(0x02_0008);
    let top_num_gpcs = rd(0x02_2430);
    let gpc_count_fecs = rd(0x40_9604);
    let pri_ring_gpc = rd(0x12_0078);

    let gpc0_cpuctl = rd(0x50_2100);
    let gpc0_version = rd(0x50_2004);
    let gpc1_cpuctl = rd(0x52_2100);

    let is_badf = |v: u32| v & 0xBADF_0000 == 0xBADF_0000;
    let gpc0_alive = gpc0_version != 0
        && gpc0_version != 0xDEAD_DEAD
        && !is_badf(gpc0_version);

    tracing::info!(
        label,
        pmc_enable = format_args!("{pmc_enable:#010x}"),
        pmc_enable2 = format_args!("{pmc_enable2:#010x}"),
        psw_ctrl = format_args!("{psw_ctrl:#010x}"),
        pmu_cpuctl = format_args!("{pmu_cpuctl:#010x}"),
        pmu_running = pmu_cpuctl & 0x20 != 0,
        pgraph_enabled = pmc_enable & (1 << 12) != 0,
        pmu_enabled = pmc_enable & (1 << 13) != 0,
        "PGOB preconditions"
    );
    tracing::info!(
        label,
        pg_elpg = format_args!("{pg_elpg:#010x}"),
        pg_ctrl = format_args!("{pg_ctrl:#010x}"),
        pg_stat = format_args!("{pg_stat:#010x}"),
        top_num_gpcs = format_args!("{top_num_gpcs:#010x}"),
        gpc_count_fecs = format_args!("{gpc_count_fecs:#010x}"),
        pri_ring_gpc_stations = pri_ring_gpc,
        "Power gating state"
    );
    tracing::info!(
        label,
        gpc0_cpuctl = format_args!("{gpc0_cpuctl:#010x}"),
        gpc0_version = format_args!("{gpc0_version:#010x}"),
        gpc1_cpuctl = format_args!("{gpc1_cpuctl:#010x}"),
        gpc0_alive,
        gpc0_badf = is_badf(gpc0_version),
        "GPC enrollment status"
    );
}

/// GK110 PGOB disable sequence — powers up GPC compute domains.
///
/// Matches kernel `gk110_pmu_pgob()` from `drivers/gpu/drm/nouveau/nvkm/subdev/pmu/gk110.c`.
///
/// 1. Disabling PGRAPH in PMC
/// 2. Setting PMC bit 27 (PGOB enable gate)
/// 3. Toggling PMU PGOB control (0x10a78c) bits 0-1
/// 4. Running a 16-step power domain enable sequence (0x0205xx)
/// 5. Toggling PMU PGOB control again
/// 6. Clearing PMC bit 27 and re-enabling PGRAPH
///
/// Accesses 0x10a78c (PMU PGOB) directly via `MappedBar`, bypassing
/// `GuardedBar`'s blocklist — this full protocol is the safety boundary.
pub fn gk110_pgob_disable(
    guard: &super::hardware_guard::GuardedBar<'_>,
) -> Result<PgobOutcome, crate::error::SovereignStagesError> {
    let bar0 = guard.inner();
    let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
    let wr = |reg: u32, val: u32| {
        let _ = bar0.write_u32(reg as usize, val);
    };
    let mask = |reg: u32, clr: u32, set: u32| {
        let cur = rd(reg);
        wr(reg, (cur & !clr) | set);
    };

    // Step 0 (ecoPrimals addition): Force ELPG off BEFORE the PGOB sequence.
    //
    // On GK210B, idle ELPG auto-gates GPC power domains. If ELPG is active
    // during the 0x0205xx power step writes, the domain never ungates (bit 31
    // stuck high). Nouveau's gk104_pmu_pgob does this via PG_CTRL bit 30;
    // gk110_pmu_pgob does not — but GK210B needs it.
    wr(0x02_0000, 0x0000_0000); // PG_ELPG: clear all auto-gating
    let pg_ctrl = rd(0x02_0004);
    wr(0x02_0004, (pg_ctrl & !0xC000_0000) | 0x4000_0000); // PG_CTRL: ELPG_DIS=1, PGOB=0
    rd(0x02_0004); // flush
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Match kernel gk110_pmu_pgob (drivers/gpu/drm/nouveau/nvkm/subdev/pmu/gk110.c).
    //
    // Step 1: Disable PGRAPH only (clear bit 12). Kernel does NOT touch bit 27 here.
    mask(0x000200, 0x0000_1000, 0x0000_0000);
    rd(0x000200); // flush

    // Step 2: Set PMC bit 27 (PGOB gate enable).
    // In a fresh POST, bit 27 starts at 0 (from DEVINIT), creating a 0→1 transition.
    // In warm handoff, bit 27 may already be 1. To guarantee the transition:
    mask(0x000200, 0x0800_0000, 0x0000_0000); // force bit 27 low first
    rd(0x000200);
    std::thread::sleep(std::time::Duration::from_millis(5));
    mask(0x000200, 0x0800_0000, 0x0800_0000); // 0→1 transition
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Step 3: PMU PGOB control (0x10a78c): set bit 1, pulse bit 0
    mask(0x10_a78c, 0x0000_0002, 0x0000_0002);
    mask(0x10_a78c, 0x0000_0001, 0x0000_0001);
    mask(0x10_a78c, 0x0000_0001, 0x0000_0000);

    // Step 4: NOP mask on 0x0206b4 — kernel does nvkm_mask(dev, 0x0206b4, 0, 0)
    // which is a read-modify-write (the write may trigger a hardware sync).
    mask(0x02_06b4, 0x0000_0000, 0x0000_0000);

    // Step 5: Magic power domain enable sequence — each write followed by
    // polling until bit 31 clears (nouveau uses nvkm_msec(2000)).
    let mut step_log = String::new();
    let mut timed_out_steps = Vec::new();
    for (i, &(addr, data)) in PGOB_POWER_STEPS.iter().enumerate() {
        let pre = rd(addr);
        wr(addr, data);
        let mut ok = false;
        let mut post = 0u32;
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            post = rd(addr);
            if post & 0x8000_0000 == 0 {
                ok = true;
                break;
            }
        }
        use std::fmt::Write;
        let _ = write!(
            step_log,
            "[{i}:{addr:#08x} pre={pre:#010x} wr={data:#010x} post={post:#010x} ok={ok}] "
        );
        if !ok {
            tracing::warn!(
                addr = format_args!("{addr:#010x}"),
                pre = format_args!("{pre:#010x}"),
                "gk110 PGOB: power step timed out (bit 31 stuck high)"
            );
            timed_out_steps.push((i, addr, pre, post));
        }
    }
    tracing::info!(steps = %step_log, "PGOB power steps");

    // Step 6: PMU PGOB control: clear bit 1, set bit 0, clear bit 0
    mask(0x10_a78c, 0x0000_0002, 0x0000_0000);
    mask(0x10_a78c, 0x0000_0001, 0x0000_0001);
    mask(0x10_a78c, 0x0000_0001, 0x0000_0000);

    // Step 7: Clear PMC bit 27 and re-enable PGRAPH (set bit 12)
    mask(0x000200, 0x0800_0000, 0x0000_0000);
    mask(0x000200, 0x0000_1000, 0x0000_1000);
    rd(0x000200);

    // Settle time for PGRAPH to come online
    std::thread::sleep(std::time::Duration::from_millis(50));

    // GK210B auto-clock-gates the GR HUB within nanoseconds of the last
    // PRI access. Disable BLCG/SLCG immediately after PGRAPH re-enable,
    // before any diagnostic reads that would give the gating hardware a
    // window to shut down the domain.
    wr(0x40_41f0, 0x0000_0000); // GR HUB BLCG off
    wr(0x40_41f4, 0x0000_0000); // GR HUB SLCG off
    wr(0x40_9890, 0x0000_0000); // FECS BLCG off
    wr(0x40_98b0, 0x0000_0000); // FECS BLCG2 off
    wr(0x40_0500, 0x0000_0000); // TRAP_EN off (quiesce GR HUB)

    let gr_hub_test = rd(0x400700);
    let fecs_test = rd(0x409100);
    let top_num_gpcs = rd(0x02_2430);
    let gpccs0_diag = rd(0x50_2100);
    let pg_status = rd(0x02_0008);
    let pg_ctrl = rd(0x02_0004);
    let pg_elpg = rd(0x02_0000);
    let pd0 = rd(0x02_0520);
    let pd1 = rd(0x02_0524);
    let pd2 = rd(0x02_0528);
    let pd3 = rd(0x02_052c);
    let pd4 = rd(0x02_0530);
    let psw_post = rd(0x10_a78c);
    tracing::info!(
        gr_hub = format_args!("{gr_hub_test:#010x}"),
        fecs = format_args!("{fecs_test:#010x}"),
        top_num_gpcs = format_args!("{top_num_gpcs:#010x}"),
        gpccs0_cpuctl = format_args!("{gpccs0_diag:#010x}"),
        pg_status = format_args!("{pg_status:#010x}"),
        pg_ctrl = format_args!("{pg_ctrl:#010x}"),
        pg_elpg = format_args!("{pg_elpg:#010x}"),
        psw = format_args!("{psw_post:#010x}"),
        "gk110 PGOB disable complete"
    );
    tracing::info!(
        pd0 = format_args!("{pd0:#010x}"),
        pd1 = format_args!("{pd1:#010x}"),
        pd2 = format_args!("{pd2:#010x}"),
        pd3 = format_args!("{pd3:#010x}"),
        pd4 = format_args!("{pd4:#010x}"),
        "PGOB power domain state after sequence"
    );

    if let Some(&(step_index, addr, pre, post)) = timed_out_steps.first() {
        return Err(crate::error::SovereignStagesError::PgobStepTimeout {
            step_index,
            addr,
            pre,
            post,
        });
    }

    let is_badf = |v: u32| v & 0xBADF_0000 == 0xBADF_0000;
    let gpc_alive = gpccs0_diag != 0xDEAD_DEAD && gpccs0_diag != 0 && !is_badf(gpccs0_diag);
    Ok(PgobOutcome { gpc_alive, gpccs0_cpuctl: gpccs0_diag, pg_status })
}

/// Lightweight PGOB power-domain un-gate: runs the magic 0x020520-0x020530
/// sequence and PMU PGOB control, but does NOT toggle PMC bit 12 or bit 27.
///
/// Use when PGRAPH is already enabled and FECS is in "software halt" —
/// a PMC toggle would put FECS into "hardware reset halt" where STARTCPU
/// is silently ignored.
#[expect(dead_code, reason = "WIP: hotspring Kepler boot strategies")]
pub(super) fn gk110_pgob_ungate_only(
    guard: &super::hardware_guard::GuardedBar<'_>,
) -> Result<PgobOutcome, crate::error::SovereignStagesError> {
    let bar0 = guard.inner();
    let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
    let wr = |reg: u32, val: u32| {
        let _ = bar0.write_u32(reg as usize, val);
    };
    let mask = |reg: u32, clr: u32, set: u32| {
        let cur = rd(reg);
        wr(reg, (cur & !clr) | set);
    };

    // Force ELPG off before the ungate sequence (same rationale as gk110_pgob_disable).
    wr(0x02_0000, 0x0000_0000);
    let pg_ctrl = rd(0x02_0004);
    wr(0x02_0004, (pg_ctrl & !0xC000_0000) | 0x4000_0000);
    rd(0x02_0004);
    std::thread::sleep(std::time::Duration::from_millis(10));

    // PMU PGOB control: set bit 1, pulse bit 0
    mask(0x10_a78c, 0x0000_0002, 0x0000_0002);
    mask(0x10_a78c, 0x0000_0001, 0x0000_0001);
    mask(0x10_a78c, 0x0000_0001, 0x0000_0000);

    // NOP mask sync
    mask(0x02_06b4, 0x0000_0000, 0x0000_0000);

    // Power domain enable sequence (same as gk110_pgob_disable step 5).
    let mut timed_out_steps = Vec::new();
    for (i, &(addr, data)) in PGOB_POWER_STEPS.iter().enumerate() {
        let pre = rd(addr);
        wr(addr, data);
        let mut ok = false;
        let mut post = 0u32;
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            post = rd(addr);
            if post & 0x8000_0000 == 0 {
                ok = true;
                break;
            }
        }
        if !ok {
            tracing::warn!(
                addr = format_args!("{addr:#010x}"),
                "pgob_ungate: step timed out"
            );
            timed_out_steps.push((i, addr, pre, post));
        }
    }

    // PMU PGOB control cleanup: clear bit 1, pulse bit 0
    mask(0x10_a78c, 0x0000_0002, 0x0000_0000);
    mask(0x10_a78c, 0x0000_0001, 0x0000_0001);
    mask(0x10_a78c, 0x0000_0001, 0x0000_0000);

    let gpccs0 = rd(0x50_2100);
    let pg_status = rd(0x02_0008);
    let gr_hub = rd(0x40_0000);
    let fecs = rd(0x40_9100);
    let is_badf = |v: u32| v & 0xBADF_0000 == 0xBADF_0000;
    let gpc_alive = gpccs0 != 0xDEAD_DEAD && gpccs0 != 0 && !is_badf(gpccs0);
    tracing::info!(
        gpc_alive,
        gr_hub = format_args!("{gr_hub:#010x}"),
        fecs = format_args!("{fecs:#010x}"),
        gr_hub_ok = gr_hub != 0xDEAD_DEAD && gr_hub & 0xBAD0_0000 != 0xBAD0_0000,
        "pgob_ungate_only complete"
    );

    if let Some(&(step_index, addr, pre, post)) = timed_out_steps.first() {
        return Err(crate::error::SovereignStagesError::PgobStepTimeout {
            step_index,
            addr,
            pre,
            post,
        });
    }
    Ok(PgobOutcome { gpc_alive, gpccs0_cpuctl: gpccs0, pg_status })
}

/// GK104-style PGOB disable using PG_CTRL register (0x020004).
///
/// From `drivers/gpu/drm/nouveau/nvkm/subdev/pmu/gk104.c`:
/// Instead of the magic 0x0205xx power domain writes, GK104 uses a single
/// PG_CTRL write: bit 30 = ELPG_DISABLE, bit 31 = PGOB_ENABLE.
/// For ungating GPCs: set bit 30 (disable ELPG), clear bit 31 (disable PGOB).
///
/// The PSW + PMC bit 27 wrapper sequence is identical to gk110.
/// Also checks fuse 0x02271c bit 0 — if not set, PGOB is not needed.
pub fn gk104_pgob_disable(
    guard: &super::hardware_guard::GuardedBar<'_>,
) -> PgobOutcome {
    let bar0 = guard.inner();
    let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
    let wr = |reg: u32, val: u32| {
        let _ = bar0.write_u32(reg as usize, val);
    };
    let mask = |reg: u32, clr: u32, set: u32| {
        let cur = rd(reg);
        wr(reg, (cur & !clr) | set);
    };
    let is_badf = |v: u32| v & 0xBADF_0000 == 0xBADF_0000;

    // Fuse check: 0x02271c bit 0 indicates PGOB support.
    // (nvkm_fuse_read at 0x022400 + 0x31c = 0x02271c)
    let fuse_31c = rd(0x02_271c);
    let pgob_fused = fuse_31c & 1 != 0;
    tracing::info!(
        fuse_31c = format_args!("{fuse_31c:#010x}"),
        pgob_fused,
        "gk104 PGOB: fuse check (0x02271c)"
    );
    if !pgob_fused {
        tracing::info!("PGOB not fused — skipping gk104_pgob_disable");
        let gpccs0 = rd(0x50_2100);
        return PgobOutcome {
            gpc_alive: gpccs0 != 0xDEAD_DEAD && gpccs0 != 0 && !is_badf(gpccs0),
            gpccs0_cpuctl: gpccs0,
            pg_status: rd(0x02_0008),
        };
    }

    // Step 1: Disable PGRAPH (clear PMC bit 12)
    mask(0x000200, 0x0000_1000, 0x0000_0000);
    rd(0x000200);

    // Step 2: Set PMC bit 27
    mask(0x000200, 0x0800_0000, 0x0800_0000);
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Step 3: PSW phase 1
    mask(0x10_a78c, 0x0000_0002, 0x0000_0002);
    mask(0x10_a78c, 0x0000_0001, 0x0000_0001);
    mask(0x10_a78c, 0x0000_0001, 0x0000_0000);

    // Step 4: PG_CTRL — enable=false → set bit 30, clear bit 31
    let pg_before = rd(0x02_0004);
    mask(0x02_0004, 0xc000_0000, 0x4000_0000);
    let pg_after = rd(0x02_0004);
    tracing::info!(
        pg_ctrl_before = format_args!("{pg_before:#010x}"),
        pg_ctrl_after = format_args!("{pg_after:#010x}"),
        "gk104 PGOB: PG_CTRL write (bit30=ELPG_DIS, bit31=0)"
    );
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Step 5: PSW phase 2
    mask(0x10_a78c, 0x0000_0002, 0x0000_0000);
    mask(0x10_a78c, 0x0000_0001, 0x0000_0001);
    mask(0x10_a78c, 0x0000_0001, 0x0000_0000);

    // Step 6: Clear PMC bit 27, enable PGRAPH (set bit 12)
    mask(0x000200, 0x0800_0000, 0x0000_0000);
    mask(0x000200, 0x0000_1000, 0x0000_1000);
    rd(0x000200);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let gpccs0 = rd(0x50_2100);
    let gr_nr = rd(0x40_9604);
    let fecs = rd(0x40_9100);
    let pg_status = rd(0x02_0008);
    let gpc_alive = gpccs0 != 0xDEAD_DEAD && !is_badf(gpccs0) && gpccs0 != 0;
    tracing::info!(
        gpccs0_cpuctl = format_args!("{gpccs0:#010x}"),
        gr_gpc_nr = format_args!("{gr_nr:#010x}"),
        fecs = format_args!("{fecs:#010x}"),
        gpc_alive,
        "gk104 PGOB disable complete"
    );
    PgobOutcome { gpc_alive, gpccs0_cpuctl: gpccs0, pg_status }
}

/// nvidia-470 proprietary PGOB disable — PSW-only handshake.
///
/// Derived from static analysis of `_nv029216rm` in `nv-kernel.o_binary`
/// (nvidia-470.256.02). Unlike Nouveau's `gk110_pmu_pgob`, this sequence
/// does NOT use the `0x0205xx` power domain registers (which cause PRIVRING
/// faults on GK210B). It communicates ungate intent solely through the PSW
/// register at 0x10a78c:
///
/// - Bit 0: PSW trigger (set to execute, then clear)
/// - Bit 1: PGOB state request (1 = power-gated, 0 = ungated)
///
/// Prerequisites: PMU falcon should be powered on (PMC bit 13 set in 0x200).
/// On a warm-caught K80 after nouveau POST, the PMU is typically running.
pub fn nvidia470_pgob_disable(
    guard: &super::hardware_guard::GuardedBar<'_>,
) -> PgobOutcome {
    let bar0 = guard.inner();
    let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
    let wr = |reg: u32, val: u32| {
        let _ = bar0.write_u32(reg as usize, val);
    };
    let is_badf = |v: u32| v & 0xBADF_0000 == 0xBADF_0000;

    let pre = rd(0x10_a78c);
    let pmu_cpuctl = rd(0x10_a100);
    let pmc = rd(0x200);
    tracing::info!(
        psw_pre = format_args!("{pre:#010x}"),
        pmu_cpuctl = format_args!("{pmu_cpuctl:#010x}"),
        pmc = format_args!("{pmc:#010x}"),
        "nvidia470 PGOB disable: starting PSW-only sequence"
    );

    // Step 1: Read PSW, clear bit 1 (request ungated state)
    let val = rd(0x10_a78c);
    wr(0x10_a78c, val & !0x02);

    // Step 2: Read PSW, set bit 0 (trigger the PSW command)
    let val = rd(0x10_a78c);
    wr(0x10_a78c, val | 0x01);

    // Step 3: Read PSW, clear bit 0 (release trigger)
    let val = rd(0x10_a78c);
    wr(0x10_a78c, val & !0x01);

    // Brief settle for the PMU to process the power state change
    std::thread::sleep(std::time::Duration::from_millis(50));

    let post = rd(0x10_a78c);
    let gpccs0 = rd(0x50_2100);
    let gr_hub = rd(0x40_0000);
    let fecs = rd(0x40_9100);
    let pg_status = rd(0x02_0008);
    let gpc_alive = gpccs0 != 0xDEAD_DEAD && gpccs0 != 0 && !is_badf(gpccs0);
    tracing::info!(
        psw_post = format_args!("{post:#010x}"),
        gpccs0_cpuctl = format_args!("{gpccs0:#010x}"),
        gr_hub = format_args!("{gr_hub:#010x}"),
        fecs = format_args!("{fecs:#010x}"),
        gpc_alive,
        "nvidia470 PGOB disable complete"
    );
    PgobOutcome { gpc_alive, gpccs0_cpuctl: gpccs0, pg_status }
}

/// nvidia-470 proprietary PGOB enable — re-gates GPCs for power saving.
///
/// Derived from `_nv029114rm` in `nv-kernel.o_binary`.
/// Inverse of `nvidia470_pgob_disable`: sets bit 1, triggers, clears trigger.
#[expect(dead_code, reason = "WIP: hotspring Kepler boot strategies")]
pub(super) fn nvidia470_pgob_enable(guard: &super::hardware_guard::GuardedBar<'_>) {
    let bar0 = guard.inner();
    let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
    let wr = |reg: u32, val: u32| {
        let _ = bar0.write_u32(reg as usize, val);
    };

    // Set bit 1 (request power-gated state)
    let val = rd(0x10_a78c);
    wr(0x10_a78c, val | 0x02);

    // Trigger (set bit 0)
    let val = rd(0x10_a78c);
    wr(0x10_a78c, val | 0x01);

    // Release trigger (clear bit 0)
    let val = rd(0x10_a78c);
    wr(0x10_a78c, val & !0x01);

    tracing::info!(
        psw = format_args!("{:#010x}", rd(0x10_a78c)),
        "nvidia470 PGOB enable complete (GPCs power-gated)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pgob_power_steps_table_has_expected_entries() {
        assert_eq!(PGOB_POWER_STEPS.len(), 16);
    }

    #[test]
    fn pgob_power_steps_addresses_are_aligned() {
        for &(addr, _) in PGOB_POWER_STEPS {
            assert_eq!(addr % 4, 0, "register address {addr:#010x} is not dword-aligned");
        }
    }

    #[test]
    fn pgob_power_steps_are_in_expected_range() {
        for &(addr, _) in PGOB_POWER_STEPS {
            assert!(
                (0x02_0520..=0x02_0530).contains(&addr),
                "unexpected address {addr:#010x} outside 0x020520..0x020530"
            );
        }
    }

    #[test]
    fn pgob_outcome_gpc_alive_default() {
        let outcome = PgobOutcome {
            gpc_alive: true,
            gpccs0_cpuctl: 0x1234_5678,
            pg_status: 0x0000_0000,
        };
        assert!(outcome.gpc_alive);
        assert_eq!(outcome.gpccs0_cpuctl, 0x1234_5678);
    }

    #[test]
    fn pgob_outcome_clone() {
        let outcome = PgobOutcome {
            gpc_alive: false,
            gpccs0_cpuctl: 0xDEAD_DEAD,
            pg_status: 0xFF,
        };
        let cloned = outcome.clone();
        assert_eq!(cloned.gpc_alive, outcome.gpc_alive);
        assert_eq!(cloned.gpccs0_cpuctl, outcome.gpccs0_cpuctl);
        assert_eq!(cloned.pg_status, outcome.pg_status);
    }

    #[test]
    fn pgob_outcome_debug_format() {
        let outcome = PgobOutcome {
            gpc_alive: true,
            gpccs0_cpuctl: 0x20,
            pg_status: 0,
        };
        let debug = format!("{outcome:?}");
        assert!(debug.contains("gpc_alive: true"));
        assert!(debug.contains("gpccs0_cpuctl"));
    }
}
