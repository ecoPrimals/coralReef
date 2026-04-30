// SPDX-License-Identifier: AGPL-3.0-or-later
//! ENGCTL HRESET sequencer, nouveau warm STARTCPU, fenced fallback reinit ladders.

use super::super::hardware_guard::GuardedBar;

#[must_use]
pub(super) fn warm_fecs_engctl_restart_or_fallback(guard: &GuardedBar<'_>) -> bool {
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
    super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

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
        super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        let gpccs0_warmcheck = r(0x50_2100); // GPCCS falcon CPUCTL
        let gpc_nr_reg = r(0x40_9604);
        let gpc0_alive = gpccs0_warmcheck != 0xDEAD_DEAD
            && gpccs0_warmcheck & 0xBAD0_0000 != 0xBAD0_0000
            && gpccs0_warmcheck != 0;
        let gpc_nr_readable = gpc_nr_reg != 0xDEAD_DEAD && gpc_nr_reg & 0xBAD0_0000 != 0xBAD0_0000;

        tracing::info!(
            gpccs0_cpuctl = format_args!("{gpccs0_warmcheck:#010x}"),
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
            let (gr_applied, gr_faulted) = super::super::kepler_gr_init::apply_gk110_gr_init(guard);
            super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
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
            super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
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

            let topo = super::super::pri::scan_gpc_topology(guard);
            super::super::kepler_fecs_boot::kepler_load_and_boot_fecs(
                guard, topo.0, topo.1, &topo.2,
            );
        } else {
            // ── Slow path: full cold reinit ──
            tracing::warn!("GPCs not alive — full GR reinit with PGOB disable + GPC topology");

            // Boot PMU firmware first — PMU manages power domains.
            let pmu_ok = super::super::pmu::gk110_pmu_boot(guard);
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
            super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

            super::super::pri::vbios_pri_ring_init(&r, &w);
            super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

            w(0x40_0124, 0x0000_0002);

            w(0x40_0500, 0x0001_0001);
            for _ in 0..200 {
                std::thread::sleep(std::time::Duration::from_millis(10));
                let v = r(0x40_0700);
                if v & 0x0000_0002 != 0 {
                    break;
                }
            }
            super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

            super::super::pri::vbios_pri_ring_init(&r, &w);
            super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

            let (gr_applied, gr_faulted) = super::super::kepler_gr_init::apply_gk110_gr_init(guard);
            super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

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
                return true;
            }

            let topo = super::super::pri::scan_gpc_topology(guard);
            super::super::kepler_fecs_boot::kepler_load_and_boot_fecs(
                guard, topo.0, topo.1, &topo.2,
            );
        }
    }

    false
}
