// SPDX-License-Identifier: AGPL-3.0-only
//! Full warm-up sequence — bring the GPU from any state to Warm.

use std::sync::Arc;

use super::super::devinit;
use super::super::diagnostic::interpreter::memory_probe;
use super::super::hbm2_training;
use super::super::oracle::{DigitalPmu, OracleState};
use super::super::pfifo;
use super::super::registers::{cg, misc, pri};
use super::GlowPlug;
use super::constants::is_dangerous_register;
use super::types::{GpuThermalState, StepSnapshot, WarmResult};

impl GlowPlug<'_> {
    /// Full warm-up sequence — bring the GPU from any state to Warm.
    pub fn warm(&self) -> WarmResult {
        let mut log = Vec::new();
        let mut step_snapshots = Vec::new();
        let initial_state = self.check_state();
        log.push(format!("initial state: {initial_state:?}"));

        if initial_state == GpuThermalState::Warm {
            return WarmResult {
                initial_state,
                final_state: GpuThermalState::Warm,
                success: true,
                memory: None,
                log,
                step_snapshots,
            };
        }

        // Step 0: D3hot → D0
        if initial_state == GpuThermalState::D3Hot {
            run_step_d3hot_to_d0(self, &mut log, &mut step_snapshots);
            if self.check_state() == GpuThermalState::D3Hot {
                return WarmResult {
                    initial_state,
                    final_state: GpuThermalState::D3Hot,
                    success: false,
                    memory: None,
                    log,
                    step_snapshots,
                };
            }
        }

        // Step 1: PMC_ENABLE — clock all engines
        if matches!(
            initial_state,
            GpuThermalState::ColdGated | GpuThermalState::EnginesClocked
        ) {
            run_step_pmc_enable(self, &mut log, &mut step_snapshots);
        }

        // Step 2: PFIFO reset cycle (bit 8)
        let state_after_pmc = self.check_state();
        if matches!(
            state_after_pmc,
            GpuThermalState::ColdGated | GpuThermalState::EnginesClocked
        ) {
            run_step_pfifo_reset(self, &mut log, &mut step_snapshots);
        }

        // Step 2.5: PRI bus health check
        run_step_pri_health(self, &mut log, &mut step_snapshots);

        // Step 2.75: Clock gating sweep
        run_step_clock_gating(self, &mut log, &mut step_snapshots);

        // Step 2.9: Digital PMU emulation
        run_step_digital_pmu(self, &mut log, &mut step_snapshots);

        // Step 3: VRAM strategies (if dead)
        let state_after_pfifo = self.check_state();
        if state_after_pfifo == GpuThermalState::PfifoAliveVramDead {
            run_step_vram_strategies(self, &mut log, &mut step_snapshots);
        }

        // Step 4: BAR2 page tables (requires VRAM)
        let state_after_fb = self.check_state();
        if matches!(
            state_after_fb,
            GpuThermalState::VramAliveBar2Dead | GpuThermalState::Warm
        ) && state_after_fb == GpuThermalState::VramAliveBar2Dead
        {
            run_step_bar2(self, &mut log);
        }

        // Step 5: Verify final state with full memory topology
        let final_state = self.check_state();
        let memory = Some(memory_probe::discover_memory_topology(
            self.bar0,
            Arc::clone(&self.container),
        ));

        let success = final_state == GpuThermalState::Warm;
        log.push(format!("final state: {final_state:?} success={success}"));

        WarmResult {
            initial_state,
            final_state,
            success,
            memory,
            log,
            step_snapshots,
        }
    }
}

