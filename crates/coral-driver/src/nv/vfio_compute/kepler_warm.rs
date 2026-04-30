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
        tracing::info!("POST already done — caching topology then PMC GR reset for clean Falcon state");

        // Cache GPC/TPC topology BEFORE PMC reset clears fuse mirrors at 0x502608.
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
            "POST-done: topology cached (before PMC reset)"
        );

        // Check GPCCS falcon CPUCTL at GPC0+0x2100 to detect if GPCs are alive.
        // GPC identity at 0x500000 (GPC+0x0000) always returns 0xbadf1100 PRI
        // timeout on GK210B even when GPCs are fully functional — it's the GPC's
        // PRI slave interface block which is inaccessible. The GPCCS falcon at
        // GPC+0x2000 and functional registers at GPC+0x0400+ work correctly.
        let fecs_itfen_pre = r(FECS + 0x048);
        let gpccs0_cpuctl_pre = r(0x50_2100);
        let gr_hub_pre = r(0x40_0000);
        let gpcs_already_alive = gpccs0_cpuctl_pre != 0xDEAD_DEAD
            && gpccs0_cpuctl_pre & 0xBAD0_0000 != 0xBAD0_0000
            && gpccs0_cpuctl_pre != 0;
        tracing::info!(
            itfen = format_args!("{fecs_itfen_pre:#010x}"),
            gpccs0_cpuctl = format_args!("{gpccs0_cpuctl_pre:#010x}"),
            gr_hub = format_args!("{gr_hub_pre:#010x}"),
            gpcs_already_alive,
            "POST-done: GPC state before reset (GPCCS falcon check)"
        );

        // Always do PMC GR reset to put the GR HUB register file in a clean
        // state. Without this, GR HUB registers return 0xbadf1002 (DECODE_ERROR)
        // from the previous driver session, causing FECS firmware to trap on init.
        // GPCs survive the PMC GR reset on GK210B — the GPCCS falcon at GPC+0x2100
        // remains accessible after re-enabling PGRAPH (bit 12).
        {
            tracing::info!("PMC GR reset for clean GR HUB state (GPCs survive on GK210B)");
            {
                let bar0 = guard.inner();
                let rd_raw =
                    |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
                let wr_raw =
                    |reg: u32, val: u32| { let _ = bar0.write_u32(reg as usize, val); };

                let pmc_cur = rd_raw(0x200);
                wr_raw(0x200, pmc_cur & !0x0000_1000);
                rd_raw(0x200);
                std::thread::sleep(std::time::Duration::from_millis(20));
                wr_raw(0x200, pmc_cur | 0x0000_1000);
                rd_raw(0x200);
                std::thread::sleep(std::time::Duration::from_millis(50));

                super::pri::nouveau_pri_ring_init(&rd_raw, &wr_raw);
                super::pri::clear_pri_ring_faults(bar0, &rd_raw, &wr_raw);

                let fecs_after = rd_raw(FECS + 0x100);
                let gpccs0_after = rd_raw(0x50_2100);
                let gr_status = rd_raw(0x40_0700);
                tracing::info!(
                    pmc = format_args!("{:#010x}", rd_raw(0x200)),
                    fecs_cpuctl = format_args!("{fecs_after:#010x}"),
                    gpccs0_cpuctl = format_args!("{gpccs0_after:#010x}"),
                    gr_status = format_args!("{gr_status:#010x}"),
                    "POST-done: state after PMC GR reset"
                );
            }
        }

        // Check GPC state after reset (whichever path we took).
        {
            super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
            let gpccs0_cpuctl = r(0x50_2100);
            let gr_hub_check = r(0x40_0000);
            let gpcs_alive = gpccs0_cpuctl != 0xDEAD_DEAD
                && gpccs0_cpuctl & 0xBAD0_0000 != 0xBAD0_0000
                && gpccs0_cpuctl != 0;
            let gr_hub_ok = gr_hub_check != 0xDEAD_DEAD
                && gr_hub_check & 0xBAD0_0000 != 0xBAD0_0000;

            tracing::info!(
                gpccs0_cpuctl = format_args!("{gpccs0_cpuctl:#010x}"),
                gr_hub = format_args!("{gr_hub_check:#010x}"),
                gpcs_alive,
                gr_hub_ok,
                "POST-done: GPC/GR HUB state after reset"
            );

            {
                // Nouveau ALWAYS calls nvkm_pmu_pgob(false) after PMC GR reset,
                // regardless of GPC power state. PGOB disable configures internal
                // PGRAPH power domains via BLG (PMC bit 27) + 0x0205xx magic table.
                // Without it, the GR HUB register decoder remains gated, returning
                // 0xbadf1002 (DECODE_ERROR) on all GR HUB register accesses.
                tracing::info!("PGOB disable (Nouveau-aligned: always after PMC GR reset)");

                // Step 1: Apply privring timing (Nouveau calls this during init)
                super::pri::gk104_privring_timing(&r, &w);

                // Step 2: Boot PMU firmware — PGOB control register (0x10a78c)
                // is only writable when the PMU falcon is running firmware.
                // Falcon CPUCTL bits: 0x10=HALTED, 0x20=HRESET/STOPPED.
                // Running state = no halt bits set (cpuctl & 0x30 == 0).
                {
                    let pmu_cpuctl = r(0x10_A100);
                    let pmu_running = pmu_cpuctl != 0xDEAD_DEAD
                        && pmu_cpuctl & 0xBAD0_0000 != 0xBAD0_0000
                        && pmu_cpuctl & 0x30 == 0;

                    tracing::info!(
                        pmu_cpuctl = format_args!("{pmu_cpuctl:#010x}"),
                        pmu_running,
                        halted = pmu_cpuctl & 0x10 != 0,
                        hreset = pmu_cpuctl & 0x20 != 0,
                        "POST-done: PMU state before PGOB"
                    );

                    if !pmu_running {
                        let pmu_ok = super::pmu::gk110_pmu_boot(guard);
                        tracing::info!(pmu_ok, "POST-done: booted PMU for PGOB");
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }

                    // Diagnostic: check if 0x10a78c accepts writes from VFIO
                    let psw_pre = r(0x10_a78c);
                    w(0x10_a78c, 0x0000_0002);
                    let psw_wrote = r(0x10_a78c);
                    w(0x10_a78c, psw_pre); // restore
                    tracing::info!(
                        psw_pre = format_args!("{psw_pre:#010x}"),
                        psw_wrote = format_args!("{psw_wrote:#010x}"),
                        writable = psw_wrote == 0x0000_0002,
                        "PSW register (0x10a78c) write test"
                    );
                }

                // Step 3: PGOB disable — try three approaches in order:
                //   a) GK104-style PG_CTRL bit 30 (simplest, verified fuse check)
                //   b) nvidia-470 PSW-only handshake
                //   c) Full Nouveau gk110_pmu_pgob magic table
                let check_gpccs0 = || {
                    let v = r(0x50_2100); // GPCCS falcon CPUCTL
                    v != 0xDEAD_DEAD && v != 0 && v & 0xBAD0_0000 != 0xBAD0_0000
                };

                super::pgob::gk104_pgob_disable(guard);
                super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

                if !check_gpccs0() {
                    tracing::info!(
                        gpccs0 = format_args!("{:#010x}", r(0x50_2100)),
                        "gk104 PG_CTRL didn't ungate — trying nvidia-470 PSW"
                    );
                    super::pgob::nvidia470_pgob_disable(guard);
                    super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
                }

                if !check_gpccs0() {
                    tracing::info!(
                        gpccs0 = format_args!("{:#010x}", r(0x50_2100)),
                        "nvidia-470 PSW didn't ungate — full gk110_pmu_pgob magic table"
                    );
                    super::pgob::gk110_pgob_disable(guard);
                    super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
                }
                let pgob_ok = check_gpccs0();

                super::pri::gk104_privring_timing(&r, &w);
                super::pri::nouveau_pri_ring_init(&r, &w);
                super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

                let gpccs0_post = r(0x50_2100);
                let gr_hub_post = r(0x40_0000);
                let pri_hub = r(0x12_0070);
                let pri_rop = r(0x12_0074);
                let pri_gpc = r(0x12_0078);
                let gr_gpc_nr = r(0x40_9604);
                tracing::info!(
                    gpccs0 = format_args!("{gpccs0_post:#010x}"),
                    gr_hub = format_args!("{gr_hub_post:#010x}"),
                    pri_hub, pri_rop, pri_gpc,
                    gr_gpc_nr = format_args!("{gr_gpc_nr:#010x}"),
                    gpc_ok = gpccs0_post != 0xDEAD_DEAD && gpccs0_post & 0xBAD0_0000 != 0xBAD0_0000,
                    pgob_ok,
                    "POST-done: state after PGOB (0x70=hub, 0x74=rop, 0x78=gpc)"
                );
            }
        }

        // Full cold-path boot: GR MMIO init + firmware upload + boot.
        super::kepler_fecs_boot::kepler_load_and_boot_fecs(
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
    let gpc0_early = r(0x50_2608);
    {
        let gpc1 = r(0x50_8000);
        let gpc_bcast = r(0x41_8000);
        let pmu_pgob = r(0x10_a78c);
        let pri_ring_intr = r(0x12_0058);
        // PRI ring master topology: 0x120070=hub, 0x120074=ROP, 0x120078=GPC
        let pri_hub_cnt = r(0x12_0070);
        let pri_rop_cnt = r(0x12_0074);
        let pri_gpc_cnt = r(0x12_0078);
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
            pri_hub_cnt = format_args!("{pri_hub_cnt:#010x}"),
            pri_rop_cnt = format_args!("{pri_rop_cnt:#010x}"),
            pri_gpc_cnt = format_args!("{pri_gpc_cnt:#010x}"),
            "EARLIEST diagnostic (0x70=hub, 0x74=rop, 0x78=gpc)"
        );

        // GPC power management diagnostic — understand WHY GPCs are 0xbadf1100.
        let ppwr_gate_sts0 = r(0x02_0840); // PPWR power gate status bank 0
        let ppwr_gate_sts1 = r(0x02_0844); // PPWR power gate status bank 1
        let therm_gate_ctrl = r(0x02_0200); // PTHERM gate control
        let pgraph_pri_be = r(0x40_0134); // GR_PRI_BE_EN
        let gr_fe_pwr = r(0x40_4170);     // GR frontend power
        let pmu_pgob_cfg = r(0x10_a78c);  // PMU PGOB control
        let blcg_gr = r(0x40_0110);        // BLCG GR engine
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

    // Step 0: Halt the old PMU firmware BEFORE any PMC changes.
    //
    // The Nouveau PMU firmware actively manages power domains. When we
    // change PMC_ENABLE, the running PMU may interpret this as a signal
    // to re-gate engines, counteracting our clock/power setup. Halt it
    // now and boot fresh PMU firmware later when we need it.
    let pmu_was_running;
    {
        const PMU_BASE: u32 = 0x10_A000;
        let pmu_cpuctl = r(PMU_BASE + 0x100);
        pmu_was_running = pmu_cpuctl != 0xDEAD_DEAD
            && pmu_cpuctl & 0xBAD0_0000 != 0xBAD0_0000
            && pmu_cpuctl & 0x30 == 0; // running = no HALTED(0x10) or HRESET(0x20) bits
        tracing::info!(
            pmu_cpuctl = format_args!("{pmu_cpuctl:#010x}"),
            pmu_running = pmu_was_running,
            halted = pmu_cpuctl & 0x10 != 0,
            hreset = pmu_cpuctl & 0x20 != 0,
            "PMU falcon state before halt"
        );

        if pmu_was_running {
            // Halt PMU via PTOP reset (same method as gt215_pmu_reset)
            let bar0 = guard.inner();
            let rd_raw =
                |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
            let wr_raw =
                |reg: u32, val: u32| { let _ = bar0.write_u32(reg as usize, val); };

            let ptop = rd_raw(0x02_2210);
            wr_raw(0x02_2210, ptop & !0x01); // disable PMU
            rd_raw(0x02_2210);
            std::thread::sleep(std::time::Duration::from_millis(5));
            wr_raw(0x02_2210, ptop | 0x01); // re-enable (puts PMU into clean reset state)
            rd_raw(0x02_2210);
            std::thread::sleep(std::time::Duration::from_millis(10));

            let pmu_after = r(PMU_BASE + 0x100);
            tracing::info!(
                pmu_cpuctl = format_args!("{pmu_after:#010x}"),
                halted = (pmu_after & 0x20 == 0),
                "PMU halted via PTOP reset (no longer managing power)"
            );
        }

        // PNVIO/crystal diagnostic: check if the reference clock chain is alive
        let pnvio_xtal = r(0x00e220);
        let pnvio_ctrl = r(0x00e000);
        let pnvio_cfg0 = r(0x00e004);
        let ref_pll0_ctrl = r(0x00e800);
        let ref_pll0_coef = r(0x00e804);
        tracing::info!(
            pnvio_xtal = format_args!("{pnvio_xtal:#010x}"),
            pnvio_ctrl = format_args!("{pnvio_ctrl:#010x}"),
            pnvio_cfg0 = format_args!("{pnvio_cfg0:#010x}"),
            ref_pll0_ctrl = format_args!("{ref_pll0_ctrl:#010x}"),
            ref_pll0_coef = format_args!("{ref_pll0_coef:#010x}"),
            "PNVIO/crystal state (reference clock chain)"
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
                let rd_raw = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
                let wr_raw = |reg: u32, val: u32| { let _ = bar0.write_u32(reg as usize, val); };

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
                    &|reg, val| { let _ = bar0.write_u32(reg as usize, val); },
                );
                super::pri::clear_pri_ring_faults(bar0, &rd_raw, &wr_raw);

                let pmc_after = rd_raw(0x200);

                // ── Nouveau-style clock tree diagnostic ──
                // The nvidia-470 PCLOCK PLLs at 0x130xxx are a red herring —
                // they require PMU to enable their power domain. Nouveau uses
                // 0x137xxx for engine clocks (gk104_clk.c).
                {
                    let r_diag = |reg: u32| -> u32 {
                        bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD)
                    };
                    let w_diag = |reg: u32, val: u32| {
                        let _ = bar0.write_u32(reg as usize, val);
                    };

                    // Reference PLLs (always-on domain)
                    let ref0_ctrl = r_diag(0xe800);
                    let ref0_coef = r_diag(0xe804);
                    tracing::info!(
                        ref0_ctrl = format_args!("{ref0_ctrl:#010x}"),
                        ref0_coef = format_args!("{ref0_coef:#010x}"),
                        pmc = format_args!("{pmc_after:#010x}"),
                        "Clock chain: reference PLLs + PMC"
                    );

                    // Full Nouveau clock tree readout
                    super::kepler_nouveau_clk::nouveau_clock_diagnostic(&r_diag);

                    // Test which 0x137xxx sub-ranges are writable
                    let (pll_w, div_w, out_w) =
                        super::kepler_nouveau_clk::test_137xxx_writability(&r_diag, &w_diag);

                    if div_w || pll_w {
                        // 0x137xxx registers ARE writable — program crystal-based
                        // engine clocks so PGRAPH has a functional clock domain.
                        tracing::info!(
                            pll_writable = pll_w,
                            div_writable = div_w,
                            "0x137xxx writable — programming Nouveau-style engine clocks"
                        );

                        // Start with crystal divider clocks (108 MHz)
                        super::kepler_nouveau_clk::program_crystal_clocks(&r_diag, &w_diag);

                        // If PLLs are also writable, program 405 MHz engine PLLs
                        if pll_w {
                            super::kepler_nouveau_clk::program_engine_plls(&r_diag, &w_diag);
                        }
                    } else {
                        tracing::warn!(
                            "0x137xxx NOT writable — engine clocks may not be reaching PGRAPH"
                        );
                    }
                }

                let gpc0_after = rd_raw(0x50_2608);
                let fecs_after = rd_raw(FECS + CPUCTL_OFF);
                tracing::info!(
                    gpc0 = format_args!("{gpc0_after:#010x}"),
                    fecs = format_args!("{fecs_after:#010x}"),
                    "State after warm PMC re-enable + Nouveau clock init"
                );
            }
            // NOTE: nvidia-470 PCLOCK PLLs (0x130xxx) are intentionally NOT
            // programmed here. They require PMU to enable their analog power
            // domain, which never works on headless K80 warm-catch. Instead,
            // we program Nouveau-style 0x137xxx engine clocks above (crystal
            // dividers or PLLs), which provide a functional clock to PGRAPH.
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
            let wr = |reg: u32, val: u32| { let _ = bar0.write_u32(reg as usize, val); };

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
            gr_hub = format_args!("{gr_hub_after:#010x}[{}]", if is_ok(gr_hub_after) { "OK" } else { "FAULT" }),
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
                && pmu_cpuctl & 0x30 == 0; // running = no HALTED(0x10) or HRESET(0x20)

            tracing::info!(
                pmu_cpuctl = format_args!("{pmu_cpuctl:#010x}"),
                pmu_running,
                halted = pmu_cpuctl & 0x10 != 0,
                hreset = pmu_cpuctl & 0x20 != 0,
                "PMU falcon state (left running for GPC power management)"
            );

            if !pmu_running {
                let pmu_ok = super::pmu::gk110_pmu_boot(guard);
                tracing::info!(pmu_ok, "PMU firmware boot (for PGRAPH power domain)");
                std::thread::sleep(std::time::Duration::from_millis(50));
            } else {
                tracing::info!("PMU already running from nouveau — skipping re-boot");
            }

            // Apply privring timing before PGOB (Nouveau: gk104_privring_init)
            super::pri::gk104_privring_timing(&r, &w);

            // ── Power management diagnostic ──
            // The PMU-mediated PGOB (0x10a78c + 0x0205xx) has been ineffective
            // because the PMU firmware doesn't process the PGOB request.
            // Try direct PG_CTRL register (0x020004) as a bypass.
            {
                let bar0 = guard.inner();
                let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
                let wr = |reg: u32, val: u32| { let _ = bar0.write_u32(reg as usize, val); };

                let pg_ctrl = rd(0x02_0004);
                let pg_status = rd(0x02_0008);
                let pg_elpg = rd(0x02_0000);
                let pmu_pgob = rd(0x10_a78c);
                let top_scg = rd(0x02_2204);
                let pg_fsm = rd(0x02_0010);
                tracing::info!(
                    pg_ctrl = format_args!("{pg_ctrl:#010x}"),
                    pg_status = format_args!("{pg_status:#010x}"),
                    pg_elpg = format_args!("{pg_elpg:#010x}"),
                    pmu_pgob = format_args!("{pmu_pgob:#010x}"),
                    top_scg = format_args!("{top_scg:#010x}"),
                    pg_fsm = format_args!("{pg_fsm:#010x}"),
                    "Power management diagnostic (pre-PGOB)"
                );

                // Attempt 1: Direct PG_CTRL write — clear PGOB bit (31),
                // set ELPG_DISABLE bit (30) to prevent re-gating.
                wr(0x02_0004, (pg_ctrl & !0x8000_0000) | 0x4000_0000);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let pg_ctrl_post = rd(0x02_0004);
                let gpccs0_test = rd(0x50_2100);
                tracing::info!(
                    pg_ctrl_post = format_args!("{pg_ctrl_post:#010x}"),
                    gpccs0 = format_args!("{gpccs0_test:#010x}"),
                    gpc_alive = gpccs0_test != 0xDEAD_DEAD && gpccs0_test & 0xBAD0_0000 != 0xBAD0_0000 && gpccs0_test != 0,
                    "PG_CTRL direct PGOB bypass attempt"
                );

                // Attempt 2: Write 0 to all power gate control registers
                // to force-ungate everything.
                for &pg_reg in &[0x02_0520u32, 0x02_0524, 0x02_0528, 0x02_052C, 0x02_0530] {
                    let pre = rd(pg_reg);
                    wr(pg_reg, 0x0000_0000);
                    let post = rd(pg_reg);
                    if pre != post {
                        tracing::info!(
                            reg = format_args!("{pg_reg:#08x}"),
                            pre = format_args!("{pre:#010x}"),
                            post = format_args!("{post:#010x}"),
                            "Power gate register changed"
                        );
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(50));

                let gpccs0_cpuctl = rd(0x50_2100);
                tracing::info!(
                    gpccs0_cpuctl = format_args!("{gpccs0_cpuctl:#010x}"),
                    gpc_alive = gpccs0_cpuctl != 0xDEAD_DEAD && gpccs0_cpuctl & 0xBAD0_0000 != 0xBAD0_0000 && gpccs0_cpuctl != 0,
                    "After direct power gate clear"
                );
            }

            // Standard PGOB sequences — try all three approaches.
            // Use GPCCS CPUCTL (GPC+0x2100) instead of GPC identity (GPC+0x0000).
            let check_gpccs0_alive = || {
                let v = r(0x50_2100);
                v != 0xDEAD_DEAD && v != 0 && v & 0xBAD0_0000 != 0xBAD0_0000
            };

            tracing::info!("Trying gk104-style PG_CTRL PGOB disable");
            super::pgob::gk104_pgob_disable(guard);
            super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

            if !check_gpccs0_alive() {
                tracing::info!("gk104 PG_CTRL insufficient — trying nvidia-470 PSW");
                super::pgob::nvidia470_pgob_disable(guard);
                super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
            }

            if !check_gpccs0_alive() {
                tracing::info!("nvidia-470 PSW insufficient — running full gk110_pmu_pgob");
                super::pgob::gk110_pgob_disable(guard);
            }

            // Re-enumerate PRI ring after PGOB (GPC stations should appear)
            super::pri::gk104_privring_timing(&r, &w);
            super::pri::nouveau_pri_ring_init(&r, &w);
            super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

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
                let (clk_applied, clk_skipped) = super::kepler_clock::apply_gk110_clock_recipe(guard);
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Re-enumerate PRI ring after DEVINIT.
                {
                    let bar0 = guard.inner();
                    let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
                    let wr_raw = |reg: u32, val: u32| { let _ = bar0.write_u32(reg as usize, val); };
                    super::pri::nouveau_pri_ring_init(&rd, &wr_raw);
                    super::pri::clear_pri_ring_faults(bar0, &rd, &wr_raw);
                }

                // Re-scan topology after DEVINIT.
                cached_tpc_counts = [(0, 0); 8];
                cached_gpc_count = 0;
                cached_tpc_total = 0;
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
                let wr_raw = |reg: u32, val: u32| { let _ = bar0.write_u32(reg as usize, val); };

                let pmc = rd(0x200);
                wr_raw(0x200, pmc & !0x0000_1000);  // PGRAPH off
                rd(0x200);                            // flush
                wr_raw(0x200, pmc | 0x0000_1000);    // PGRAPH on
                rd(0x200);                            // flush

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
                    let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
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
            let wr_raw = |reg: u32, val: u32| { let _ = bar0.write_u32(reg as usize, val); };

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
        super::kepler_fecs_boot::kepler_load_and_boot_fecs(guard, cached_gpc_count, cached_tpc_total, &cached_tpc_counts);
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
        let gpccs0_warmcheck = r(0x50_2100); // GPCCS falcon CPUCTL
        let gpc_nr_reg = r(0x40_9604);
        let gpc0_alive = gpccs0_warmcheck != 0xDEAD_DEAD
            && gpccs0_warmcheck & 0xBAD0_0000 != 0xBAD0_0000
            && gpccs0_warmcheck != 0;
        let gpc_nr_readable = gpc_nr_reg != 0xDEAD_DEAD
            && gpc_nr_reg & 0xBAD0_0000 != 0xBAD0_0000;

        tracing::info!(
            gpccs0_cpuctl = format_args!("{gpccs0_warmcheck:#010x}"),
            gpc_nr_reg = format_args!("{gpc_nr_reg:#010x}"),
            gpc0_alive, gpc_nr_readable,
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
            let (gr_applied, gr_faulted) =
                super::kepler_gr_init::apply_gk110_gr_init(guard);
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
            super::kepler_fecs_boot::kepler_load_and_boot_fecs(guard, topo.0, topo.1, &topo.2);
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
                let rd_raw = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
                let wr_raw = |reg: u32, val: u32| { let _ = bar0.write_u32(reg as usize, val); };
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

            let (gr_applied, gr_faulted) =
                super::kepler_gr_init::apply_gk110_gr_init(guard);
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
                gr_applied, gr_faulted,
                "Slow path: state after full GR reinit"
            );

            let fecs_pri_fault = fecs_cpuctl_post & 0xBAD0_0000 == 0xBAD0_0000
                || fecs_cpuctl_post == 0xDEAD_DEAD;
            if fecs_pri_fault {
                tracing::error!("FECS PRI-faulted after reinit — aborting firmware upload");
                return;
            }

            let topo = super::pri::scan_gpc_topology(guard);
            super::kepler_fecs_boot::kepler_load_and_boot_fecs(guard, topo.0, topo.1, &topo.2);
        }
    }
}
