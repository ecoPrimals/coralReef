// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kepler cold-boot initialization — PRI ring, clocks, PGRAPH reset, FECS boot.

use crate::vfio::device::MappedBar;

/// Full Kepler cold-boot initialization: PRI ring → clocks → PGRAPH reset → FECS boot.
///
/// Must be called BEFORE PFIFO init / channel creation on cold VFIO K80s.
///
/// After PCI FLR, the PRI ring has zero topology (no stations respond) because
/// engine clocks are gated. The VBIOS normally bootstraps the ring during POST;
/// we replicate the exact sequence discovered by reverse-engineering the VBIOS
/// DEVINIT scripts on GK210:
///
/// 1. Set minimal PMC_ENABLE (PDAEMON + PRING only)
/// 2. Configure hub station parameters (from VBIOS DEVINIT)
/// 3. Issue ring INIT command 0x03 (not 0x04!) — discovers topology
/// 4. Program full PCLOCK PLL tree while ring is alive
/// 5. Set full PMC_ENABLE (all engines)
/// 6. Re-init ring (stations now have clocks, all respond)
/// 7. PGRAPH reset + FECS/GPCCS falcon boot
pub(crate) fn kepler_cold_init(bar0: &MappedBar) {
    use super::hardware_guard::GuardedBar;
    use crate::nv::kepler_falcon;

    const PMC_ENABLE: u32 = 0x200;
    const PMC_PGRAPH: u32 = 1 << 12;
    const NV470_PMC_ENABLE: u32 = 0xe011312c;

    let guard = match GuardedBar::new(bar0, 32) {
        Ok(g) => g,
        Err(refusal) => {
            tracing::error!("{refusal}");
            tracing::error!("Kepler cold init ABORTED — GPU is dead before we even started");
            return;
        }
    };

    let r = guard.read_fn();
    let w = |reg: u32, val: u32| {
        if let Err(refusal) = guard.write_u32(reg, val) {
            tracing::error!(
                reg = format_args!("{reg:#010x}"),
                val = format_args!("{val:#010x}"),
                "write refused: {refusal}"
            );
        }
    };

    let pmc_before = r(PMC_ENABLE);
    tracing::info!(
        pmc_before = format_args!("{pmc_before:#010x}"),
        "Kepler cold init: current PMC_ENABLE"
    );

    // ── Phase 1: Bootstrap PRI ring with minimal PMC ──
    // Only PDAEMON (bit 13) + PRING (bit 5) — fewer stations on the ring means
    // the INIT token can traverse the ring even without engine clocks.
    w(PMC_ENABLE, 0x0000_2020);
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Configure hub station parameters extracted from K80 VBIOS DEVINIT:
    // These must be set BEFORE the ring INIT command.
    super::pri::write_kepler_hub_station_params(&w);

    // Apply Nouveau's privring timing (gk104_privring_init)
    super::pri::gk104_privring_timing(&r, &w);

    // VBIOS uses command 0x03 (bits 0+1), NOT 0x04 as in nouveau.
    // 0x04 never completes on cold K80 — 0x03 is the correct INIT command.
    let phase1_ok = super::pri::vbios_pri_ring_init(&r, &w);

    let hub = r(0x12_0070);
    let rop = r(0x12_0074);
    let gpc = r(0x12_0078);
    tracing::info!(
        ok = phase1_ok,
        hub_stations = hub,
        rop_stations = rop,
        gpc_stations = gpc,
        pclock = format_args!("{:#010x}", r(0x13_0000)),
        "Phase 1: PRI ring init (0x70=hub, 0x74=rop, 0x78=gpc)"
    );

    if !phase1_ok {
        tracing::error!("PRI ring INIT failed with minimal PMC — cannot proceed");
        return;
    }

    if let Err(e) = guard.check_canary() {
        tracing::error!("GPU died during Phase 1 PRI ring init: {e}");
        return;
    }

    // ── Phase 1.5: PCLOCK domain power-up diagnostic ──
    // Probe the entire clock source chain to understand why PCLOCK registers
    // (0x130000-0x137FFF) are unwritable despite the PRI station responding.
    {
        let devinit_done = r(0x001540);
        let pmc_enable2 = r(0x000640);
        let pbus_debug0 = r(0x001084);
        let pbus_debug1 = r(0x001098);
        let pmc_enable = r(0x000200);
        let pmc_device = r(0x000204);
        tracing::info!(
            devinit_done = format_args!("{devinit_done:#010x}"),
            pmc_enable = format_args!("{pmc_enable:#010x}"),
            pmc_enable2 = format_args!("{pmc_enable2:#010x}"),
            pmc_device = format_args!("{pmc_device:#010x}"),
            pbus_debug0 = format_args!("{pbus_debug0:#010x}"),
            pbus_debug1 = format_args!("{pbus_debug1:#010x}"),
            "Phase 1.5: PMC / PBUS state before DEVINIT"
        );

        let pnvio_xtal = r(0x00e220);
        let pnvio_ctrl = r(0x00e000);
        let pnvio_cfg0 = r(0x00e004);
        let pnvio_cfg1 = r(0x00e018);
        let pnvio_pll_ref = r(0x00e800);
        let ptimer_0 = r(0x009400);
        let ptimer_1 = r(0x009410);
        tracing::info!(
            pnvio_xtal = format_args!("{pnvio_xtal:#010x}"),
            pnvio_ctrl = format_args!("{pnvio_ctrl:#010x}"),
            pnvio_cfg0 = format_args!("{pnvio_cfg0:#010x}"),
            pnvio_cfg1 = format_args!("{pnvio_cfg1:#010x}"),
            pnvio_pll_ref = format_args!("{pnvio_pll_ref:#010x}"),
            ptimer_0 = format_args!("{ptimer_0:#010x}"),
            ptimer_1 = format_args!("{ptimer_1:#010x}"),
            "Phase 1.5: PNVIO / crystal oscillator state"
        );

        let pclock_samples: [(u32, &str); 8] = [
            (0x13_0000, "PLL0_CTRL"),
            (0x13_0004, "PLL0_COEF"),
            (0x13_2000, "CLK_DOM0"),
            (0x13_4000, "CLK_PROG0"),
            (0x13_7000, "CLK_ROUTE0"),
            (0x13_7100, "CLK_SRC_SEL"),
            (0x13_7250, "CLK_ROUTE_x"),
            (0x13_2800, "CLK_DOM_HUB"),
        ];
        for (reg, name) in &pclock_samples {
            let val = r(*reg);
            tracing::info!(
                reg = format_args!("{reg:#010x}"),
                val = format_args!("{val:#010x}"),
                name,
                "Phase 1.5: PCLOCK register probe"
            );
        }

        let pclock_write_test = 0xCAFE_0001_u32;
        w(0x13_0000, pclock_write_test);
        let pc_rb = r(0x13_0000);
        w(0x13_0000, 0);
        tracing::info!(
            wrote = format_args!("{pclock_write_test:#010x}"),
            readback = format_args!("{pc_rb:#010x}"),
            writable = (pc_rb == pclock_write_test),
            "Phase 1.5: PCLOCK write test (pre-DEVINIT, minimal PMC)"
        );

        let pmu_cpuctl = r(0x10_a100);
        let pmu_engctl = r(0x10_a200);
        let pmu_mutex0 = r(0x10_a580);
        let pmu_mutex1 = r(0x10_a584);
        let pmu_intr = r(0x10_a008);
        tracing::info!(
            pmu_cpuctl = format_args!("{pmu_cpuctl:#010x}"),
            pmu_engctl = format_args!("{pmu_engctl:#010x}"),
            pmu_mutex0 = format_args!("{pmu_mutex0:#010x}"),
            pmu_mutex1 = format_args!("{pmu_mutex1:#010x}"),
            pmu_intr = format_args!("{pmu_intr:#010x}"),
            pmu_running = (pmu_cpuctl & 0x20 != 0),
            "Phase 1.5: PMU state (clock management owner)"
        );
    }

    // ── Phase 2: VBIOS DEVINIT first ──
    // Run DEVINIT scripts BEFORE the clock recipe. DEVINIT performs
    // hardware-level initialization (memory controller, PTHERM, GPIO, etc.)
    // in the correct power-up order defined by the VBIOS firmware.
    super::vbios_devinit::kepler_vbios_devinit(bar0);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let pll0_after_devinit = r(0x13_0000);
    let boot0_check = r(0x000);
    tracing::info!(
        pll0 = format_args!("{pll0_after_devinit:#010x}"),
        boot0 = format_args!("{boot0_check:#010x}"),
        "Phase 2: VBIOS DEVINIT completed"
    );

    if let Err(e) = guard.check_canary() {
        tracing::error!("GPU died during Phase 2 DEVINIT: {e}");
        return;
    }

    // ── Phase 3: Transition to full PMC_ENABLE ──
    // Enable all engine clock domains so their PRI ring stations respond.
    w(PMC_ENABLE, NV470_PMC_ENABLE);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let phase3_ok = super::pri::vbios_pri_ring_init(&r, &w);
    let hub2 = r(0x12_0070);
    let gpc0_cpuctl_ph3 = r(0x50_2100);
    let gpc0_ver_ph3 = r(0x50_2004);
    let gpc0_root_ph3 = r(0x50_0000);
    let pg_stat_ph3 = r(0x02_0008);
    let pg_elpg_ph3 = r(0x02_0000);
    let pg_ctrl_ph3 = r(0x02_0004);
    tracing::info!(
        ok = phase3_ok,
        hub = hub2,
        fecs = format_args!("{:#010x}", r(0x40_9100)),
        gpc0_cpuctl = format_args!("{gpc0_cpuctl_ph3:#010x}"),
        gpc0_ver = format_args!("{gpc0_ver_ph3:#010x}"),
        gpc0_root = format_args!("{gpc0_root_ph3:#010x}"),
        pg_stat = format_args!("{pg_stat_ph3:#010x}"),
        pg_elpg = format_args!("{pg_elpg_ph3:#010x}"),
        pg_ctrl = format_args!("{pg_ctrl_ph3:#010x}"),
        "Phase 3: PRI ring re-init + GPC probe (pre-PGOB)"
    );

    // PMC secondary control
    w(0x640, 0xFEBF_B1E1);
    std::thread::sleep(std::time::Duration::from_millis(10));

    // ── Phase 3.25: Clock tree probe + nouveau 0x137xxx path ──
    //
    // On cold VFIO K80, the nvidia-470 0x130xxx PCLOCK PLLs are in a
    // power-gated domain and unwritable. Nouveau uses a completely different
    // clock tree at 0x137xxx that routes through crystal dividers and
    // optional PLLs. This path works on cold hardware because 0x137xxx
    // registers are in the always-on PRI domain.
    {
        // Test which clock paths are writable
        let (pll_w, div_w, out_w) =
            super::kepler_nouveau_clk::test_137xxx_writability(&r, &w);

        // Also test 0x130xxx (nvidia-470 path) for comparison
        let _pll0_pre = r(0x13_0000);
        w(0x13_0000, 0x8000_0101);
        let pll0_rb = r(0x13_0000);
        w(0x13_0000, 0);
        let nv470_writable = pll0_rb != 0;

        tracing::info!(
            nouveau_137_pll = pll_w,
            nouveau_137_div = div_w,
            nouveau_137_out = out_w,
            nv470_130_pll = nv470_writable,
            "Phase 3.25: Clock path writability test"
        );

        // Dump current 0x137xxx state for diagnosis
        super::kepler_nouveau_clk::nouveau_clock_diagnostic(&r);

        if div_w || pll_w {
            // Nouveau clock path is writable — program crystal clocks first
            // for a minimal viable clock (27/108 MHz) to all engine domains.
            super::kepler_nouveau_clk::program_crystal_clocks(&r, &w);
            std::thread::sleep(std::time::Duration::from_millis(20));

            // Now try PLL-based clocks for higher frequency (405 MHz target)
            super::kepler_nouveau_clk::program_engine_plls(&r, &w);
            std::thread::sleep(std::time::Duration::from_millis(20));

            // Verify GPC state after nouveau clocks
            let gpc0_ver = r(0x50_2004);
            let gpc0_cpuctl = r(0x50_2100);
            tracing::info!(
                gpc0_ver = format_args!("{gpc0_ver:#010x}"),
                gpc0_cpuctl = format_args!("{gpc0_cpuctl:#010x}"),
                gpc_alive = gpc0_ver != 0 && gpc0_ver != 0xDEAD_DEAD
                    && gpc0_ver & 0xBAD0_0000 != 0xBAD0_0000,
                "Phase 3.25: GPC probe after nouveau clock programming"
            );
        } else {
            tracing::warn!("Phase 3.25: Neither 0x137xxx nor 0x130xxx writable — GPCs may lack clocks");
        }
    }

    // ── Phase 3.5: Apply nvidia-470 clock recipe (0x130xxx + 0x137xxx) ──
    // The recipe includes both 0x130xxx PLLs (may be dropped on cold K80)
    // and 0x137xxx routing (should succeed and reinforce the above).
    let (applied, skipped) = super::kepler_clock::apply_gk110_clock_recipe(&guard);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let pll0_final = r(0x13_0000);
    tracing::info!(
        applied,
        skipped,
        pll0 = format_args!("{pll0_final:#010x}"),
        "Phase 3.5: PCLOCK recipe applied (post-PMC)"
    );

    // Clear any PRI ring faults accumulated during init
    super::pri::clear_pri_ring_faults(bar0, &r, &w);

    if let Err(e) = guard.check_canary() {
        tracing::error!("GPU died during Phase 3/3.5 PMC+clock: {e}");
        return;
    }

    // ── Phase 3.75: PMU firmware boot ──
    //
    // The PMU (PDAEMON) manages GPU power domains. The firmware handles
    // PGOB power state transitions and clock sequencer coordination.
    // Boot it before PGOB so power gate commands are properly processed.
    {
        tracing::info!("Phase 3.75: PMU firmware boot");
        let pmu_ok = super::pmu::gk110_pmu_boot(&guard);

        let gpc0_ver = r(0x50_2004);
        let gpc0_cpuctl = r(0x50_2100);
        tracing::info!(
            pmu_ok,
            gpc0_ver = format_args!("{gpc0_ver:#010x}"),
            gpc0_cpuctl = format_args!("{gpc0_cpuctl:#010x}"),
            gpc_alive = gpc0_ver != 0 && gpc0_ver != 0xDEAD_DEAD
                && gpc0_ver & 0xBAD0_0000 != 0xBAD0_0000,
            "Phase 3.75: GPC probe after PMU boot (pre-PGOB)"
        );

        super::pri::clear_pri_ring_faults(bar0, &r, &w);
    }

    if let Err(e) = guard.check_canary() {
        tracing::error!("GPU died during Phase 3.75 PMU boot: {e}");
        return;
    }

    // ── Phase 4: PGOB + PGRAPH reset + MMIO init + FECS boot ──
    //
    // On GK210B, the GR HUB auto-clock-gates within nanoseconds of the
    // last PRI access. All GR-related steps must execute in a single tight
    // burst with no logging pauses between them:
    //
    //   1. PGOB disable (ungate GPCs) — includes PMC PGRAPH toggle
    //   2. PRI ring re-init + fault clear
    //   3. GR MMIO init (577 registers)
    //   4. sw_nonctx.bin overrides
    //   5. Clock gating disable (BLCG/SLCG)
    //   6. FECS/GPCCS firmware upload + boot
    //
    // gk110_pgob_disable's internal PMC bit 12 toggle serves as the
    // PGRAPH reset — no separate Phase 4 toggle needed.
    tracing::info!("Phase 4: PGOB + GR init + FECS boot (tight burst — no gaps)");

    // Try GK110 PGOB disable first (matches GK210B lineage)
    super::pgob::gk110_pgob_disable(&guard);

    // Also try GK104 PGOB variant (uses PG_CTRL directly)
    super::pgob::gk104_pgob_disable(&guard);

    // Also try nvidia470 PSW-only variant
    super::pgob::nvidia470_pgob_disable(&guard);

    // ── ELPG disable ──
    //
    // The GK110 PGOB sequence ungates power domains (0x0205xx) but does NOT
    // disable ELPG (Engine Level Power Gating). ELPG auto-gates GPCs when
    // idle. Without a running PMU to manage ELPG wake, the GPCs remain
    // power-gated despite PGOB reporting success.
    //
    // GK104 nouveau explicitly writes PG_CTRL (0x020004) bit 30 = ELPG_DIS.
    // We do the same here, plus clear PG_ELPG (0x020000) to disable all
    // automatic power gating.
    {
        let bar0_inner = guard.inner();
        let rd_raw = |reg: u32| -> u32 { bar0_inner.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
        let wr_raw = |reg: u32, val: u32| { let _ = bar0_inner.write_u32(reg as usize, val); };

        let elpg_pre  = rd_raw(0x02_0000);
        let ctrl_pre  = rd_raw(0x02_0004);
        let stat_pre  = rd_raw(0x02_0008);

        // Disable all ELPG/PGOB auto-gating
        wr_raw(0x02_0000, 0x0000_0000);              // PG_ELPG: clear all
        let ctrl_cur = rd_raw(0x02_0004);
        wr_raw(0x02_0004, (ctrl_cur & !0xC000_0000) | 0x4000_0000); // PG_CTRL: set ELPG_DIS, clear PGOB

        std::thread::sleep(std::time::Duration::from_millis(50));

        let elpg_post = rd_raw(0x02_0000);
        let ctrl_post = rd_raw(0x02_0004);
        let stat_post = rd_raw(0x02_0008);
        let gpc0_test = rd_raw(0x50_2004); // GPCCS falcon version register

        tracing::info!(
            elpg  = format_args!("{elpg_pre:#010x} → {elpg_post:#010x}"),
            ctrl  = format_args!("{ctrl_pre:#010x} → {ctrl_post:#010x}"),
            stat  = format_args!("{stat_pre:#010x} → {stat_post:#010x}"),
            gpc0_falcon_ver = format_args!("{gpc0_test:#010x}"),
            gpc0_alive = gpc0_test != 0 && gpc0_test != 0xDEAD_DEAD && gpc0_test & 0xBAD0_0000 != 0xBAD0_0000,
            "ELPG disable — GPC power state"
        );
    }

    // ── Extra PMC PGRAPH reset ──
    //
    // The PMC bit 12 toggle inside gk110_pgob_disable ungates the GPC power
    // domains and re-enables PGRAPH, but GPC falcons don't enter HRESET
    // because the power domains are still settling at that instant. FECS
    // (in the GR HUB domain) gets HRESET correctly but GPCCS doesn't.
    //
    // With BLCG already at 0 (set by pgob.rs post-Step-7) and GPCs fully
    // powered, a clean PMC PGRAPH reset cycle propagates HRESET to all
    // falcons including per-GPC GPCCS.
    {
        let pmc = r(PMC_ENABLE);
        w(PMC_ENABLE, pmc & !PMC_PGRAPH);
        let _ = r(PMC_ENABLE);
        std::thread::sleep(std::time::Duration::from_millis(50));
        w(PMC_ENABLE, pmc | PMC_PGRAPH);
        let _ = r(PMC_ENABLE);
        std::thread::sleep(std::time::Duration::from_millis(20));

        // PMC reset clears BLCG/SLCG — re-disable immediately
        let bar0_inner = guard.inner();
        let wr_raw = |reg: u32, val: u32| { let _ = bar0_inner.write_u32(reg as usize, val); };
        wr_raw(0x40_41f0, 0); // HUB BLCG
        wr_raw(0x40_41f4, 0); // HUB SLCG
        wr_raw(0x40_9890, 0); // FECS BLCG
        wr_raw(0x40_98b0, 0); // FECS SLCG
        wr_raw(0x40_0500, 0); // TRAP_EN

        let gpccs_cpuctl = r(0x50_2100);
        let fecs_cpuctl = r(kepler_falcon::FECS_BASE + 0x100);
        tracing::info!(
            gpccs_cpuctl = format_args!("{gpccs_cpuctl:#010x}"),
            fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
            gpccs_hreset = gpccs_cpuctl & 0x10 != 0,
            fecs_hreset = fecs_cpuctl & 0x10 != 0,
            "Extra PGRAPH reset — GPCCS HRESET propagation check"
        );
    }

    // PRI ring re-init after PGRAPH reset
    let _ = super::pri::vbios_pri_ring_init(&r, &w);
    super::pri::clear_pri_ring_faults(bar0, &r, &w);

    // Post-PGOB: re-apply nouveau clocks. PGOB may have ungated GPC power
    // domains, but they still need clock signals. The 0x137xxx clock tree
    // can provide clocks even if 0x130xxx PLLs remain gated.
    {
        let gpc0_pre = r(0x50_2004);
        if gpc0_pre == 0 || gpc0_pre & 0xBAD0_0000 == 0xBAD0_0000 {
            tracing::info!("Post-PGOB: GPCs still dead, re-applying nouveau clocks");
            super::kepler_nouveau_clk::program_crystal_clocks(&r, &w);
            std::thread::sleep(std::time::Duration::from_millis(20));
            super::kepler_nouveau_clk::program_engine_plls(&r, &w);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    super::pri::clear_pri_ring_faults(bar0, &r, &w);

    // Verify GPCCS and FECS are accessible
    let gpccs_cpuctl = r(kepler_falcon::GPCCS_BASE + 0x100);
    let fecs_cpuctl = r(kepler_falcon::FECS_BASE + 0x100);
    let gr_hub = r(0x40_0700);
    let gpccs_ok = gpccs_cpuctl & 0xBAD0_0000 != 0xBAD0_0000 && gpccs_cpuctl != 0xDEAD_DEAD;
    let fecs_ok = fecs_cpuctl & 0xBAD0_0000 != 0xBAD0_0000 && fecs_cpuctl != 0xDEAD_DEAD;
    let gr_hub_ok = gr_hub & 0xBAD0_0000 != 0xBAD0_0000 && gr_hub != 0xDEAD_DEAD;
    tracing::info!(
        gpccs_cpuctl = format_args!("{gpccs_cpuctl:#010x}"),
        fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
        gr_hub = format_args!("{gr_hub:#010x}"),
        gpccs_ok, fecs_ok, gr_hub_ok,
        "Post-PGOB+reset+clocks accessibility check"
    );

    if !fecs_ok {
        tracing::warn!("FECS inaccessible after PGOB+reset — aborting GR init");
        return;
    }

    // Clock gating COLD init — zero ALL BLCG/SLCG including per-GPC.
    // Must happen before GR MMIO init so auto-gating doesn't kill
    // the GR HUB mid-init.
    let (cg_applied, cg_faulted) = super::kepler_gr_init::apply_gk110_clkgate_cold(&guard);
    tracing::info!(cg_applied, cg_faulted, "Clock gating COLD init (all disabled)");

    // GR MMIO init — must happen while GR HUB is alive
    let (gr_applied, gr_faulted) = super::kepler_gr_init::apply_gk110_gr_init(&guard);
    tracing::info!(gr_applied, gr_faulted, "GR MMIO init");

    // sw_nonctx.bin overrides
    let (nonctx_applied, nonctx_skipped) = super::pri::apply_sw_nonctx(&guard, "gk210");
    tracing::info!(nonctx_applied, nonctx_skipped, "sw_nonctx.bin overrides");

    // Re-apply clock gating cold init — GR MMIO init may have written
    // BLCG/SLCG values that re-enable gating.
    let (cg2_applied, cg2_faulted) = super::kepler_gr_init::apply_gk110_clkgate_cold(&guard);
    tracing::info!(cg2_applied, cg2_faulted, "Clock gating COLD re-apply (post MMIO)");

    super::pri::clear_pri_ring_faults(bar0, &r, &w);

    // Verify GR HUB alive and falcon HRESET state
    let gr_hub_post = r(0x40_0700);
    let fecs_post = r(kepler_falcon::FECS_BASE + 0x100);
    let gpc0_post = r(0x50_2100);
    tracing::info!(
        gr_hub = format_args!("{gr_hub_post:#010x}"),
        fecs_cpuctl = format_args!("{fecs_post:#010x}"),
        gpc0_cpuctl = format_args!("{gpc0_post:#010x}"),
        gr_hub_ok = gr_hub_post & 0xBAD0_0000 != 0xBAD0_0000 && gr_hub_post != 0xDEAD_DEAD,
        fecs_hreset = fecs_post & 0x10 != 0,
        gpccs_hreset = gpc0_post & 0x10 != 0,
        "Pre-upload state (GR HUB + falcon HRESET)"
    );

    // ── GPCCS falcon deep diagnostic ──
    //
    // GPCCS CPUCTL stubbornly reads 0x00 instead of 0x10 (HRESET).
    // Probe the per-GPC GPCCS registers to understand if the falcon
    // hardware is present, writable, and what state it's actually in.
    {
        let bar0_inner = guard.inner();
        let rd = |off: u32| -> u32 { bar0_inner.read_u32(off as usize).unwrap_or(0xDEAD_DEAD) };
        let wr_raw = |off: u32, val: u32| { let _ = bar0_inner.write_u32(off as usize, val); };

        let gpc0 = 0x50_2000u32;
        let falcon_id  = rd(gpc0 + 0x000);
        let falcon_ver = rd(gpc0 + 0x004);
        let irqstat    = rd(gpc0 + 0x008);
        let irqmask    = rd(gpc0 + 0x010);
        let itfen      = rd(gpc0 + 0x048);
        let exci       = rd(gpc0 + 0x04C);
        let cpuctl     = rd(gpc0 + 0x100);
        let bootvec    = rd(gpc0 + 0x104);
        let hwcfg      = rd(gpc0 + 0x108);
        let dmactl     = rd(gpc0 + 0x10C);
        let engctl     = rd(gpc0 + 0x3C0);
        let sctl       = rd(gpc0 + 0x240);

        // Write test: try BOOTVEC (should be writable)
        wr_raw(gpc0 + 0x104, 0xDEAD_0000);
        let bootvec_wb = rd(gpc0 + 0x104);
        wr_raw(gpc0 + 0x104, 0); // restore
        // Write test: ITFEN
        wr_raw(gpc0 + 0x048, 0x03);
        let itfen_wb = rd(gpc0 + 0x048);
        wr_raw(gpc0 + 0x048, 0);

        tracing::info!(
            falcon_id  = format_args!("{falcon_id:#010x}"),
            falcon_ver = format_args!("{falcon_ver:#010x}"),
            irqstat    = format_args!("{irqstat:#010x}"),
            irqmask    = format_args!("{irqmask:#010x}"),
            itfen      = format_args!("{itfen:#010x}"),
            exci       = format_args!("{exci:#010x}"),
            cpuctl     = format_args!("{cpuctl:#010x}"),
            bootvec    = format_args!("{bootvec:#010x}"),
            hwcfg      = format_args!("{hwcfg:#010x}"),
            dmactl     = format_args!("{dmactl:#010x}"),
            "GPC0 GPCCS falcon register dump"
        );
        tracing::info!(
            engctl         = format_args!("{engctl:#010x}"),
            sctl           = format_args!("{sctl:#010x}"),
            bootvec_write  = format_args!("{bootvec_wb:#010x}"),
            bootvec_ok     = bootvec_wb == 0xDEAD_0000,
            itfen_write    = format_args!("{itfen_wb:#010x}"),
            itfen_ok       = itfen_wb == 0x03,
            "GPC0 GPCCS write test + control regs"
        );

        // ENGCTL hard reset attempt on per-GPC GPCCS (now with BLCG=0)
        wr_raw(gpc0 + 0x3C0, 0x02);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl_after_engctl = rd(gpc0 + 0x100);
        tracing::info!(
            cpuctl_after = format_args!("{cpuctl_after_engctl:#010x}"),
            hreset = cpuctl_after_engctl & 0x10 != 0,
            "GPC0 GPCCS after ENGCTL hard reset (0x3C0=0x02)"
        );

        // PIO DMEM test after ENGCTL reset
        wr_raw(gpc0 + 0x1C0, (1 << 24) | (1 << 30)); // DMEMC: write, addr=0
        wr_raw(gpc0 + 0x1C4, 0xCAFE_BEEF);            // DMEMD: write data
        wr_raw(gpc0 + 0x1C0, 1 << 25);                // DMEMC: read, addr=0
        let dmem_rb = rd(gpc0 + 0x1C4);               // DMEMD: read data
        tracing::info!(
            dmem_rb   = format_args!("{dmem_rb:#010x}"),
            pio_works = dmem_rb == 0xCAFE_BEEF,
            "GPC0 GPCCS PIO DMEM test after ENGCTL reset"
        );

        // Check: are ALL GPC0 registers returning 0? (power domain issue)
        let gpc0_base = 0x50_0000u32;
        let gpc0_tpc0  = rd(gpc0_base + 0x4000);
        let gpc0_mmu   = rd(gpc0_base + 0x0880);
        let gpc0_unk   = rd(gpc0_base + 0x0000);
        tracing::info!(
            gpc0_root  = format_args!("{gpc0_unk:#010x}"),
            gpc0_mmu   = format_args!("{gpc0_mmu:#010x}"),
            gpc0_tpc0  = format_args!("{gpc0_tpc0:#010x}"),
            "GPC0 non-GPCCS register check (power domain alive?)"
        );
    }

    // MC_UNK260=0 (disable GR method dispatch for firmware upload)
    w(0x260, 0);

    // Firmware upload + boot — goes directly, no gr_precursor
    super::kepler_fecs_boot::kepler_load_and_boot_fecs_direct(&guard);
}
