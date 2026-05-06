// SPDX-License-Identifier: AGPL-3.0-or-later
//! FECS unlock + PMU/PGOB + DEVINIT/PMC ladders ending in cold FECS upload.

use super::super::hardware_guard::GuardedBar;

#[must_use]
pub(super) fn maybe_gr_hub_firmware_prep_and_upload(guard: &GuardedBar<'_>) -> bool {
    use crate::nv::kepler_falcon;

    const FECS: u32 = kepler_falcon::FECS_BASE;
    const CPUCTL_OFF: u32 = 0x100;

    let r = guard.read_fn();
    let w = |reg: u32, val: u32| {
        if let Err(refusal) = guard.write_u32(reg, val) {
            tracing::error!(%refusal, "kepler_warm_gr_init: hardware guard refused write");
        }
    };

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
    super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
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
        return true;
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
        super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        std::thread::sleep(std::time::Duration::from_millis(10));
        super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

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

        super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        super::super::pri::vbios_pri_ring_init(&r, &w);
        super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

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
            super::super::kepler_recovery::kepler_cold_recovery(guard);
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
                let pmu_ok = super::super::pmu::gk110_pmu_boot(guard);
                tracing::info!(pmu_ok, "PMU firmware boot (for PGRAPH power domain)");
                std::thread::sleep(std::time::Duration::from_millis(50));
            } else {
                tracing::info!("PMU already running from nouveau — skipping re-boot");
            }

            // Apply privring timing before PGOB (Nouveau: gk104_privring_init)
            super::super::pri::gk104_privring_timing(&r, &w);

            // ── Power management diagnostic ──
            // The PMU-mediated PGOB (0x10a78c + 0x0205xx) has been ineffective
            // because the PMU firmware doesn't process the PGOB request.
            // Try direct PG_CTRL register (0x020004) as a bypass.
            {
                let bar0 = guard.inner();
                let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
                let wr = |reg: u32, val: u32| {
                    let _ = bar0.write_u32(reg as usize, val);
                };

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
                    gpc_alive = gpccs0_test != 0xDEAD_DEAD
                        && gpccs0_test & 0xBAD0_0000 != 0xBAD0_0000
                        && gpccs0_test != 0,
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
                    gpc_alive = gpccs0_cpuctl != 0xDEAD_DEAD
                        && gpccs0_cpuctl & 0xBAD0_0000 != 0xBAD0_0000
                        && gpccs0_cpuctl != 0,
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
            let _ = super::super::pgob::gk104_pgob_disable(guard);
            super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

            if !check_gpccs0_alive() {
                tracing::info!("gk104 PG_CTRL insufficient — trying nvidia-470 PSW");
                let _ = super::super::pgob::nvidia470_pgob_disable(guard);
                super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
            }

            if !check_gpccs0_alive() {
                tracing::info!("nvidia-470 PSW insufficient — running full gk110_pmu_pgob");
                let _ = super::super::pgob::gk110_pgob_disable(guard);
            }

            // Re-enumerate PRI ring after PGOB (GPC stations should appear)
            super::super::pri::gk104_privring_timing(&r, &w);
            super::super::pri::nouveau_pri_ring_init(&r, &w);
            super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

            // PGOB brought PGRAPH online. Read GPC/TPC topology NOW
            // before the PMC reset clears the fuse mirrors at 0x502608.
            // nouveau reads these in gf100_gr_oneinit (before mc_reset).
            {
                let bar0 = guard.inner();
                let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
                super::super::pri::clear_pri_ring_faults(bar0, &r, &w);
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
                super::super::vbios_devinit::kepler_vbios_devinit(guard.inner());
                std::thread::sleep(std::time::Duration::from_millis(50));

                // Apply clock recipe so GPC clocks are running.
                let (clk_applied, clk_skipped) =
                    super::super::kepler_clock::apply_gk110_clock_recipe(guard);
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Re-enumerate PRI ring after DEVINIT.
                {
                    let bar0 = guard.inner();
                    let rd =
                        |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
                    let wr_raw = |reg: u32, val: u32| {
                        let _ = bar0.write_u32(reg as usize, val);
                    };
                    super::super::pri::nouveau_pri_ring_init(&rd, &wr_raw);
                    super::super::pri::clear_pri_ring_faults(bar0, &rd, &wr_raw);
                }

                // Re-scan topology after DEVINIT.
                cached_tpc_counts = [(0, 0); 8];
                cached_gpc_count = 0;
                cached_tpc_total = 0;
                {
                    let bar0 = guard.inner();
                    let rd =
                        |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
                    super::super::pri::clear_pri_ring_faults(bar0, &r, &w);
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

                super::super::pri::nouveau_pri_ring_init(&rd, &wr_raw);
                super::super::pri::clear_pri_ring_faults(bar0, &rd, &wr_raw);

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

                let _ = super::super::pgob::gk110_pgob_disable(guard);

                super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
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
            let topo = super::super::pri::scan_gpc_topology(guard);
            cached_gpc_count = topo.0;
            cached_tpc_total = topo.1;
            cached_tpc_counts = topo.2;
        }

        // PGOB disable + PMC GR reset.
        // With PLLs alive from nouveau POST, the PMU is properly clocked
        // and cooperates with the PGOB disable protocol. No need to halt
        // the PMU — it manages power domains correctly when engine clocks
        // are running.
        let _ = super::super::pgob::gk110_pgob_disable(guard);
        super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

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

            super::super::pri::nouveau_pri_ring_init(&rd, &wr_raw);
            super::super::pri::clear_pri_ring_faults(bar0, &rd, &wr_raw);

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
                let _ = super::super::pgob::gk110_pgob_disable(guard);
                super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
                let gr_hub_extra = r(0x40_0000);
                tracing::info!(
                    gr_hub = format_args!("{gr_hub_extra:#010x}"),
                    "After extra PGOB disable"
                );
            }
        }

        tracing::info!(hw_gpc_count, "Proceeding to FECS/GPCCS firmware upload");
        super::super::kepler_fecs_boot::kepler_load_and_boot_fecs(
            guard,
            cached_gpc_count,
            cached_tpc_total,
            &cached_tpc_counts,
        );
        return true;
    }

    false
}
