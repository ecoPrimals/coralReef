// SPDX-License-Identifier: AGPL-3.0-or-later
//! nouveau POST-done fast path topology + PMC reset + PMU/PGOB + cold FECS load.

use super::super::hardware_guard::GuardedBar;

#[must_use]
pub(super) fn maybe_post_done_early_boot(guard: &GuardedBar<'_>, _bdf: &str) -> bool {
    use crate::nv::kepler_falcon;

    const FECS: u32 = kepler_falcon::FECS_BASE;
    const GPCCS: u32 = kepler_falcon::GPCCS_BASE;
    const ENGCTL_OFF: u32 = 0x3C0;
    const CPUCTL_OFF: u32 = 0x100;
    let r = guard.read_fn();
    let w = |reg: u32, val: u32| {
        if let Err(refusal) = guard.write_u32(reg, val) {
            tracing::error!(%refusal, "kepler_warm_gr_init: hardware guard refused write");
        }
    };

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

    if !(post_done && pgraph_on && gpc0_ok) {
        return false;
    }

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
            let rd_raw = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
            let wr_raw = |reg: u32, val: u32| {
                let _ = bar0.write_u32(reg as usize, val);
            };

            let pmc_cur = rd_raw(0x200);
            wr_raw(0x200, pmc_cur & !0x0000_1000);
            rd_raw(0x200);
            std::thread::sleep(std::time::Duration::from_millis(20));
            wr_raw(0x200, pmc_cur | 0x0000_1000);
            rd_raw(0x200);
            std::thread::sleep(std::time::Duration::from_millis(50));

            super::super::pri::nouveau_pri_ring_init(&rd_raw, &wr_raw);
            super::super::pri::clear_pri_ring_faults(bar0, &rd_raw, &wr_raw);

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
        super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        let gpccs0_cpuctl = r(0x50_2100);
        let gr_hub_check = r(0x40_0000);
        let gpcs_alive = gpccs0_cpuctl != 0xDEAD_DEAD
            && gpccs0_cpuctl & 0xBAD0_0000 != 0xBAD0_0000
            && gpccs0_cpuctl != 0;
        let gr_hub_ok = gr_hub_check != 0xDEAD_DEAD && gr_hub_check & 0xBAD0_0000 != 0xBAD0_0000;

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
            super::super::pri::gk104_privring_timing(&r, &w);

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
                    let pmu_ok = super::super::pmu::gk110_pmu_boot(guard);
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

            super::super::pgob::gk104_pgob_disable(guard);
            super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

            if !check_gpccs0() {
                tracing::info!(
                    gpccs0 = format_args!("{:#010x}", r(0x50_2100)),
                    "gk104 PG_CTRL didn't ungate — trying nvidia-470 PSW"
                );
                super::super::pgob::nvidia470_pgob_disable(guard);
                super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
            }

            if !check_gpccs0() {
                tracing::info!(
                    gpccs0 = format_args!("{:#010x}", r(0x50_2100)),
                    "nvidia-470 PSW didn't ungate — full gk110_pmu_pgob magic table"
                );
                super::super::pgob::gk110_pgob_disable(guard);
                super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
            }
            let pgob_ok = check_gpccs0();

            super::super::pri::gk104_privring_timing(&r, &w);
            super::super::pri::nouveau_pri_ring_init(&r, &w);
            super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

            let gpccs0_post = r(0x50_2100);
            let gr_hub_post = r(0x40_0000);
            let pri_hub = r(0x12_0070);
            let pri_rop = r(0x12_0074);
            let pri_gpc = r(0x12_0078);
            let gr_gpc_nr = r(0x40_9604);
            tracing::info!(
                gpccs0 = format_args!("{gpccs0_post:#010x}"),
                gr_hub = format_args!("{gr_hub_post:#010x}"),
                pri_hub,
                pri_rop,
                pri_gpc,
                gr_gpc_nr = format_args!("{gr_gpc_nr:#010x}"),
                gpc_ok = gpccs0_post != 0xDEAD_DEAD && gpccs0_post & 0xBAD0_0000 != 0xBAD0_0000,
                pgob_ok,
                "POST-done: state after PGOB (0x70=hub, 0x74=rop, 0x78=gpc)"
            );
        }
    }

    // Full cold-path boot: GR MMIO init + firmware upload + boot.
    super::super::kepler_fecs_boot::kepler_load_and_boot_fecs(
        guard,
        cached_gpc_count,
        cached_tpc_total,
        &cached_tpc_counts,
    );

    true
}