fn run_step_d3hot_to_d0(
    gp: &GlowPlug<'_>,
    log: &mut Vec<String>,
    step_snapshots: &mut Vec<StepSnapshot>,
) {
    if let Some(bdf) = &gp.bdf {
        log.push("step 0: GPU in D3hot — forcing D0 via PCI PMCSR write".into());
        let before_d0 = gp.snap();
        match devinit::force_pci_d0(bdf) {
            Ok(()) => {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let after_d0 = gp.snap();
                step_snapshots.push(StepSnapshot {
                    step: "D3hot → D0 force".into(),
                    before: before_d0,
                    after: after_d0,
                });
                let post_d0 = gp.check_state();
                log.push(format!("  After D0 force: {post_d0:?}"));
                if post_d0 == GpuThermalState::D3Hot {
                    log.push("  D0 force failed — device still in D3hot.".into());
                }
            }
            Err(e) => log.push(format!("  D0 force failed: {e}")),
        }
    } else {
        log.push("GPU in D3hot — no BDF set, cannot force D0.".into());
    }
}

fn run_step_pmc_enable(
    gp: &GlowPlug<'_>,
    log: &mut Vec<String>,
    step_snapshots: &mut Vec<StepSnapshot>,
) {
    let pmc_off = gp.pmc_enable_off();
    let before_pmc = gp.snap();

    if let Some(ref metal) = gp.metal {
        let steps = metal.warmup_sequence();
        for (i, step) in steps.iter().enumerate() {
            log.push(format!("step 1.{i}: {}", step.description));
            let step_before = gp.snap();
            for w in &step.writes {
                if let Some(mask) = w.mask {
                    let cur = gp.r(w.offset);
                    gp.w(w.offset, (cur & mask) | w.value);
                } else {
                    gp.w(w.offset, w.value);
                }
            }
            if step.delay_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(step.delay_ms));
            }
            let step_after = gp.snap();
            step_snapshots.push(StepSnapshot {
                step: (*step.description).to_owned(),
                before: step_before,
                after: step_after,
            });
            for v in &step.verify {
                let val = gp.r(v.offset);
                let ok = (val & v.mask) == (v.expected & v.mask);
                log.push(format!("  verify {:#x}: {val:#010x} (ok={ok})", v.offset));
            }
        }
    } else {
        log.push("step 1: PMC_ENABLE = 0xFFFFFFFF".into());
        gp.w(pmc_off, 0xFFFF_FFFF);
        std::thread::sleep(std::time::Duration::from_millis(50));

        let after_pmc = gp.snap();
        step_snapshots.push(StepSnapshot {
            step: "PMC_ENABLE = 0xFFFFFFFF".into(),
            before: before_pmc,
            after: after_pmc,
        });
        let pmc_after = gp.r(pmc_off);
        log.push(format!("  PMC_ENABLE after: {pmc_after:#010x}"));
    }
}

fn run_step_pfifo_reset(
    gp: &GlowPlug<'_>,
    log: &mut Vec<String>,
    step_snapshots: &mut Vec<StepSnapshot>,
) {
    let pmc_off = gp.pmc_enable_off();
    log.push("step 2: PFIFO reset cycle (PMC bit 8)".into());
    let before_pfifo = gp.snap();
    let pmc_cur = gp.r(pmc_off);
    let pfifo_bit: u32 = 1 << 8;
    gp.w(pmc_off, pmc_cur & !pfifo_bit);
    std::thread::sleep(std::time::Duration::from_millis(20));
    gp.w(pmc_off, pmc_cur | pfifo_bit);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let after_pfifo = gp.snap();
    step_snapshots.push(StepSnapshot {
        step: "PFIFO reset cycle".into(),
        before: before_pfifo,
        after: after_pfifo,
    });

    if let Some(pbdma_off) = gp.pbdma_map_off() {
        let pbdma_after = gp.r(pbdma_off);
        log.push(format!("  PBDMA_MAP after: {pbdma_after:#010x}"));
    }
}

fn run_step_pri_health(
    gp: &GlowPlug<'_>,
    log: &mut Vec<String>,
    step_snapshots: &mut Vec<StepSnapshot>,
) {
    let (alive, faulted, pri_log) = gp.check_pri_health();
    log.extend(pri_log);
    step_snapshots.push(StepSnapshot {
        step: format!("PRI health check: {alive} alive, {faulted} faulted"),
        before: gp.snap(),
        after: gp.snap(),
    });
}

