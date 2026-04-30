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
    tracing::info!(
        ok = phase3_ok,
        hub = hub2,
        pclock = format_args!("{:#010x}", r(0x13_0000)),
        fecs = format_args!("{:#010x}", r(0x40_9100)),
        "Phase 3: PRI ring re-init (full PMC)"
    );

    // PMC secondary control
    w(0x640, 0xFEBF_B1E1);
    std::thread::sleep(std::time::Duration::from_millis(10));

    // ── Phase 3.25: PCLOCK sequencer / master probe (read-only) ──
    // The clock sequencer at 0x138000 controls PLL access. Reading its
    // state reveals whether PLL registers are gated by sequencer mode.
    {
        let seq_regs: [(u32, &str); 10] = [
            (0x13_8000, "SEQ_CTRL"),
            (0x13_8004, "SEQ_CFG0"),
            (0x13_8008, "SEQ_CFG1"),
            (0x13_800C, "SEQ_CFG2"),
            (0x13_8010, "SEQ_CFG3"),
            (0x13_8014, "SEQ_CFG4"),
            (0x13_8018, "SEQ_CFG5"),
            (0x13_801C, "SEQ_CFG6"),
            (0x13_9000, "PCLOCK_CG0"),
            (0x13_9004, "PCLOCK_CG1"),
        ];
        for (reg, name) in &seq_regs {
            let val = r(*reg);
            tracing::info!(
                reg = format_args!("{reg:#010x}"),
                val = format_args!("{val:#010x}"),
                name,
                "Phase 3.25: PCLOCK sequencer probe"
            );
        }

        let pmc_readback = r(0x200);
        let pmc640_readback = r(0x640);
        let devinit_done = r(0x001540);
        tracing::info!(
            pmc = format_args!("{pmc_readback:#010x}"),
            pmc640 = format_args!("{pmc640_readback:#010x}"),
            devinit_done = format_args!("{devinit_done:#010x}"),
            "Phase 3.25: PMC state after full enable"
        );

        let ref_pll0 = r(0xe800);
        let ref_pll0_coef = r(0xe804);
        tracing::info!(
            ref_pll0 = format_args!("{ref_pll0:#010x}"),
            ref_pll0_coef = format_args!("{ref_pll0_coef:#010x}"),
            "Phase 3.25: Reference PLL state (should be alive)"
        );

        let pll0 = r(0x13_0000);
        w(0x13_0000, 0x8000_0101); // PLL enable + minimal N=1,M=1
        let pll0_rb = r(0x13_0000);
        w(0x13_0000, 0);
        tracing::info!(
            before = format_args!("{pll0:#010x}"),
            after_write = format_args!("{pll0_rb:#010x}"),
            writable = (pll0_rb != 0),
            "Phase 3.25: PCLOCK PLL0 write test (post-PMC, pre-recipe)"
        );

        // If PCLOCK PLLs are unwritable, try resetting the PMU to release
        // any clock sequencer lock. The PMU (Nouveau's daemon) may be
        // holding the sequencer in "auto" mode, blocking direct PLL writes.
        if pll0_rb == 0 {
            let pmc_cur = r(0x200);
            const PMC_PDAEMON: u32 = 1 << 13;

            tracing::info!(
                pmc = format_args!("{pmc_cur:#010x}"),
                "Phase 3.25: PMU reset — toggling PMC bit 13 to release clock sequencer"
            );

            w(0x200, pmc_cur & !PMC_PDAEMON);
            std::thread::sleep(std::time::Duration::from_millis(20));
            w(0x200, pmc_cur | PMC_PDAEMON);
            std::thread::sleep(std::time::Duration::from_millis(50));

            let seq_after = r(0x13_8000);
            let pmu_cpuctl = r(0x10_a100);
            tracing::info!(
                seq_ctrl = format_args!("{seq_after:#010x}"),
                pmu_cpuctl = format_args!("{pmu_cpuctl:#010x}"),
                pmu_halted = (pmu_cpuctl & 0x10 != 0),
                "Phase 3.25: After PMU reset — sequencer state"
            );

            w(0x13_0000, 0x8000_0101);
            let pll0_post_reset = r(0x13_0000);
            w(0x13_0000, 0);
            tracing::info!(
                readback = format_args!("{pll0_post_reset:#010x}"),
                writable = (pll0_post_reset != 0),
                "Phase 3.25: PCLOCK PLL0 write test (after PMU reset)"
            );

            // If still unwritable, try clearing the clock sequencer
            // control register (0x138000). Write 0x00 to disable sequencer
            // auto-mode. Use isolated write for safety.
            if pll0_post_reset == 0 && seq_after != 0 {
                tracing::warn!(
                    seq_ctrl = format_args!("{seq_after:#010x}"),
                    "Phase 3.25: Attempting SEQ_CTRL clear (0x138000 → 0x00)"
                );
                w(0x13_8000, 0x0000_0000);
                std::thread::sleep(std::time::Duration::from_millis(10));

                let seq_cleared = r(0x13_8000);
                w(0x13_0000, 0x8000_0101);
                let pll0_post_seq = r(0x13_0000);
                w(0x13_0000, 0);
                tracing::info!(
                    seq_ctrl = format_args!("{seq_cleared:#010x}"),
                    pll0_readback = format_args!("{pll0_post_seq:#010x}"),
                    writable = (pll0_post_seq != 0),
                    "Phase 3.25: After SEQ_CTRL clear"
                );

                if pll0_post_seq == 0 {
                    // Try writing specific sequencer mode: manual (0x04)
                    w(0x13_8000, 0x0000_0004);
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    let seq_manual = r(0x13_8000);

                    w(0x13_0000, 0x8000_0101);
                    let pll0_manual = r(0x13_0000);
                    w(0x13_0000, 0);
                    tracing::info!(
                        seq_ctrl = format_args!("{seq_manual:#010x}"),
                        pll0_readback = format_args!("{pll0_manual:#010x}"),
                        writable = (pll0_manual != 0),
                        "Phase 3.25: After SEQ_CTRL manual mode (0x04)"
                    );
                }
            }
        }
    }

    // ── Phase 3.5: Apply clock recipe AFTER PMC enable ──
    // Now that all engine clock domains are gated on via PMC, the PCLOCK
    // registers (0x13xxxx) are accessible. Apply the captured PLL tree.
    let (applied, skipped) = super::kepler_clock::apply_gk110_clock_recipe(&guard);
    std::thread::sleep(std::time::Duration::from_millis(200));

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

    // ── Phase 4: PGRAPH reset ──
    let pmc_now = r(PMC_ENABLE);
    w(PMC_ENABLE, pmc_now & !PMC_PGRAPH);
    std::thread::sleep(std::time::Duration::from_millis(50));
    w(PMC_ENABLE, pmc_now | PMC_PGRAPH);
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Re-init ring after PGRAPH reset (PGRAPH stations rejoin)
    let _ = super::pri::vbios_pri_ring_init(&r, &w);
    super::pri::clear_pri_ring_faults(bar0, &r, &w);

    // Verify GPCCS and FECS accessibility
    let gpccs_cpuctl = r(kepler_falcon::GPCCS_BASE + 0x100);
    let gpccs_ok = gpccs_cpuctl & 0xBAD0_0000 != 0xBAD0_0000 && gpccs_cpuctl != 0xDEAD_DEAD;
    let pfifo_en = r(0x002504);
    tracing::info!(
        gpccs_cpuctl = format_args!("{gpccs_cpuctl:#010x}"),
        accessible = gpccs_ok,
        pfifo = format_args!("{pfifo_en:#010x}"),
        "GPCCS state after PGRAPH reset"
    );

    if !gpccs_ok {
        tracing::warn!(
            gpccs_cpuctl = format_args!("{gpccs_cpuctl:#010x}"),
            "GPCCS still PRI-faulted — GPC clocks may not have locked"
        );
        super::pri::kepler_pri_ring_diag(bar0, &r);
    }

    let fecs_cpuctl = r(kepler_falcon::FECS_BASE + 0x100);
    let is_pri_fault = fecs_cpuctl & 0xBAD0_0000 == 0xBAD0_0000 || fecs_cpuctl == 0xDEAD_DEAD;
    tracing::info!(
        fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
        accessible = !is_pri_fault,
        "FECS state after PGRAPH reset"
    );

    if is_pri_fault {
        tracing::warn!("FECS still PRI-faulted after PGRAPH reset — GR engine not accessible");
        return;
    }

    // ── Phase 5: PGRAPH MMIO init ──
    // Nouveau applies ~150 register writes to configure GR subsystems
    // (frontend, datastreamer, zcull, SM, backend, etc.) before booting
    // FECS. Without these, the falcon halts on boot.
    let (gr_applied, gr_faulted) = super::kepler_gr_init::apply_gk110_gr_init(&guard);
    super::pri::clear_pri_ring_faults(bar0, &r, &w);
    tracing::info!(gr_applied, gr_faulted, "Phase 5: PGRAPH MMIO init");

    // ── Phase 6: Load and boot FECS/GPCCS firmware ──
    let topo = super::pri::scan_gpc_topology(&guard);
    super::kepler_fecs_boot::kepler_load_and_boot_fecs(&guard, topo.0, topo.1, &topo.2);
}
