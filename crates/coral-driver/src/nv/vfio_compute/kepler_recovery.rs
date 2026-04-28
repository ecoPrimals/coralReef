// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kepler cold recovery — full PMC, DEVINIT, clock recipe after bus reset.

/// GK110 PGOB (Power Gate Off Block) disable — exact match of nouveau's
/// `gk110_pmu_pgob(pmu, false)` from `nvkm/subdev/pmu/gk110.c`.
///
/// Cold recovery after VFIO bus reset.
///
/// vfio-pci performs a secondary bus reset every time the VFIO group is
/// released. This wipes all GPU register state, collapsing PMC_ENABLE
/// from ~0xfc37b1ef to ~0xc0002020. PLLs may survive (hardware-latched),
/// but all engine enables, PRI ring, power domains, and — critically —
/// VBIOS DEVINIT state (GPC fuses, memory controller, PTHERM, GPIO)
/// need to be restored.
///
/// Without DEVINIT, PGRAPH has no GPC fuse configuration and cannot route
/// to GPCs, causing GPCCS PIO (both broadcast 0x41A000 and per-GPC 0x502000+)
/// to silently fail (reads return 0, writes go nowhere).
///
/// Sequence:
/// 1. Minimal PMC (PDAEMON + PRING) + PRI ring init
/// 2. VBIOS DEVINIT (programs GPC fuses, clocks, memory controller)
/// 3. Full PMC_ENABLE + PRI ring re-enumerate
/// 4. Clock recipe (PLL tree for engine clocks)
pub(super) fn kepler_cold_recovery(guard: &super::hardware_guard::GuardedBar<'_>) {
    let bar0 = guard.inner();
    let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
    let wr = |reg: u32, val: u32| {
        let _ = bar0.write_u32(reg as usize, val);
    };
    let r = |reg: u32| -> u32 { guard.read_u32(reg).unwrap_or(0xDEAD_DEAD) };
    let w = |reg: u32, val: u32| {
        let _ = guard.write_u32(reg, val);
    };

    // Step 1: Enable full PMC from the start. Previous approach used minimal
    // PMC (0x2020 = PDAEMON + PRING only) out of fear that unclocked stations
    // jam the ring. But this means DEVINIT runs with PCLOCK disabled —
    // preventing PLL configuration. PCLOCK gets its base clock from the
    // crystal oscillator, not from engine PLLs, so it should be safe to
    // enable early. Write NV470 value (hardware-validated to be accepted).
    const NV470_PMC_ENABLE: u32 = 0xe011_312c;
    wr(0x200, NV470_PMC_ENABLE);
    rd(0x200);
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Configure hub station parameters (from K80 VBIOS DEVINIT) BEFORE ring init.
    super::pri::write_kepler_hub_station_params(&wr);

    // Step 2: PRI ring init with VBIOS command 0x03 (not 0x04).
    // Command 0x04 never completes after bus reset; 0x03 activates the
    // bus interface that allows the INIT token to traverse the ring.
    let ring_ok = super::pri::vbios_pri_ring_init(&|reg| r(reg), &|reg, val| w(reg, val));

    let hub = r(0x12_0070);
    let gpc = r(0x12_0078);
    tracing::info!(
        ring_ok,
        hub,
        gpc,
        pmc = format_args!("{:#010x}", rd(0x200)),
        "Cold recovery: PRI ring init (minimal PMC, cmd 0x03)"
    );

    // Step 3: VBIOS DEVINIT — programs GPC fuses, memory controller,
    // PTHERM, GPIO, and other hardware-level init in the correct power-up
    // order. This is the critical missing piece: without DEVINIT, PGRAPH
    // doesn't know about GPCs and GPCCS PIO is dead.
    super::vbios_devinit::kepler_vbios_devinit(bar0);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let pll0_post_devinit = rd(0x13_0000);
    tracing::info!(
        pll0 = format_args!("{pll0_post_devinit:#010x}"),
        "Cold recovery: VBIOS DEVINIT completed"
    );

    // Step 4: Transition to full PMC_ENABLE so all engine stations respond.
    const GK210_PMC_ENABLE: u32 = 0xFC37_B1EF;
    wr(0x200, GK210_PMC_ENABLE);
    rd(0x200);
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Re-enumerate with full PMC — engine stations now have power.
    super::pri::vbios_pri_ring_init(&|reg| r(reg), &|reg, val| w(reg, val));
    super::pri::clear_pri_ring_faults(bar0, &|reg| r(reg), &|reg, val| w(reg, val));

    // PMC secondary control (matches kepler_cold_init Phase 3).
    wr(0x640, 0xFEBF_B1E1);
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Step 4.5: Clock chain diagnostic + PCLOCK master control.
    // After PMC enable and PRI ring re-enumerate, the PCLOCK station is
    // reachable but PLL registers won't accept writes until the PCLOCK
    // master controller and PBUS debug mode are configured.
    {
        let ref_pll0_ctrl = rd(0xe800);
        let ref_pll0_coef = rd(0xe804);
        let ref_pll1_ctrl = rd(0xe820);
        let ref_pll1_coef = rd(0xe824);
        let pclock_pll0 = rd(0x13_0000);
        let pclock_master = rd(0x13_8000);
        let clk_src_sel = rd(0x13_7100);
        tracing::info!(
            ref_pll0_ctrl = format_args!("{ref_pll0_ctrl:#010x}"),
            ref_pll0_coef = format_args!("{ref_pll0_coef:#010x}"),
            ref_pll1_ctrl = format_args!("{ref_pll1_ctrl:#010x}"),
            ref_pll1_coef = format_args!("{ref_pll1_coef:#010x}"),
            pclock_pll0 = format_args!("{pclock_pll0:#010x}"),
            pclock_master = format_args!("{pclock_master:#010x}"),
            clk_src_sel = format_args!("{clk_src_sel:#010x}"),
            "Cold recovery: clock chain BEFORE master control"
        );

        // Write-test: PCLOCK PLL register writability before master control
        wr(0x13_0000, 0x0000_0001);
        let pc0_test = rd(0x13_0000);
        wr(0x13_0000, 0x0000_0000);
        tracing::info!(
            readback = format_args!("{pc0_test:#010x}"),
            writable = (pc0_test == 0x0000_0001),
            "Cold recovery: PCLOCK PLL write test (pre-master-ctrl)"
        );

        // Reference PLLs (0xe800/0xe820) are in the PNVIO domain, driven
        // by the crystal oscillator. If dead, program them first.
        let ref_alive = ref_pll0_ctrl != 0 && ref_pll0_coef != 0 && ref_pll0_ctrl != 0xDEAD_DEAD;
        if !ref_alive {
            tracing::warn!("Reference PLLs dead — programming from crystal (27 MHz → ~2 GHz VCO)");
            let ref_coef: u32 = 1 | (74 << 8); // M=1, N=74 → 1998 MHz
            wr(0xe800, 0);
            wr(0xe804, ref_coef);
            wr(0xe800, 1);
            std::thread::sleep(std::time::Duration::from_millis(20));
            wr(0xe820, 0);
            wr(0xe824, ref_coef);
            wr(0xe820, 1);
            std::thread::sleep(std::time::Duration::from_millis(20));
            let ref0_post = rd(0xe800);
            tracing::info!(
                ref0_ctrl = format_args!("{ref0_post:#010x}"),
                "After reference PLL programming"
            );
        }

        // PCLOCK clock routing — on GK104/GK110, nouveau programs routing
        // registers (0x137xxx) BEFORE PLLs. The master routing at 0x137000
        // may gate PLL register writability. Write the routing entries from
        // the clock recipe first, then test if PLLs become writable.
        {
            for &(reg, val) in super::kepler_clock::gk110_clock_recipe_entries() {
                if reg >= 0x13_7000 && reg <= 0x13_7FFF {
                    wr(reg, val);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));

            let pc0_post = rd(0x13_0000);
            wr(0x13_0000, 0x0000_0001);
            let pc0_test = rd(0x13_0000);
            wr(0x13_0000, 0x0000_0000);
            let clk_master = rd(0x13_7000);
            let clk_src = rd(0x13_7100);
            tracing::info!(
                pclock_pll0 = format_args!("{pc0_post:#010x}"),
                write_test = format_args!("{pc0_test:#010x}"),
                writable = (pc0_test == 0x0000_0001),
                clk_master = format_args!("{clk_master:#010x}"),
                clk_src = format_args!("{clk_src:#010x}"),
                "Cold recovery: after 0x137xxx routing writes"
            );
        }

        // Also try the clock domain selectors (0x132xxx-0x134xxx)
        {
            for &(reg, val) in super::kepler_clock::gk110_clock_recipe_entries() {
                if reg >= 0x13_2000 && reg <= 0x13_6FFF {
                    wr(reg, val);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));

            wr(0x13_0000, 0x0000_0001);
            let pc0_test = rd(0x13_0000);
            wr(0x13_0000, 0x0000_0000);
            tracing::info!(
                write_test = format_args!("{pc0_test:#010x}"),
                writable = (pc0_test == 0x0000_0001),
                "Cold recovery: after domain selector writes"
            );
        }
    }

    // Step 5: Apply clock recipe — now that all engine clock domains are
    // gated on via PMC, program the PLL tree so GPC clocks actually run.
    let (clk_applied, clk_skipped) = super::kepler_clock::apply_gk110_clock_recipe(guard);
    std::thread::sleep(std::time::Duration::from_millis(200));

    let pmc_after = rd(0x200);
    let pll0_final = rd(0x13_0000);
    let hub2 = r(0x12_0070);
    let gpc2 = r(0x12_0078);
    let gpc0_post = r(0x50_2608);
    tracing::info!(
        pmc = format_args!("{pmc_after:#010x}"),
        pll0 = format_args!("{pll0_final:#010x}"),
        clk_applied,
        clk_skipped,
        hub = hub2,
        gpc = gpc2,
        gpc0 = format_args!("{gpc0_post:#010x}"),
        "Cold recovery: full PMC + DEVINIT + clocks + PRI ring"
    );
}