fn run_step_clock_gating(
    gp: &GlowPlug<'_>,
    log: &mut Vec<String>,
    step_snapshots: &mut Vec<StepSnapshot>,
) {
    let before_cg = gp.snap();
    let mut cg_log = Vec::new();
    cg_log.push("step 2.75: Clock gating sweep — disabling CG on faulted domains".into());

    // Phase 1: Sweep all known CG control registers
    for &(offset, name) in cg::CG_SWEEP_TARGETS {
        let old = gp.r(offset);
        let is_err = pri::is_pri_error(old);
        if is_err {
            cg_log.push(format!(
                "  {name} [{offset:#08x}]: PRI error {old:#010x} — domain unreachable"
            ));
        } else {
            gp.w(offset, cg::CG_DISABLE);
            let new = gp.r(offset);
            if old != new {
                cg_log.push(format!(
                    "  {name} [{offset:#08x}]: {old:#010x} → {new:#010x}"
                ));
            }
        }
    }

    // Phase 2: Per-FBPA clock gating disable
    for i in 0..cg::FBPA_COUNT {
        let base = cg::FBPA0_BASE + i * cg::FBPA_STRIDE;
        let cg_reg = base + cg::FBPA_CG_OFFSET;
        let old = gp.r(cg_reg);
        if pri::is_pri_error(old) {
            cg_log.push(format!(
                "  FBPA{i} CG [{cg_reg:#08x}]: PRI error {old:#010x}"
            ));
            gp.w(cg_reg, cg::CG_DISABLE);
        } else {
            gp.w(cg_reg, cg::CG_DISABLE);
            let new = gp.r(cg_reg);
            if old != new {
                cg_log.push(format!(
                    "  FBPA{i} CG [{cg_reg:#08x}]: {old:#010x} → {new:#010x}"
                ));
            }
        }
    }

    // Phase 3: Per-LTC clock gating disable
    for i in 0..cg::LTC_COUNT {
        let base = cg::LTC0_BASE + i * cg::LTC_STRIDE;
        let cg_reg = base + cg::LTC_CG_OFFSET;
        let old = gp.r(cg_reg);
        if pri::is_pri_error(old) {
            gp.w(cg_reg, cg::CG_DISABLE);
        } else {
            gp.w(cg_reg, cg::CG_DISABLE);
            let new = gp.r(cg_reg);
            if old != new {
                cg_log.push(format!(
                    "  LTC{i} CG [{cg_reg:#08x}]: {old:#010x} → {new:#010x}"
                ));
            }
        }
    }

    // Phase 4: PRI recovery after CG sweep
    let recovered = gp.recover_pri_bus();
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Phase 5: Re-probe domains
    let (alive2, faulted2, probe_log) = gp.check_pri_health();
    cg_log.push(format!(
        "  Post-CG sweep: {alive2} alive, {faulted2} faulted (recovery={})",
        if recovered { "ok" } else { "failed" }
    ));
    cg_log.extend(probe_log.into_iter().map(|l| format!("    {l}")));

    // Phase 6: PCLOCK PLL probe
    cg_log.push("  PCLOCK PLL probe:".into());
    let pclock_base = 0x0013_7000_usize;
    for &(off, name) in &[
        (0x000, "PCLOCK_CTL"),
        (0x004, "PCLOCK_STATUS"),
        (0x008, "PCLOCK_COEFF"),
        (0x010, "PCLOCK_PLL0"),
        (0x014, "PCLOCK_PLL1"),
        (0x020, "PCLOCK_BYPASS"),
        (0x050, "NVPLL_CTL"),
        (0x054, "NVPLL_COEFF"),
        (0x100, "MEMPLL_CTL"),
        (0x104, "MEMPLL_COEFF"),
    ] {
        let reg = pclock_base + off;
        let val = gp.r(reg);
        let err = if pri::is_pri_error(val) {
            format!(" ← {}", pri::decode_pri_error(val))
        } else {
            String::new()
        };
        cg_log.push(format!("    {name} [{reg:#08x}] = {val:#010x}{err}"));
    }

    let after_cg = gp.snap();
    step_snapshots.push(StepSnapshot {
        step: "Clock gating sweep + PLL probe".into(),
        before: before_cg,
        after: after_cg,
    });
    log.extend(cg_log);
}

