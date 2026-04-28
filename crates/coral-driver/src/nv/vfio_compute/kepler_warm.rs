// SPDX-License-Identifier: AGPL-3.0-or-later

//! Warm Kepler GR initialization — IMEM-preserved restart or cold boot fallback.

/// Warm Kepler GR initialization: try IMEM-preserved restart, else cold boot.
///
/// After nouveau POST + vfio-pci rebind, PLLs and PRI ring are alive.
/// On kernel 6.17, nouveau does NOT initialize GR for GK110B — FECS is
/// left in HRESET with empty IMEM. The warm restart path detects this and
/// falls back to `kepler_load_and_boot_fecs` which uploads the correct
/// GK110 desktop firmware (extracted from nouveau.ko's internal blobs).
///
/// If FECS IMEM does contain firmware (e.g. a future kernel initializes GR),
/// the warm path attempts a fast restart first.
pub(crate) fn kepler_warm_gr_init(guard: &super::hardware_guard::GuardedBar<'_>) {
    use crate::nv::kepler_falcon;

    const FECS: u32 = kepler_falcon::FECS_BASE;
    const GPCCS: u32 = kepler_falcon::GPCCS_BASE;
    const ENGCTL_OFF: u32 = 0x3C0;
    const CPUCTL_OFF: u32 = 0x100;
    const IRQSCLR_OFF: u32 = 0x004;

    let r = guard.read_fn();
    let w = |reg: u32, val: u32| {
        if let Err(refusal) = guard.write_u32(reg, val) {
            tracing::error!(%refusal, "kepler_warm_gr_init: hardware guard refused write");
        }
    };

    // Cross-check: read GPC0 via sysfs resource0 to verify if GPCs are truly
    // alive independently of the VFIO BAR mapping.
    {
        let vfio_gpc0 = r(0x50_2608);
        let vfio_pmc = r(0x200);
        let sysfs_result = super::pri::sysfs_bar0_read_gpc0();
        tracing::info!(
            vfio_pmc = format_args!("{vfio_pmc:#010x}"),
            vfio_gpc0 = format_args!("{vfio_gpc0:#010x}"),
            sysfs_gpc0 = format_args!("{:#010x}", sysfs_result.unwrap_or(0xDEAD_DEAD)),
            "GPC0 cross-check: VFIO BAR vs sysfs resource0"
        );
    }

    // PRI ring faults accumulate during nouveau unbind + vfio-pci rebind.
    // Must clear them before any GR/FECS register access or reads return 0xbadfXXXX.
    super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

    let pri_status = r(0x120058);
    if pri_status != 0 {
        tracing::warn!(
            pri_status = format_args!("{pri_status:#010x}"),
            "PRI ring faults persist — re-initializing ring master"
        );
        let _ = super::pri::vbios_pri_ring_init(&r, &w);
        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
    }

    let pmc = r(0x200);
    let pll0 = r(0x13_0000);
    let fecs_cpuctl = r(FECS + CPUCTL_OFF);
    let gpccs_cpuctl = r(GPCCS + CPUCTL_OFF);
    let fecs_engctl = r(FECS + ENGCTL_OFF);
    let gpccs_engctl = r(GPCCS + ENGCTL_OFF);

    tracing::info!(
        pmc = format_args!("{pmc:#010x}"),
        pll0 = format_args!("{pll0:#010x}"),
        fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
        gpccs_cpuctl = format_args!("{gpccs_cpuctl:#010x}"),
        fecs_engctl = format_args!("{fecs_engctl:#010x}"),
        gpccs_engctl = format_args!("{gpccs_engctl:#010x}"),
        "Kepler warm GR init: preserving firmware in IMEM"
    );

    // ── POST-done fast path ──
    // When Nouveau (or VBIOS) has already POST'd the GPU, the clock tree,
    // PRI ring, PMC engines, and PGRAPH are fully configured. Re-initializing
    // any of these destroys the working state. Instead, skip straight to
    // FECS firmware upload.
    //
    // Detect: DEVINIT POST bit (0x2240c bit 1), PGRAPH in PMC (bit 12),
    // and at least one GPC accessible (not PRI fault).
    let devinit_post = r(0x02_240C);
    let post_done = devinit_post & 0x2 != 0;
    let pgraph_on = pmc & 0x0000_1000 != 0;
    let gpc0_probe = r(0x50_2608);
    let gpc0_ok = gpc0_probe != 0xDEAD_DEAD && gpc0_probe & 0xBAD0_0000 != 0xBAD0_0000;

    tracing::info!(
        devinit_post = format_args!("{devinit_post:#010x}"),
        post_done,
        pgraph_on,
        gpc0 = format_args!("{gpc0_probe:#010x}"),
        gpc0_ok,
        "POST-done fast path check"
    );

    if post_done && pgraph_on && gpc0_ok {
        tracing::info!("POST already done — direct firmware overwrite (no ENGCTL/PMC/PGOB)");

        // Scan GPC/TPC topology from live hardware.
        let mut cached_tpc_counts: [(u32, u32); 8] = [(0, 0); 8];
        let mut cached_gpc_count = 0u32;
        let mut cached_tpc_total = 0u32;
        for gpc in 0..8u32 {
            let tpc_reg = r(0x50_0000 + gpc * 0x8000 + 0x2608);
            if tpc_reg != 0xDEAD_DEAD && tpc_reg & 0xBAD0_0000 != 0xBAD0_0000 {
                let tpc_cnt = tpc_reg & 0xFF;
                cached_tpc_counts[gpc as usize] = (gpc, tpc_cnt);
                cached_gpc_count += 1;
                cached_tpc_total += tpc_cnt;
            }
        }
        tracing::info!(
            gpcs = cached_gpc_count,
            tpcs = cached_tpc_total,
            "POST-done: topology scan"
        );

        // Nouveau left FECS in "software halt" (HALT instruction in its
        // firmware, CPUCTL=0x10).  STARTCPU works from software halt.
        //
        // CRITICAL: Do NOT touch ENGCTL, PMC bit 12, or PGOB.
        //  - ENGCTL cycle puts FECS into "hardware reset halt" where
        //    STARTCPU is silently ignored (Falcon v3 behaviour).
        //  - PMC PGRAPH reset (bit 12 toggle) destroys GR HUB PRI routing
        //    and with PMU halted, PGOB disable can't recover GPC stations.
        //  - PGOB disable does an internal PMC toggle that wipes IMEM/DMEM.
        //
        // Instead: overwrite FECS/GPCCS IMEM+DMEM via PIO (which works
        // independently of CPU state) and STARTCPU directly.
        super::fecs_boot::kepler_post_done_boot_fecs(
            guard,
            cached_gpc_count,
            cached_tpc_total,
            &cached_tpc_counts,
        );
        return;
    }

    // Earliest possible diagnostic — BEFORE any init code runs.
    // Detects whether VFIO bus reset wiped GPU state (PMC collapses from
    // ~0xfc37b1ef to ~0xc0002020 after secondary bus reset).
    let pll0_early = r(0x13_0000);
    let pll_coef_early = r(0x13_0004);
    let ptimer_early = r(0x9400);
    let pri_ring_gpc_cnt = r(0x12_0074);
    let gpc0_early = r(0x50_2608);
    {
        let gpc1 = r(0x50_8000);
        let gpc_bcast = r(0x41_8000);
        let pmu_pgob = r(0x10_a78c);
        let pri_ring_intr = r(0x12_0058);
        tracing::info!(
            pmc = format_args!("{pmc:#010x}"),
            pll0 = format_args!("{pll0_early:#010x}"),
            pll_coef = format_args!("{pll_coef_early:#010x}"),
            ptimer = format_args!("{ptimer_early:#010x}"),
            gpc0 = format_args!("{gpc0_early:#010x}"),
            gpc1 = format_args!("{gpc1:#010x}"),
            gpc_bcast = format_args!("{gpc_bcast:#010x}"),
            pmu_pgob = format_args!("{pmu_pgob:#010x}"),
            pri_ring_intr = format_args!("{pri_ring_intr:#010x}"),
            pri_gpc_cnt = format_args!("{pri_ring_gpc_cnt:#010x}"),
            "EARLIEST diagnostic (pre-init)"
        );

        // GPC power management diagnostic — understand WHY GPCs are 0xbadf1100.
        let ppwr_gate_sts0 = r(0x02_0840); // PPWR power gate status bank 0
        let ppwr_gate_sts1 = r(0x02_0844); // PPWR power gate status bank 1
        let therm_gate_ctrl = r(0x02_0200); // PTHERM gate control
        let pgraph_pri_be = r(0x40_0134); // GR_PRI_BE_EN
        let gr_fe_pwr = r(0x40_4170); // GR frontend power
        let pmu_pgob_cfg = r(0x10_a78c); // PMU PGOB control
        let blcg_gr = r(0x40_0110); // BLCG GR engine
        tracing::info!(
            ppwr_gate_sts0 = format_args!("{ppwr_gate_sts0:#010x}"),
            ppwr_gate_sts1 = format_args!("{ppwr_gate_sts1:#010x}"),
            therm_gate = format_args!("{therm_gate_ctrl:#010x}"),
            pgraph_pri_be = format_args!("{pgraph_pri_be:#010x}"),
            gr_fe_pwr = format_args!("{gr_fe_pwr:#010x}"),
            pmu_pgob_cfg = format_args!("{pmu_pgob_cfg:#010x}"),
            blcg_gr = format_args!("{blcg_gr:#010x}"),
            "GPC power management diagnostic"
        );
    }

    // Step 0: Log PMU state but leave it RUNNING for warm restart.
    //
    // The PMU firmware manages per-GPC power gating.  FECS communicates
    // with PMU to power-on GPCs when it needs them.  If we halt PMU now,
    // FECS can't request GPC power and crashes accessing 0xbadf1100 GPCs.
    // Only halt PMU in the fallback "full reinit" path where we boot
    // fresh PMU firmware anyway.
    let pmu_was_running;
    {
        const PMU_BASE: u32 = 0x10_A000;
        let pmu_cpuctl = r(PMU_BASE + 0x100);
        pmu_was_running = pmu_cpuctl != 0xDEAD_DEAD
            && pmu_cpuctl & 0xBAD0_0000 != 0xBAD0_0000
            && pmu_cpuctl & 0x20 != 0; // bit 5 = RUNNING (nouveau: nvkm_falcon_v1_enabled)
        tracing::info!(
            pmu_cpuctl = format_args!("{pmu_cpuctl:#010x}"),
            pmu_running = pmu_was_running,
            "PMU falcon state (left running for GPC power management)"
        );

        let gpc0_early_check = r(0x50_2608);
        tracing::info!(
            gpc0 = format_args!("{gpc0_early_check:#010x}"),
            "GPC0 with PMU still running"
        );
    }

    // PGRAPH (bit 12) absent in PMC. Two possible causes:
    // (a) Nouveau orderly shutdown: PMU still running, PLLs configured
    //     but power-gated. Fix: re-enable PMC engines to un-gate domains.
    // (b) True VFIO bus reset (FLR): all state destroyed. Fix: cold recovery.
    //
    // We distinguish by checking PMU state: if PMU falcon is running,
    // this is a nouveau shutdown. PLLs are preserved in hardware and
    // come back when we re-enable their engine domains in PMC.
    if pmc & 0x0000_1000 == 0 {
        if pmu_was_running {
            tracing::info!(
                pmc = format_args!("{pmc:#010x}"),
                pll0 = format_args!("{pll0_early:#010x}"),
                "Nouveau shutdown detected (PMU running, PGRAPH gated) — warm PMC re-enable"
            );

            // Re-enable all engine domains that nvidia-470 uses.
            // This un-gates PCLOCK, PGRAPH, PFIFO, etc. PLLs configured
            // by nouveau POST are preserved in the PLL analog circuitry.
            const NV470_PMC_ENABLE: u32 = 0xe011_312c;
            {
                let bar0 = guard.inner();
                let rd_raw =
                    |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
                let wr_raw = |reg: u32, val: u32| {
                    let _ = bar0.write_u32(reg as usize, val);
                };

                wr_raw(0x200, NV470_PMC_ENABLE);
                rd_raw(0x200);
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Hub station parameters from K80 VBIOS DEVINIT — needed
                // for PRI ring to enumerate PCLOCK and other stations.
                super::pri::write_kepler_hub_station_params(&wr_raw);

                // VBIOS ring init command 0x03 (not 0x04) — discovers
                // all stations including PCLOCK.
                super::pri::vbios_pri_ring_init(
                    &|reg| bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD),
                    &|reg, val| {
                        let _ = bar0.write_u32(reg as usize, val);
                    },
                );
                super::pri::clear_pri_ring_faults(bar0, &rd_raw, &wr_raw);

                let pmc_after = rd_raw(0x200);

                // ── Full clock chain diagnostic ──
                // Chain: Crystal(27MHz) → Ref PLLs(0xe800) → VCO →
                //   PCLOCK Core PLLs(0x130000) → Domain Selectors(0x134000) →
                //   Master Routing(0x137000) → Engine Clocks
                let ref_pll0_ctrl = rd_raw(0xe800);
                let ref_pll0_coef = rd_raw(0xe804);
                let ref_pll1_ctrl = rd_raw(0xe820);
                let ref_pll1_coef = rd_raw(0xe824);
                let pclock_pll0_ctrl = rd_raw(0x13_0000);
                let pclock_pll0_coef = rd_raw(0x13_0004);
                let pclock_pll0_stat = rd_raw(0x13_0014);
                let clk_master = rd_raw(0x13_7000);
                let clk_src_sel = rd_raw(0x13_7100);
                let clk_ref_div0 = rd_raw(0x13_7120);
                let clk_ref_div1 = rd_raw(0x13_7140);
                let clk_out_div0 = rd_raw(0x13_7250);
                let pclock_master = rd_raw(0x13_8000);
                tracing::info!(
                    ref_pll0_ctrl = format_args!("{ref_pll0_ctrl:#010x}"),
                    ref_pll0_coef = format_args!("{ref_pll0_coef:#010x}"),
                    ref_pll1_ctrl = format_args!("{ref_pll1_ctrl:#010x}"),
                    ref_pll1_coef = format_args!("{ref_pll1_coef:#010x}"),
                    "Clock chain: Reference PLLs (crystal → VCO)"
                );
                tracing::info!(
                    pll0_ctrl = format_args!("{pclock_pll0_ctrl:#010x}"),
                    pll0_coef = format_args!("{pclock_pll0_coef:#010x}"),
                    pll0_stat = format_args!("{pclock_pll0_stat:#010x}"),
                    clk_master = format_args!("{clk_master:#010x}"),
                    clk_src_sel = format_args!("{clk_src_sel:#010x}"),
                    pclock_master = format_args!("{pclock_master:#010x}"),
                    "Clock chain: PCLOCK core PLLs + routing"
                );
                tracing::info!(
                    ref_div0 = format_args!("{clk_ref_div0:#010x}"),
                    ref_div1 = format_args!("{clk_ref_div1:#010x}"),
                    out_div0 = format_args!("{clk_out_div0:#010x}"),
                    pmc = format_args!("{pmc_after:#010x}"),
                    "Clock chain: dividers + PMC state"
                );

                // Write-test: can reference PLL accept writes?
                let ref0_orig = ref_pll0_ctrl;
                wr_raw(0xe800, ref0_orig ^ 0x1);
                let ref0_test = rd_raw(0xe800);
                wr_raw(0xe800, ref0_orig);
                let ref_writable = ref0_test == (ref0_orig ^ 0x1);

                // Write-test: can PCLOCK PLL accept writes?
                wr_raw(0x13_0000, 0x0000_0001);
                let pc0_test = rd_raw(0x13_0000);
                wr_raw(0x13_0000, pclock_pll0_ctrl);
                let pclock_writable = pc0_test == 0x0000_0001;

                tracing::info!(
                    ref_writable,
                    pclock_writable,
                    ref0_test = format_args!("{ref0_test:#010x}"),
                    pc0_test = format_args!("{pc0_test:#010x}"),
                    "Write-test: PLL register writability"
                );

                let ref_pll_alive =
                    ref_pll0_ctrl != 0 && ref_pll0_ctrl != 0xDEAD_DEAD && ref_pll0_coef != 0;
                let pll_alive = pclock_pll0_ctrl != 0
                    && pclock_pll0_ctrl != 0xDEAD_DEAD
                    && pclock_pll0_ctrl & 0xBAD0_0000 != 0xBAD0_0000;

                if !ref_pll_alive {
                    tracing::warn!(
                        "Reference PLLs dead — programming from crystal (27 MHz → ~2 GHz VCO)"
                    );
                    // Crystal = 27 MHz. Target VCO ≈ 2 GHz.
                    // M=1, N=74, P=0 → actual = 27000 * 74 / 1 = 1998 MHz.
                    let ref_coef: u32 = 1 | (74 << 8); // M=1, N=74, P=0
                    wr_raw(0xe800, 0);
                    wr_raw(0xe804, ref_coef);
                    wr_raw(0xe800, 1);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    wr_raw(0xe820, 0);
                    wr_raw(0xe824, ref_coef);
                    wr_raw(0xe820, 1);
                    std::thread::sleep(std::time::Duration::from_millis(20));

                    let ref0_post = rd_raw(0xe800);
                    let ref0c_post = rd_raw(0xe804);
                    let ref1_post = rd_raw(0xe820);
                    tracing::info!(
                        ref0_ctrl = format_args!("{ref0_post:#010x}"),
                        ref0_coef = format_args!("{ref0c_post:#010x}"),
                        ref1_ctrl = format_args!("{ref1_post:#010x}"),
                        "After reference PLL programming"
                    );
                }

                // Write 0x137xxx clock routing entries first — on GK104/GK110,
                // routing must be configured before PLLs accept writes.
                for &(reg, val) in super::kepler_clock::gk110_clock_recipe_entries() {
                    if reg >= 0x13_7000 && reg <= 0x13_7FFF {
                        wr_raw(reg, val);
                    }
                }
                // Also write 0x132xxx-0x136xxx domain selectors.
                for &(reg, val) in super::kepler_clock::gk110_clock_recipe_entries() {
                    if reg >= 0x13_2000 && reg <= 0x13_6FFF {
                        wr_raw(reg, val);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(10));

                wr_raw(0x13_0000, 0x0000_0001);
                let pc0_test2 = rd_raw(0x13_0000);
                wr_raw(0x13_0000, 0x0000_0000);
                let pclock_writable_now = pc0_test2 == 0x0000_0001;
                tracing::info!(
                    pclock_writable = pclock_writable_now,
                    readback = format_args!("{pc0_test2:#010x}"),
                    clk_master = format_args!("{:#010x}", rd_raw(0x13_7000)),
                    clk_src = format_args!("{:#010x}", rd_raw(0x13_7100)),
                    "PCLOCK PLL writability after routing + selector writes"
                );

                let gpc0_after = rd_raw(0x50_2608);
                let fecs_after = rd_raw(FECS + CPUCTL_OFF);
                tracing::info!(
                    gpc0 = format_args!("{gpc0_after:#010x}"),
                    fecs = format_args!("{fecs_after:#010x}"),
                    "State after warm PMC re-enable + clock chain init"
                );

                if !pclock_writable_now && !pll_alive {
                    tracing::warn!("PLLs still not writable — booting PMU to un-gate PLL power");
                }
            }

            // If PCLOCK PLLs are still dead after PMC re-enable + master control,
            // the PLL analog power domain is gated. The PMU manages this.
            let pll0_check = r(0x13_0000);
            let pll_alive_pre = pll0_check != 0
                && pll0_check != 0xDEAD_DEAD
                && pll0_check & 0xBAD0_0000 != 0xBAD0_0000;

            if !pll_alive_pre {
                tracing::info!("Booting PMU firmware to un-gate PLL power domains");
                let pmu_ok = super::pmu::gk110_pmu_boot(guard);
                std::thread::sleep(std::time::Duration::from_millis(100));

                let pll0_post_pmu = r(0x13_0000);
                let pll0_coef_post = r(0x13_0004);
                tracing::info!(
                    pmu_ok,
                    pll0 = format_args!("{pll0_post_pmu:#010x}"),
                    pll0_coef = format_args!("{pll0_coef_post:#010x}"),
                    "PLLs after PMU boot"
                );

                let pll_alive_post = pll0_post_pmu != 0
                    && pll0_post_pmu != 0xDEAD_DEAD
                    && pll0_post_pmu & 0xBAD0_0000 != 0xBAD0_0000;

                if pll_alive_post {
                    tracing::info!("PMU boot un-gated PLLs — applying clock recipe");
                    let (clk_applied, clk_skipped) =
                        super::kepler_clock::apply_gk110_clock_recipe(guard);
                    std::thread::sleep(std::time::Duration::from_millis(100));

                    let pll0_final = r(0x13_0000);
                    tracing::info!(
                        clk_applied,
                        clk_skipped,
                        pll0 = format_args!("{pll0_final:#010x}"),
                        "Clock recipe applied after PMU boot"
                    );
                } else {
                    tracing::warn!("PLLs dead after PMU boot — trying direct clock recipe");
                    let (clk_applied, clk_skipped) =
                        super::kepler_clock::apply_gk110_clock_recipe(guard);
                    std::thread::sleep(std::time::Duration::from_millis(100));

                    let pll0_recipe = r(0x13_0000);
                    tracing::info!(
                        clk_applied,
                        clk_skipped,
                        pll0 = format_args!("{pll0_recipe:#010x}"),
                        "Clock recipe applied directly (PLLs may still be dead)"
                    );

                    if pll0_recipe == 0 || pll0_recipe == 0xDEAD_DEAD {
                        tracing::warn!(
                            "All clock init attempts failed — falling back to cold recovery"
                        );
                        super::kepler_recovery::kepler_cold_recovery(guard);
                    }
                }
            } else {
                tracing::info!(
                    pll0 = format_args!("{pll0_check:#010x}"),
                    "PLLs alive after warm PMC re-enable — applying clock recipe"
                );
                let (clk_applied, clk_skipped) =
                    super::kepler_clock::apply_gk110_clock_recipe(guard);
                std::thread::sleep(std::time::Duration::from_millis(100));

                let pll0_final = r(0x13_0000);
                tracing::info!(
                    clk_applied,
                    clk_skipped,
                    pll0 = format_args!("{pll0_final:#010x}"),
                    "Clock recipe applied (warm PLLs)"
                );
            }
        } else {
            tracing::warn!(
                pmc = format_args!("{pmc:#010x}"),
                pll0 = format_args!("{pll0_early:#010x}"),
                "True bus reset detected (PMU not running) — performing cold recovery"
            );
            super::kepler_recovery::kepler_cold_recovery(guard);
        }

        let pmc_after = r(0x00_0200);
        let gpc0_after = r(0x50_2608);
        let fecs_after = r(FECS + CPUCTL_OFF);
        let pll0_after = r(0x13_0000);
        tracing::info!(
            pmc = format_args!("{pmc_after:#010x}"),
            gpc0 = format_args!("{gpc0_after:#010x}"),
            fecs = format_args!("{fecs_after:#010x}"),
            pll0 = format_args!("{pll0_after:#010x}"),
            "State after PMC recovery"
        );
    }

    // Probe active GPC count from live hardware (NOT dead FECS registers).
    // Scan per-GPC TPC count register; PRI faults indicate disabled GPCs.
    let mut hw_gpc_count: u32 = 0;
    for gpc in 0..8u32 {
        let tpc_reg = r(0x50_0000 + gpc * 0x8000 + 0x2608);
        if tpc_reg != 0xDEAD_DEAD && tpc_reg & 0xBAD0_0000 != 0xBAD0_0000 {
            hw_gpc_count += 1;
        }
    }

    // Re-read FECS state (may have changed if cold recovery ran).
    super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
    let fecs_cpuctl = r(FECS + CPUCTL_OFF);
    let mut fecs_dead = fecs_cpuctl == 0xDEAD_DEAD || fecs_cpuctl & 0xBAD0_0000 == 0xBAD0_0000;

    tracing::info!(
        hw_gpc_count,
        fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
        fecs_dead,
        "GPC count + FECS state"
    );

    if fecs_dead && hw_gpc_count == 0 {
        tracing::error!("Both FECS and GPCs unreachable — GPU completely dead");
        return;
    }

    // If FECS is PRI-locked, try clearing the GR interrupt trap BEFORE
    // resorting to a PMC toggle.  On Kepler, a pending GR_INTR (0x400100)
    // gates host PRI access to all PGRAPH sub-units including FECS.
    // Clearing it via blind posted write costs nothing and avoids the
    // destructive PGRAPH reset that wipes GR HUB state.
    if fecs_dead && hw_gpc_count > 0 {
        tracing::info!("FECS PRI-locked — attempting GR trap clear (non-destructive)");

        w(0x40_0100, 0xFFFF_FFFF); // GR_INTR: write-1-to-clear all traps
        w(0x40_0108, 0x0000_0000); // GR_INTR_EN: disable stall interrupts
        w(0x40_0138, 0x0000_0000); // GR_INTR_EN_NONSTALL: disable
        w(0x40_0140, 0x0000_0000); // GR_INTR_EN_2: disable (if exists)
        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        std::thread::sleep(std::time::Duration::from_millis(10));
        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

        let fecs_retry = r(FECS + CPUCTL_OFF);
        let gr_hub_retry = r(0x40_0000);
        let is_ok = |v: u32| v != 0xDEAD_DEAD && v & 0xBAD0_0000 != 0xBAD0_0000;
        tracing::info!(
            fecs_cpuctl = format_args!("{fecs_retry:#010x}"),
            fecs_ok = is_ok(fecs_retry),
            gr_hub = format_args!("{gr_hub_retry:#010x}"),
            gr_hub_ok = is_ok(gr_hub_retry),
            "After GR trap clear"
        );

        if is_ok(fecs_retry) {
            tracing::info!("GR trap clear unlocked FECS — skipping PMC toggle");
            fecs_dead = false;
        }
    }

    if fecs_dead && hw_gpc_count > 0 {
        // GR trap clear didn't unlock FECS — fall back to PMC toggle.
        // This resets all PGRAPH state but is the last resort.
        tracing::warn!(
            hw_gpc_count,
            "GR trap clear failed — PMC toggle (destructive PGRAPH reset)"
        );

        {
            let bar0 = guard.inner();
            let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
            let wr = |reg: u32, val: u32| {
                let _ = bar0.write_u32(reg as usize, val);
            };

            let pmc_cur = rd(0x200);
            wr(0x200, pmc_cur & !0x0000_1000);
            rd(0x200);
            std::thread::sleep(std::time::Duration::from_millis(20));
            wr(0x200, pmc_cur | 0x0000_1000);
            rd(0x200);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        super::pri::vbios_pri_ring_init(&r, &w);
        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

        let fecs_after_reset = r(FECS + CPUCTL_OFF);
        let gpc0_after_reset = r(0x50_2608);
        let gr_hub_after = r(0x40_0000);
        let is_ok = |v: u32| v != 0xDEAD_DEAD && v & 0xBAD0_0000 != 0xBAD0_0000;
        tracing::info!(
            fecs_cpuctl = format_args!("{fecs_after_reset:#010x}"),
            gpc0 = format_args!("{gpc0_after_reset:#010x}"),
            gr_hub = format_args!(
                "{gr_hub_after:#010x}[{}]",
                if is_ok(gr_hub_after) { "OK" } else { "FAULT" }
            ),
            "After PMC toggle + PRI ring re-init"
        );
    }

    // Proceed to firmware upload — FECS may be unlocked (trap-clear
    // or PMC toggle) or we may be in PMC-reset state.  Either way,
    // we need to load and boot FECS/GPCCS.
    if hw_gpc_count > 0 {
        let gpc0_alive = {
            let v = r(0x50_2608);
            v != 0xDEAD_DEAD && v & 0xBAD0_0000 != 0xBAD0_0000
        };

        if !gpc0_alive {
            tracing::warn!("GPCs died — cold recovery path");
            super::kepler_recovery::kepler_cold_recovery(guard);
        }

        // Cache topology before any PMC reset that might clear GPC fuse
        // mirrors.  Defaults are populated via scan_gpc_topology if the
        // PGOB path is skipped.
        let mut cached_tpc_counts: [(u32, u32); 8] = [(0, 0); 8];
        let mut cached_gpc_count = 0u32;
        let mut cached_tpc_total = 0u32;

        // Check GR HUB accessibility. On headless K80, nouveau never
        // initializes GR so the GR HUB power domain is gated even when
        // GPCs are alive. PMU boot + PGOB disable brings it online.
        let gr_hub_test = r(0x40_0000);
        let gr_hub_ok = gr_hub_test != 0xDEAD_DEAD && gr_hub_test & 0xBAD0_0000 != 0xBAD0_0000;

        if !gr_hub_ok {
            tracing::info!(
                gr_hub = format_args!("{gr_hub_test:#010x}"),
                "GR HUB inaccessible — booting PMU + PGOB disable"
            );

            // PMU must be running for PGRAPH power management.
            let pmu_cpuctl = r(0x10_A100);
            let pmu_running = pmu_cpuctl != 0xDEAD_DEAD
                && pmu_cpuctl & 0xBAD0_0000 != 0xBAD0_0000
                && pmu_cpuctl & 0x20 != 0; // bit 5 = CPU running

            tracing::info!(
                pmu_cpuctl = format_args!("{pmu_cpuctl:#010x}"),
                pmu_running,
                "PMU falcon state (left running for GPC power management)"
            );

            if !pmu_running {
                let pmu_ok = super::pmu::gk110_pmu_boot(guard);
                tracing::info!(pmu_ok, "PMU firmware boot (for PGRAPH power domain)");
                std::thread::sleep(std::time::Duration::from_millis(50));
            } else {
                tracing::info!("PMU already running from nouveau — skipping re-boot");
            }

            // Run the full GK110 PGOB disable sequence. This is
            // nouveau's gk110_pmu_pgob() — disables PGRAPH power
            // gating so GR HUB silicon actually clocks.
            super::pgob::gk110_pgob_disable(guard);

            // PGOB brought PGRAPH online. Read GPC/TPC topology NOW
            // before the PMC reset clears the fuse mirrors at 0x502608.
            // nouveau reads these in gf100_gr_oneinit (before mc_reset).
            {
                let bar0 = guard.inner();
                let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
                super::pri::clear_pri_ring_faults(bar0, &r, &w);
                for gpc in 0..8u32 {
                    let tpc_reg = rd(0x50_0000 + gpc * 0x8000 + 0x2608);
                    let alive = tpc_reg != 0xDEAD_DEAD && tpc_reg & 0xBAD0_0000 != 0xBAD0_0000;
                    if alive {
                        let tpc_nr = tpc_reg & 0x1F;
                        cached_tpc_counts[gpc as usize] = (gpc, tpc_nr);
                        cached_gpc_count += 1;
                        cached_tpc_total += tpc_nr;
                    }
                }
                tracing::info!(
                    cached_gpc_count, cached_tpc_total,
                    tpc_per_gpc = ?cached_tpc_counts.iter()
                        .take(cached_gpc_count as usize)
                        .map(|&(g, t)| format!("GPC{g}:{t}T"))
                        .collect::<Vec<_>>(),
                    "Topology cached BEFORE PMC reset"
                );
            }

            // If TPC counts are all zero, GPC fuse mirrors haven't been
            // programmed. This happens when nouveau never initialized GR
            // (headless K80 on kernel 6.17). Run VBIOS DEVINIT to program
            // the fuses, then re-scan topology.
            if cached_tpc_total == 0 && cached_gpc_count > 0 {
                tracing::warn!(
                    cached_gpc_count,
                    "GPCs alive but 0 TPCs — running VBIOS DEVINIT to program fuse mirrors"
                );
                super::vbios_devinit::kepler_vbios_devinit(guard.inner());
                std::thread::sleep(std::time::Duration::from_millis(50));

                // Apply clock recipe so GPC clocks are running.
                let (clk_applied, clk_skipped) =
                    super::kepler_clock::apply_gk110_clock_recipe(guard);
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Re-enumerate PRI ring after DEVINIT.
                {
                    let bar0 = guard.inner();
                    let rd =
                        |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
                    let wr_raw = |reg: u32, val: u32| {
                        let _ = bar0.write_u32(reg as usize, val);
                    };
                    super::pri::nouveau_pri_ring_init(&rd, &wr_raw);
                    super::pri::clear_pri_ring_faults(bar0, &rd, &wr_raw);
                }

                // Re-scan topology after DEVINIT.
                cached_tpc_counts = [(0, 0); 8];
                cached_gpc_count = 0;
                cached_tpc_total = 0;
                {
                    let bar0 = guard.inner();
                    let rd =
                        |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
                    super::pri::clear_pri_ring_faults(bar0, &r, &w);
                    for gpc in 0..8u32 {
                        let tpc_reg = rd(0x50_0000 + gpc * 0x8000 + 0x2608);
                        let alive = tpc_reg != 0xDEAD_DEAD && tpc_reg & 0xBAD0_0000 != 0xBAD0_0000;
                        if alive {
                            let tpc_nr = tpc_reg & 0x1F;
                            cached_tpc_counts[gpc as usize] = (gpc, tpc_nr);
                            cached_gpc_count += 1;
                            cached_tpc_total += tpc_nr;
                        }
                    }
                }

                let gpc_bcast = r(0x41_8000);
                tracing::info!(
                    cached_gpc_count, cached_tpc_total, clk_applied, clk_skipped,
                    gpc_bcast = format_args!("{gpc_bcast:#010x}"),
                    tpc_per_gpc = ?cached_tpc_counts.iter()
                        .take(cached_gpc_count as usize)
                        .map(|&(g, t)| format!("GPC{g}:{t}T"))
                        .collect::<Vec<_>>(),
                    "Topology after DEVINIT + clock recipe"
                );
            }

            // Now perform a CLEAN nvkm_mc_reset(GR) — the kernel does
            // mask-clear, flush, mask-set, flush with no sleeps.
            // This enables GR HUB register writes.
            {
                let bar0 = guard.inner();
                let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
                let wr_raw = |reg: u32, val: u32| {
                    let _ = bar0.write_u32(reg as usize, val);
                };

                let pmc = rd(0x200);
                wr_raw(0x200, pmc & !0x0000_1000); // PGRAPH off
                rd(0x200); // flush
                wr_raw(0x200, pmc | 0x0000_1000); // PGRAPH on
                rd(0x200); // flush

                super::pri::nouveau_pri_ring_init(&rd, &wr_raw);
                super::pri::clear_pri_ring_faults(bar0, &rd, &wr_raw);

                // Verify GR HUB is writable after PMC reset.
                wr_raw(0x40_0080, 0x003083c2);
                let readback = rd(0x40_0080);
                tracing::info!(
                    readback = format_args!("{readback:#010x}"),
                    writable = readback == 0x003083c2,
                    "GR HUB writability after PMC reset (0x400080)"
                );
            }

            // Re-disable PGOB after the PMC GR reset.
            //
            // The PMC PGRAPH toggle (bit 12 of 0x200) resets the entire
            // PGRAPH block, which re-enables PGOB power gating on GK110+.
            // GPCs become inaccessible (0xbadf1100) until PGOB is disabled
            // again. In nouveau, gk110_pmu_pgob() runs AFTER mc_reset(GR),
            // not before. We must match that ordering.
            {
                let gpc0_pre = r(0x50_2608);
                let gr_hub_pre = r(0x40_0000);
                tracing::info!(
                    gpc0_pre_pgob2 = format_args!("{gpc0_pre:#010x}"),
                    gr_hub_pre_pgob2 = format_args!("{gr_hub_pre:#010x}"),
                    "Pre second PGOB disable (post PMC reset)"
                );

                super::pgob::gk110_pgob_disable(guard);

                super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
                let gpc0_post = r(0x50_2608);
                let gr_hub_post = r(0x40_0000);
                let gpc_bcast = r(0x41_8000);
                tracing::info!(
                    gpc0_post_pgob2 = format_args!("{gpc0_post:#010x}"),
                    gr_hub_post_pgob2 = format_args!("{gr_hub_post:#010x}"),
                    gpc_bcast = format_args!("{gpc_bcast:#010x}"),
                    "Post second PGOB disable (post PMC reset)"
                );

                // Re-scan topology — GPCs should now be accessible.
                cached_tpc_counts = [(0, 0); 8];
                cached_gpc_count = 0;
                cached_tpc_total = 0;
                {
                    let bar0 = guard.inner();
                    let rd =
                        |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
                    for gpc in 0..8u32 {
                        let tpc_reg = rd(0x50_0000 + gpc * 0x8000 + 0x2608);
                        let alive = tpc_reg != 0xDEAD_DEAD && tpc_reg & 0xBAD0_0000 != 0xBAD0_0000;
                        if alive {
                            let tpc_nr = tpc_reg & 0x1F;
                            cached_tpc_counts[gpc as usize] = (gpc, tpc_nr);
                            cached_gpc_count += 1;
                            cached_tpc_total += tpc_nr;
                        }
                    }
                }
                tracing::info!(
                    cached_gpc_count, cached_tpc_total,
                    tpc_per_gpc = ?cached_tpc_counts.iter()
                        .take(cached_gpc_count as usize)
                        .map(|&(g, t)| format!("GPC{g}:{t}T"))
                        .collect::<Vec<_>>(),
                    "Topology after second PGOB disable (post PMC reset)"
                );
            }
        } else {
            // GR HUB already accessible (no PGOB needed). Scan topology now.
            let topo = super::pri::scan_gpc_topology(guard);
            cached_gpc_count = topo.0;
            cached_tpc_total = topo.1;
            cached_tpc_counts = topo.2;
        }

        // PGOB disable + PMC GR reset.
        // With PLLs alive from nouveau POST, the PMU is properly clocked
        // and cooperates with the PGOB disable protocol. No need to halt
        // the PMU — it manages power domains correctly when engine clocks
        // are running.
        super::pgob::gk110_pgob_disable(guard);
        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

        {
            let gr_hub_post_pgob = r(0x40_0000);
            let fecs_post_pgob = r(FECS + CPUCTL_OFF);
            tracing::info!(
                gr_hub = format_args!("{gr_hub_post_pgob:#010x}"),
                fecs_cpuctl = format_args!("{fecs_post_pgob:#010x}"),
                "After PGOB disable (PMU running, PLLs alive)"
            );
        }

        // PMC GR reset — proper Falcon initialization.
        {
            let bar0 = guard.inner();
            let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
            let wr_raw = |reg: u32, val: u32| {
                let _ = bar0.write_u32(reg as usize, val);
            };

            let pmc = rd(0x200);
            tracing::info!(pmc = format_args!("{pmc:#010x}"), "PMC GR reset");
            wr_raw(0x200, pmc & !0x0000_1000);
            rd(0x200);
            std::thread::sleep(std::time::Duration::from_millis(20));
            wr_raw(0x200, pmc | 0x0000_1000);
            rd(0x200);
            std::thread::sleep(std::time::Duration::from_millis(50));

            super::pri::nouveau_pri_ring_init(&rd, &wr_raw);
            super::pri::clear_pri_ring_faults(bar0, &rd, &wr_raw);

            let gr_hub_final = rd(0x40_0000);
            let fecs_final = rd(FECS + CPUCTL_OFF);
            let gpc0_final = rd(0x50_2608);
            tracing::info!(
                gr_hub = format_args!("{gr_hub_final:#010x}"),
                fecs_cpuctl = format_args!("{fecs_final:#010x}"),
                gpc0 = format_args!("{gpc0_final:#010x}"),
                "Post-PMC GR reset state"
            );

            if gr_hub_final == 0xDEAD_DEAD || gr_hub_final & 0xBAD0_0000 == 0xBAD0_0000 {
                tracing::warn!("GR HUB still faulted after PMC reset — extra PGOB disable");
                super::pgob::gk110_pgob_disable(guard);
                super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
                let gr_hub_extra = r(0x40_0000);
                tracing::info!(
                    gr_hub = format_args!("{gr_hub_extra:#010x}"),
                    "After extra PGOB disable"
                );
            }
        }

        tracing::info!(hw_gpc_count, "Proceeding to FECS/GPCCS firmware upload");
        super::fecs_boot::kepler_load_and_boot_fecs(
            guard,
            cached_gpc_count,
            cached_tpc_total,
            &cached_tpc_counts,
        );
        return;
    }

    // Phase 1: Force both falcons through hardware HRESET cycle.
    //
    // After nouveau teardown, FECS/GPCCS are in software-HALTED state
    // (firmware executed HALT instruction). Writing STARTCPU to a
    // software-halted falcon does NOT restart it — only hardware reset
    // via ENGCTL can re-enter the HRESET state from which STARTCPU works.
    //
    // ENGCTL=0x01 forces hardware reset (preserves IMEM/DMEM contents).
    // ENGCTL=0x00 releases from reset → falcon enters HRESET state.
    // CPUCTL=STARTCPU then starts execution from BOOTVEC.
    w(GPCCS + ENGCTL_OFF, 0x01);
    w(FECS + ENGCTL_OFF, 0x01);
    std::thread::sleep(std::time::Duration::from_millis(10));
    w(GPCCS + ENGCTL_OFF, 0x00);
    w(FECS + ENGCTL_OFF, 0x00);
    std::thread::sleep(std::time::Duration::from_millis(10));

    let fecs_cpuctl_after = r(FECS + CPUCTL_OFF);
    tracing::info!(
        fecs_cpuctl = format_args!("{fecs_cpuctl_after:#010x}"),
        "Phase 1: ENGCTL HRESET cycle complete"
    );

    // Phase 2: Nouveau pre-FECS init (gf100_gr_init_ctxctl_int).
    // Exact match for nouveau: clear INTR_UP, zero scratch0/scratch1.
    // Do NOT write non-zero values here — FECS firmware interprets
    // scratch1 != 0 as a pending method command during early boot.
    w(FECS + 0x840, 0xFFFF_FFFF); // INTR_UP: write-1-to-clear all pending
    w(FECS + 0x500, 0x0000_0000); // SCRATCH0 = 0
    w(FECS + 0x504, 0x0000_0000); // SCRATCH1 = 0

    // Start GPCCS first (nouveau ordering) — per-GPC unicast, not broadcast.
    for gpc in 0..8u32 {
        let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
        let probe = r(gpccs_base + CPUCTL_OFF);
        if probe != 0xDEAD_DEAD && probe & 0xBAD0_0000 != 0xBAD0_0000 {
            w(gpccs_base + 0x10C, 0x0000_0000);
            w(gpccs_base + 0x104, 0x0000_0000);
            w(gpccs_base + CPUCTL_OFF, 0x0000_0002);
        }
    }
    super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

    // Phase 3: Clear falcon IRQs and STARTCPU for FECS.
    w(FECS + 0x10C, 0x0000_0000); // FECS DMACTL = 0
    w(FECS + IRQSCLR_OFF, 0xFFFF_FFFF);
    w(FECS + CPUCTL_OFF, 0x0000_0002); // STARTCPU (bit 1)

    // Phase 4: Poll FECS_STATUS bit 0 for boot confirmation.
    let mut booted = false;
    for i in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let status = r(kepler_falcon::FECS_STATUS);
        let cpuctl = r(FECS + CPUCTL_OFF);

        if status & 1 != 0 {
            let ctx_size = r(FECS + 0x804);
            tracing::info!(
                poll = i,
                status = format_args!("{status:#010x}"),
                cpuctl = format_args!("{cpuctl:#010x}"),
                ctx_size = format_args!("{ctx_size:#010x}"),
                "FECS boot confirmed (warm restart)"
            );
            booted = true;
            break;
        }

        if cpuctl & 0x10 != 0 && i > 2 {
            let pc = r(FECS + 0x030);
            let exci = r(FECS + 0x148);
            let mb0 = r(FECS + 0x040);
            let scratch0 = r(FECS + 0x500);
            tracing::warn!(
                cpuctl = format_args!("{cpuctl:#010x}"),
                pc = format_args!("{pc:#010x}"),
                exci = format_args!("{exci:#010x}"),
                mb0 = format_args!("{mb0:#010x}"),
                scratch0 = format_args!("{scratch0:#010x}"),
                "FECS HALTED during warm boot — firmware may have trapped"
            );
            break;
        }
    }

    if booted {
        tracing::info!("Kepler FECS warm restart complete — GR engine ready");
    } else {
        let cpuctl = r(FECS + CPUCTL_OFF);
        let status = r(kepler_falcon::FECS_STATUS);
        let pc = r(FECS + 0x030);
        let exci = r(FECS + 0x148);
        tracing::warn!(
            cpuctl = format_args!("{cpuctl:#010x}"),
            status = format_args!("{status:#010x}"),
            pc = format_args!("{pc:#010x}"),
            exci = format_args!("{exci:#010x}"),
            "Kepler FECS warm restart did not complete"
        );

        // Detect whether nouveau's warm state is intact: GPCs accessible,
        // PMC fully enabled, and the FECS topology register readable.
        // If so, skip the destructive PGOB/SCC/MMIO reinit — it kills the
        // warm state that nouveau set up and that our firmware needs.
        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        let gpc0_warmcheck = r(0x50_2608);
        let gpc_nr_reg = r(0x40_9604);
        let gpc0_alive =
            gpc0_warmcheck != 0xDEAD_DEAD && gpc0_warmcheck & 0xBAD0_0000 != 0xBAD0_0000;
        let gpc_nr_readable = gpc_nr_reg != 0xDEAD_DEAD && gpc_nr_reg & 0xBAD0_0000 != 0xBAD0_0000;

        tracing::info!(
            gpc0 = format_args!("{gpc0_warmcheck:#010x}"),
            gpc_nr_reg = format_args!("{gpc_nr_reg:#010x}"),
            gpc0_alive,
            gpc_nr_readable,
            "Warm-state check: can we skip destructive reinit?"
        );

        if gpc0_alive && gpc_nr_readable {
            // ── Fast path: nouveau warm state intact ──
            //
            // nouveau left: PMC fully enabled, PRI ring working, GPCs powered,
            // FECS topology registers populated. We skip destructive steps
            // (PGOB toggle, SCC reset) but MUST apply GR MMIO init — GPCCS
            // firmware reads GPC-internal registers (PROP, ZCULL, TEX, SM)
            // during boot and halts if they're unconfigured.
            tracing::info!("Using nouveau warm-state fast path (skip PGOB/SCC)");

            // Apply GR MMIO init (~173 registers + exceptions).
            let (gr_applied, gr_faulted) = super::kepler_gr_init::apply_gk110_gr_init(guard);
            super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
            tracing::info!(gr_applied, gr_faulted, "GR MMIO init applied (fast path)");

            // Probe disabled GPCs and write the GPC_DISABLE mask.
            let mut gpc_disable_mask: u32 = 0;
            let mut active_gpcs = Vec::new();
            for gpc in 0..8u32 {
                let tpc_reg = r(0x50_0000 + gpc * 0x8000 + 0x2608);
                if tpc_reg != 0xDEAD_DEAD && tpc_reg & 0xBAD0_0000 != 0xBAD0_0000 {
                    let tpc_nr = tpc_reg & 0x1F;
                    active_gpcs.push((gpc, tpc_nr));
                } else {
                    let pri_gpc_count = r(0x12_0074);
                    if gpc < pri_gpc_count {
                        gpc_disable_mask |= 1 << gpc;
                    }
                }
            }
            let tpc_total: u32 = active_gpcs.iter().map(|&(_, t)| t).sum();

            // Write GPC disable mask to 0x40960C so FECS firmware
            // knows which GPC stations to skip during broadcast.
            super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
            w(0x40_960C, gpc_disable_mask);
            tracing::info!(
                gpc_disable_mask = format_args!("{gpc_disable_mask:#010x}"),
                active_gpcs = active_gpcs.len(),
                tpc_total,
                active = ?active_gpcs.iter().map(|&(g, t)| format!("GPC{g}:{t}T")).collect::<Vec<_>>(),
                "GPC disable mask written — FECS will skip dead stations"
            );

            // Write TPC total to GR_FE_TPC_NR (0x409608) — firmware reads this.
            w(0x40_9608, tpc_total);

            let topo = super::pri::scan_gpc_topology(guard);
            super::fecs_boot::kepler_load_and_boot_fecs(guard, topo.0, topo.1, &topo.2);
        } else {
            // ── Slow path: full cold reinit ──
            tracing::warn!("GPCs not alive — full GR reinit with PGOB disable + GPC topology");

            // Boot PMU firmware first — PMU manages power domains.
            let pmu_ok = super::pmu::gk110_pmu_boot(guard);
            tracing::info!(pmu_ok, "PMU firmware boot (for GPC power sequencing)");
            std::thread::sleep(std::time::Duration::from_millis(50));

            // NVIDIA official PGOB disable sequence.
            {
                let bar0 = guard.inner();
                let rd_raw =
                    |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
                let wr_raw = |reg: u32, val: u32| {
                    let _ = bar0.write_u32(reg as usize, val);
                };
                let mask_raw = |reg: u32, clr: u32, set: u32| {
                    let cur = rd_raw(reg);
                    wr_raw(reg, (cur & !clr) | set);
                };

                let pmc_before = rd_raw(0x200);
                let gpc0_before = rd_raw(0x50_2608);
                tracing::info!(
                    pmc = format_args!("{pmc_before:#010x}"),
                    gpc0 = format_args!("{gpc0_before:#010x}"),
                    "PGOB disable: pre-state"
                );

                mask_raw(0x200, 0x0000_1000, 0x0000_0000); // disable PGRAPH
                rd_raw(0x200);
                mask_raw(0x200, 0x0800_0000, 0x0800_0000); // enable BLG
                std::thread::sleep(std::time::Duration::from_millis(50));

                mask_raw(0x10_a78c, 0x0000_0002, 0x0000_0002);
                mask_raw(0x10_a78c, 0x0000_0001, 0x0000_0001);
                mask_raw(0x10_a78c, 0x0000_0001, 0x0000_0000);

                mask_raw(0x20004, 0x8000_0000, 0x0000_0000);
                mask_raw(0x20004, 0x4000_0000, 0x4000_0000);
                std::thread::sleep(std::time::Duration::from_millis(50));

                mask_raw(0x10_a78c, 0x0000_0002, 0x0000_0000);
                mask_raw(0x10_a78c, 0x0000_0001, 0x0000_0001);
                mask_raw(0x10_a78c, 0x0000_0001, 0x0000_0000);

                mask_raw(0x200, 0x0800_0000, 0x0000_0000); // disable BLG
                mask_raw(0x200, 0x0000_1000, 0x0000_1000); // enable PGRAPH
                rd_raw(0x200);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let pmc_after = rd_raw(0x200);
                let gpc0_after = rd_raw(0x50_2608);
                tracing::info!(
                    pmc = format_args!("{pmc_after:#010x}"),
                    gpc0 = format_args!("{gpc0_after:#010x}"),
                    "PGOB disable: post-state"
                );
            }
            super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

            super::pri::vbios_pri_ring_init(&r, &w);
            super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

            w(0x40_0124, 0x0000_0002);

            w(0x40_0500, 0x0001_0001);
            for _ in 0..200 {
                std::thread::sleep(std::time::Duration::from_millis(10));
                let v = r(0x40_0700);
                if v & 0x0000_0002 != 0 {
                    break;
                }
            }
            super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

            super::pri::vbios_pri_ring_init(&r, &w);
            super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

            let (gr_applied, gr_faulted) = super::kepler_gr_init::apply_gk110_gr_init(guard);
            super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

            // GPC topology from live hardware
            {
                let fbp_nr = r(0x12_0078);
                let mut active_gpcs = Vec::new();
                for gpc in 0..8u32 {
                    let tpc_reg = r(0x50_0000 + gpc * 0x8000 + 0x2608);
                    if tpc_reg != 0xDEAD_DEAD && tpc_reg & 0xBAD0_0000 != 0xBAD0_0000 {
                        let tpc_nr = tpc_reg & 0x1F;
                        active_gpcs.push((gpc, tpc_nr));
                    }
                }
                let gpc_nr = active_gpcs.len() as u32;
                let tpc_total: u32 = active_gpcs.iter().map(|&(_, t)| t).sum();

                for &(gpc, tpc_nr) in &active_gpcs {
                    let gpc_base = 0x50_0000 + gpc * 0x8000;
                    w(gpc_base + 0x0914, tpc_nr);
                    w(gpc_base + 0x0910, 0x0004_0000 | tpc_total);
                    w(gpc_base + 0x0918, fbp_nr);
                }
                tracing::info!(
                    gpc_nr, tpc_total, fbp_nr,
                    active = ?active_gpcs.iter().map(|&(g, t)| format!("GPC{g}:{t}T")).collect::<Vec<_>>(),
                    "GPC topology from hardware (per-GPC TPC count)"
                );
            }

            let fecs_cpuctl_post = r(kepler_falcon::FECS_BASE + 0x100);
            let gpc0_verify = r(0x50_2608);
            tracing::info!(
                fecs_cpuctl = format_args!("{fecs_cpuctl_post:#010x}"),
                gpc0 = format_args!("{gpc0_verify:#010x}"),
                gr_applied,
                gr_faulted,
                "Slow path: state after full GR reinit"
            );

            let fecs_pri_fault =
                fecs_cpuctl_post & 0xBAD0_0000 == 0xBAD0_0000 || fecs_cpuctl_post == 0xDEAD_DEAD;
            if fecs_pri_fault {
                tracing::error!("FECS PRI-faulted after reinit — aborting firmware upload");
                return;
            }

            let topo = super::pri::scan_gpc_topology(guard);
            super::fecs_boot::kepler_load_and_boot_fecs(guard, topo.0, topo.1, &topo.2);
        }
    }
}
