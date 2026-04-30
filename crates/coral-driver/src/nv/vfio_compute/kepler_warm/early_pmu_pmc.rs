// SPDX-License-Identifier: AGPL-3.0-or-later
//! PLL/bus-reset probes, PMU halt, PMC PGRAPH ladders (nouveau vs bus reset).

use super::super::hardware_guard::GuardedBar;

#[must_use]
pub(super) fn warm_early_pmu_and_pmc_recovery(guard: &GuardedBar<'_>) -> bool {
    use crate::nv::kepler_falcon;

    const FECS: u32 = kepler_falcon::FECS_BASE;
    const CPUCTL_OFF: u32 = 0x100;

    let r = guard.read_fn();
    let pmc = r(0x200);

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
            let rd_raw = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
            let wr_raw = |reg: u32, val: u32| {
                let _ = bar0.write_u32(reg as usize, val);
            };

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
                super::super::pri::write_kepler_hub_station_params(&wr_raw);

                // VBIOS ring init command 0x03 (not 0x04) — discovers
                // all stations including PCLOCK.
                super::super::pri::vbios_pri_ring_init(
                    &|reg| bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD),
                    &|reg, val| {
                        let _ = bar0.write_u32(reg as usize, val);
                    },
                );
                super::super::pri::clear_pri_ring_faults(bar0, &rd_raw, &wr_raw);

                let pmc_after = rd_raw(0x200);

                // ── Nouveau-style clock tree diagnostic ──
                // The nvidia-470 PCLOCK PLLs at 0x130xxx are a red herring —
                // they require PMU to enable their power domain. Nouveau uses
                // 0x137xxx for engine clocks (gk104_clk.c).
                {
                    let r_diag =
                        |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
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
                    super::super::kepler_nouveau_clk::nouveau_clock_diagnostic(&r_diag);

                    // Test which 0x137xxx sub-ranges are writable
                    let (pll_w, div_w, _out_w) =
                        super::super::kepler_nouveau_clk::test_137xxx_writability(&r_diag, &w_diag);

                    if div_w || pll_w {
                        // 0x137xxx registers ARE writable — program crystal-based
                        // engine clocks so PGRAPH has a functional clock domain.
                        tracing::info!(
                            pll_writable = pll_w,
                            div_writable = div_w,
                            "0x137xxx writable — programming Nouveau-style engine clocks"
                        );

                        // Start with crystal divider clocks (108 MHz)
                        super::super::kepler_nouveau_clk::program_crystal_clocks(&r_diag, &w_diag);

                        // If PLLs are also writable, program 405 MHz engine PLLs
                        if pll_w {
                            super::super::kepler_nouveau_clk::program_engine_plls(&r_diag, &w_diag);
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
            super::super::kepler_recovery::kepler_cold_recovery(guard);
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

    false
}