fn run_step_digital_pmu(
    gp: &GlowPlug<'_>,
    log: &mut Vec<String>,
    step_snapshots: &mut Vec<StepSnapshot>,
) {
    let oracle_state = if let Some(ref state) = gp.oracle_state {
        Some(state.clone())
    } else if let Some(ref obdf) = gp.oracle_bdf {
        match OracleState::from_live_card(obdf) {
            Ok(state) => Some(state),
            Err(e) => {
                log.push(format!("step 2.9: oracle load failed: {e}"));
                None
            }
        }
    } else {
        None
    };

    if let Some(ref oracle) = oracle_state {
        log.push(format!(
            "step 2.9: Digital PMU — {} oracle registers from {}",
            oracle.registers.len(),
            oracle.source,
        ));
        let before_dpmu = gp.snap();

        let mut dpmu = DigitalPmu::new(gp.bar0, oracle);

        let (pll_applied, pll_skipped) = dpmu.program_root_plls();
        log.extend(dpmu.take_log());

        if pll_applied > 0 {
            let bypass_log = dpmu.program_pclock_bypass();
            log.extend(bypass_log);

            let pclock_val = gp.r(0x137000);
            let pclock_alive = !pri::is_pri_error(pclock_val);
            log.push(format!(
                "  Post-PLL: PCLOCK={pclock_val:#010x} ({})",
                if pclock_alive { "ALIVE" } else { "still gated" }
            ));

            if pclock_alive || pll_applied > 10 {
                log.push("  Running full digital PMU sequence...".into());
                let result = dpmu.execute();
                log.extend(result.log);

                if result.vram_unlocked {
                    log.push(format!(
                        "  *** DIGITAL PMU UNLOCKED VRAM after {:?}! ***",
                        result.vram_unlocked_after,
                    ));
                } else {
                    log.push(format!(
                        "  Digital PMU: {} applied, {} stuck, {} PRI-skipped — VRAM still dead",
                        result.applied, result.stuck, result.pri_skipped,
                    ));
                }
            }
        }

        let after_dpmu = gp.snap();
        step_snapshots.push(StepSnapshot {
            step: format!("Digital PMU ({pll_applied} PLLs, {pll_skipped} skipped)"),
            before: before_dpmu,
            after: after_dpmu,
        });

        gp.recover_pri_bus();
    }
}

fn run_step_vram_strategies(
    gp: &GlowPlug<'_>,
    log: &mut Vec<String>,
    _step_snapshots: &mut Vec<StepSnapshot>,
) {
    let devinit_status = devinit::DevinitStatus::probe(gp.bar0);
    log.push(format!(
        "step 3: VRAM dead — devinit_reg={:#010x} needs_post={}",
        devinit_status.devinit_reg, devinit_status.needs_post
    ));

    // Strategy 1: D3cold power cycle
    if devinit_status.needs_post
        && let Some(bdf) = &gp.bdf
    {
        log.push("step 3a: Attempting D3cold power cycle (boot ROM devinit)".into());
        match devinit::pci_power_cycle_devinit(bdf) {
            Ok(true) => {
                std::thread::sleep(std::time::Duration::from_millis(500));
                log.push("  Power cycle complete — re-checking devinit...".into());
                let post_status = devinit::DevinitStatus::probe(gp.bar0);
                if !post_status.needs_post && gp.check_vram() {
                    log.push("  *** DEVINIT COMPLETE + VRAM ALIVE! ***".into());
                } else {
                    log.push(format!(
                        "  Post-cycle: devinit={:#010x} needs_post={} vram={}",
                        post_status.devinit_reg,
                        post_status.needs_post,
                        if gp.check_vram() { "alive" } else { "dead" }
                    ));
                }
            }
            Ok(false) => log.push("  D3cold power cycle returned false.".into()),
            Err(e) => log.push(format!("  D3cold power cycle failed: {e}")),
        }
    }

    // Strategy 2: VBIOS script register writes
    if !gp.check_vram() {
        log.push("step 3b: Scanning VBIOS boot scripts for register writes".into());
        let rom_result = devinit::read_vbios_prom(gp.bar0).or_else(|e1| {
            log.push(format!("  PROM read failed: {e1}"));
            if let Some(bdf) = &gp.bdf {
                devinit::read_vbios_sysfs(bdf)
            } else {
                Err("no BDF for sysfs fallback".into())
            }
        });
        if let Ok(rom) = rom_result {
            match devinit::extract_boot_script_writes(&rom) {
                Ok(writes) => {
                    log.push(format!(
                        "  Found {} register writes in VBIOS scripts",
                        writes.len()
                    ));
                    let mut applied = 0;
                    for w in &writes {
                        if is_dangerous_register(w.reg as usize) {
                            continue;
                        }
                        if let Some(mask) = w.mask {
                            let cur = gp.r(w.reg as usize);
                            let new_val = (cur & mask) | w.value;
                            gp.w(w.reg as usize, new_val);
                        } else {
                            gp.w(w.reg as usize, w.value);
                        }
                        applied += 1;
                    }
                    log.push(format!(
                        "  Applied {applied}/{} script register writes",
                        writes.len()
                    ));
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if gp.check_vram() {
                        log.push("  *** VBIOS SCRIPT WRITES UNLOCKED VRAM! ***".into());
                    } else {
                        log.push("  VRAM still dead after script writes.".into());
                    }
                }
                Err(e) => log.push(format!("  Script extraction failed: {e}")),
            }
        }
    }

    if !gp.check_vram() {
        let recovered = gp.recover_pri_bus();
        log.push(format!(
            "  PRI recovery between strategies: {}",
            if recovered {
                "success"
            } else {
                "bus still faulted"
            }
        ));
    }

    // Strategy 2b: Sovereign HBM2 training
    if !gp.check_vram() {
        log.push("step 3b2: Attempting sovereign HBM2 training".into());
        use hbm2_training::{Hbm2Controller, Untrained, volta_hbm2};
        let ctrl =
            Hbm2Controller::<Untrained>::new(gp.bar0, gp.bdf.as_deref(), volta_hbm2::FBPA_COUNT);
        match ctrl
            .enable_phy()
            .and_then(|c| c.train_links())
            .and_then(|c| c.init_dram())
            .and_then(|c| c.verify_vram())
        {
            Ok(verified) => {
                let tlog = verified.training_log();
                log.push(format!(
                    "  *** SOVEREIGN HBM2 TRAINING SUCCEEDED ({} writes) ***",
                    tlog.write_count(),
                ));
            }
            Err(phase_err) => {
                log.push(format!(
                    "  HBM2 training failed at {}: {}",
                    phase_err.phase, phase_err.detail
                ));
                if !phase_err.register_snapshot.is_empty() {
                    for (off, val) in &phase_err.register_snapshot {
                        log.push(format!("    [{off:#010x}] = {val:#010x}"));
                    }
                }
            }
        }
    }

    if !gp.check_vram() {
        gp.recover_pri_bus();
    }

    // Strategy 2c: Enhanced devinit
    if !gp.check_vram() {
        log.push("step 3b3: Enhanced devinit with diagnostics".into());
        match devinit::execute_devinit_with_diagnostics(gp.bar0, gp.bdf.as_deref()) {
            Ok(true) => {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if gp.check_vram() {
                    log.push("  *** ENHANCED DEVINIT UNLOCKED VRAM! ***".into());
                } else {
                    log.push("  Enhanced devinit returned true but VRAM still dead.".into());
                }
            }
            Ok(false) => log.push("  Devinit reports already complete.".into()),
            Err(e) => log.push(format!("  Enhanced devinit failed: {e}")),
        }
    }

    if gp.oracle_bdf.is_some() && !gp.check_vram() {
        let recovered = gp.recover_pri_bus();
        log.push(format!(
            "  PRI recovery before oracle: {}",
            if recovered {
                "success"
            } else {
                "bus still faulted"
            }
        ));
    }

    // Strategy 3: Oracle register cloning
    if gp.oracle_bdf.is_some() && !gp.check_vram() {
        log.push("step 3c: Applying oracle register state".into());
        let (applied, stuck, total) = gp.apply_oracle_registers(log);
        if applied > 0 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if gp.check_vram() {
                log.push("  *** ORACLE REGISTERS UNLOCKED VRAM! ***".into());
            } else {
                log.push(format!(
                    "  Oracle: {applied}/{total} regs applied ({stuck} stuck) — VRAM still dead"
                ));
            }
        }
    }

    // Strategy 3b: Differential replay from oracle
    if gp.oracle_bdf.is_some() && !gp.check_vram() {
        log.push("step 3c2: Differential replay (domain-ordered) from oracle".into());
        if let Some(ref oracle_bdf) = gp.oracle_bdf {
            match hbm2_training::differential_training(gp.bar0, oracle_bdf) {
                Ok(result) => {
                    if result.success {
                        log.push(format!(
                            "  *** DIFFERENTIAL REPLAY UNLOCKED VRAM after {} domain! ***",
                            result.vram_unlocked_after.as_deref().unwrap_or("?"),
                        ));
                    } else {
                        log.push(format!(
                            "  Differential replay: {} domains applied, VRAM still dead",
                            result.domains_applied.len(),
                        ));
                    }
                }
                Err(e) => log.push(format!("  Differential replay failed: {e}")),
            }
        }
    }

    // Strategy 4: PMU FALCON devinit
    if !gp.check_vram() && devinit_status.needs_post {
        log.push("step 3d: Attempting PMU FALCON devinit (VBIOS + FALCON)".into());
        let rom_result = devinit::read_vbios_prom(gp.bar0).or_else(|e1| {
            log.push(format!("  PROM read failed: {e1}"));
            if let Some(bdf) = &gp.bdf {
                devinit::read_vbios_sysfs(bdf)
            } else {
                Err("no BDF for sysfs fallback".into())
            }
        });
        if let Ok(rom) = rom_result {
            match devinit::execute_devinit(gp.bar0, &rom) {
                Ok(true) => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if gp.check_vram() {
                        log.push("  *** PMU DEVINIT SUCCEEDED + VRAM ALIVE! ***".into());
                    } else {
                        log.push("  PMU devinit completed but VRAM still dead.".into());
                    }
                }
                Ok(false) => log.push("  Devinit reports already complete.".into()),
                Err(e) => log.push(format!("  PMU devinit failed: {e}")),
            }
        }
    }

    // Strategy 5: Register-level FB init probe
    if !gp.check_vram() {
        log.push("step 3e: Attempting register-level FB init probe...".into());
        let (topo, deltas) = memory_probe::attempt_fb_init(gp.bar0, Arc::clone(&gp.container));
        if topo.vram_accessible {
            log.push("  FB init probe succeeded! VRAM is accessible.".into());
        } else {
            log.push(format!(
                "  FB init probe: VRAM still dead ({} deltas)",
                deltas.len()
            ));
        }
    }

    let pfb_regs = memory_probe::snapshot_pfb_registers(gp.bar0);
    log.push(format!("  NV_PFB registers readable: {}", pfb_regs.len()));
}

fn run_step_bar2(gp: &GlowPlug<'_>, log: &mut Vec<String>) {
    log.push("step 4: Setting up BAR2 page tables in VRAM".into());
    match pfifo::setup_bar2_page_table(gp.bar0) {
        Ok(()) => {
            log.push("  BAR2 page tables configured successfully.".into());
            let bar2 = gp.r(misc::PBUS_BAR2_BLOCK);
            log.push(format!("  BAR2_BLOCK = {bar2:#010x}"));
        }
        Err(e) => {
            log.push(format!("  BAR2 setup failed: {e}"));
        }
    }
}
