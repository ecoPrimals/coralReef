// SPDX-License-Identifier: AGPL-3.0-or-later
//! Internal (`gf100_gr_init_ctxctl_int`) vs external (`ctxctl_ext`) STARTCPU sequencing.

use super::super::hardware_guard::GuardedBar;

pub(super) fn run_kepler_boot_protocols(
    guard: &GuardedBar<'_>,
    blobs: &super::firmware::KeplerFirmwareBlobs,
) {
    use crate::nv::kepler_falcon;

    const FECS: u32 = kepler_falcon::FECS_BASE;
    const GPCCS: u32 = kepler_falcon::GPCCS_BASE;

    let r = guard.read_fn();
    let w = |reg: u32, val: u32| {
        if let Err(refusal) = guard.write_u32(reg, val) {
            tracing::error!(%refusal, "kepler_load_and_boot_fecs: guard refused write");
        }
    };

    let use_internal_protocol = blobs.use_internal_protocol;

    const CTXSW_MAILBOX0: u32 = 0x40_9800;
    let nstations_total: u32 = 32;

    let clear_all_faults = |r: &dyn Fn(u32) -> u32, w: &dyn Fn(u32, u32)| {
        let gr_intr = r(0x40_0100);
        if gr_intr != 0 && gr_intr != 0xDEAD_DEAD && gr_intr & 0xBAD0_0000 != 0xBAD0_0000 {
            w(0x40_0100, gr_intr);
        }
        let status = r(0x12_0058);
        if status != 0 && status != 0xDEAD_DEAD {
            for s in 0..nstations_total {
                let stat = r(0x12_2120 + s * 0x800);
                if stat != 0 && stat != 0xDEAD_DEAD {
                    w(0x12_2120 + s * 0x800 + 4, 0x2);
                }
            }
            w(0x12_004C, 0x2);
        }
    };

    if use_internal_protocol {
        // ================================================================
        // INTERNAL firmware boot protocol (gf100_gr_init_ctxctl_int)
        // ================================================================
        //
        // Internal firmware (embedded in nouveau.ko) uses a different protocol:
        //   a) Upload FECS + GPCCS firmware (done above)
        //   b) Load csdata into FECS/GPCCS DMEM (register save/restore lists)
        //   c) Set FECS DMACTL = 0
        //   d) STARTCPU on FECS only (FECS starts GPCCS internally)
        //   e) Poll 0x409800 bit 31 (not bit 0)
        //
        // Internal firmware discovers GPC/TPC topology from hardware fuse
        // mirrors, so we skip the 0x409600-0x4096FF topology register writes.

        tracing::info!("Using INTERNAL firmware boot protocol (FECS-only start, poll bit 31)");

        // Load csdata — full GK110B register save/restore lists for context
        // switching, matching the POST-done path (kepler_post_done_boot_fecs).
        // Previous attempts used a stub terminator that caused FECS to trap
        // (TRAP#4 / 0x8704) because the firmware couldn't find valid register
        // lists for context save/restore.
        {
            let bar0 = guard.inner();
            let rd_fn = |off: u32| -> u32 { bar0.read_u32(off as usize).unwrap_or(0xDEAD_DEAD) };
            let wr_fn = |off: u32, val: u32| {
                let _ = bar0.write_u32(off as usize, val);
            };

            super::super::kepler_csdata::load_csdata(
                &rd_fn,
                &wr_fn,
                super::super::kepler_csdata::GK110B_GRCTX_PACK_HUB,
                FECS,
                0x000,
                0x00_0000,
            );
            super::super::kepler_csdata::load_csdata(
                &rd_fn,
                &wr_fn,
                super::super::kepler_csdata::GK110B_GRCTX_PACK_GPC_0,
                GPCCS,
                0x000,
                0x41_8000,
            );
            super::super::kepler_csdata::load_csdata(
                &rd_fn,
                &wr_fn,
                super::super::kepler_csdata::GK110B_GRCTX_PACK_GPC_1,
                GPCCS,
                0x000,
                0x41_8000,
            );
            super::super::kepler_csdata::load_csdata(
                &rd_fn,
                &wr_fn,
                super::super::kepler_csdata::GK110B_GRCTX_PACK_TPC,
                GPCCS,
                0x004,
                0x41_9800,
            );
            super::super::kepler_csdata::load_csdata(
                &rd_fn,
                &wr_fn,
                super::super::kepler_csdata::GK110B_GRCTX_PACK_PPC,
                GPCCS,
                0x008,
                0x41_BE00,
            );

            tracing::info!("Internal boot: csdata register lists loaded (5 GK110B packs)");
        }

        super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        w(0x40_0100, 0xFFFF_FFFF);

        // Verify GR HUB is still accessible before starting FECS.
        // PMU may re-enable PGOB after our disable, gating PGRAPH.
        {
            let gr_hub_check = r(0x40_0000);
            let fecs_cpuctl_pre = r(FECS + 0x100);
            let pmc_check = r(0x200);
            tracing::info!(
                gr_hub = format_args!("{gr_hub_check:#010x}"),
                fecs_cpuctl = format_args!("{fecs_cpuctl_pre:#010x}"),
                pmc = format_args!("{pmc_check:#010x}"),
                pgraph_on = pmc_check & (1 << 12) != 0,
                "Pre-STARTCPU state check"
            );

            if gr_hub_check == 0xDEAD_DEAD || gr_hub_check & 0xBAD0_0000 == 0xBAD0_0000 {
                tracing::warn!(
                    "GR HUB gated before STARTCPU — firmware likely wiped if PGOB runs now; \
                     the pre-upload PGOB disable should have prevented this"
                );
            }
        }

        // Exact Nouveau gf100_gr_init_ctxctl_int() STARTCPU sequence:
        //   nvkm_wr32(device, 0x40910c, 0x00000000)  — FECS DMACTL = 0
        //   nvkm_wr32(device, 0x409100, 0x00000002)  — FECS STARTCPU
        //
        // No BOOTVEC write (default 0 after PMC reset).
        // No ITFEN write (Nouveau v6.8 doesn't set ITFEN — falcon
        //   fetches directly from IMEM physical addressing).
        // No ENGCTL cycle or retry logic.
        {
            let cpuctl_pre = r(FECS + 0x100);
            let itfen_pre = r(FECS + 0x048);
            tracing::info!(
                cpuctl = format_args!("{cpuctl_pre:#010x}"),
                itfen = format_args!("{itfen_pre:#010x}"),
                "Pre-STARTCPU: FECS state (ITFEN should be 0x00000000)"
            );

            w(FECS + 0x10C, 0x0000_0000); // DMACTL = 0
            w(FECS + 0x100, 0x0000_0002); // STARTCPU
            std::thread::sleep(std::time::Duration::from_millis(1));

            let cpuctl_post = r(FECS + 0x100);
            let debug_pc = r(FECS + 0xC20);
            let falcon_pc = r(FECS + 0x0A4);
            let exci_post = r(FECS + 0x04C);
            let gpc0_cpuctl = r(0x50_2000 + 0x100);
            tracing::info!(
                cpuctl = format_args!("{cpuctl_post:#010x}"),
                debug_pc = format_args!("{debug_pc:#010x}"),
                falcon_pc = format_args!("{falcon_pc:#010x}"),
                exci = format_args!("{exci_post:#010x}"),
                gpc0_gpccs = format_args!("{gpc0_cpuctl:#010x}"),
                hreset = cpuctl_post & 0x10 != 0,
                "Post-STARTCPU: FECS state (debug_pc=+0xC20, falcon_pc=+0x0A4)"
            );
        }

        // Poll 0x409800 bit 31 — internal firmware ready signal.
        w(0x40_0138, 0x0000_0000);
        w(0x40_0140, 0x0000_0000);
        w(0x40_0100, 0xFFFF_FFFF);

        let mut booted = false;
        for i in 0..2000 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            clear_all_faults(&r, &w);

            if i % 50 == 0 {
                clear_all_faults(&r, &w);
                let mailbox0 = r(CTXSW_MAILBOX0);
                let cpuctl = r(FECS + 0x100);
                let gpc0_cpuctl = r(0x50_2000 + 0x100);
                let debug_pc = r(FECS + 0xC20);
                let falcon_pc = r(FECS + 0x0A4);
                let fecs_exci = r(FECS + 0x04C);

                let mailbox_ok = mailbox0 != 0xDEAD_DEAD && mailbox0 & 0xBAD0_0000 != 0xBAD0_0000;
                let cpuctl_ok = cpuctl != 0xDEAD_DEAD && cpuctl & 0xBAD0_0000 != 0xBAD0_0000;

                tracing::info!(
                    poll_ms = i,
                    mailbox0 = format_args!("{mailbox0:#010x}"),
                    cpuctl = format_args!("{cpuctl:#010x}"),
                    gpc0_gpccs = format_args!("{gpc0_cpuctl:#010x}"),
                    debug_pc = format_args!("{debug_pc:#010x}"),
                    falcon_pc = format_args!("{falcon_pc:#010x}"),
                    exci = format_args!("{fecs_exci:#010x}"),
                    "FECS boot poll (internal firmware — 0x409800 bit 31)"
                );

                if mailbox_ok && mailbox0 & 0x8000_0000 != 0 {
                    tracing::info!(
                        mailbox0 = format_args!("{mailbox0:#010x}"),
                        "FECS boot confirmed (0x409800 bit 31 set)"
                    );
                    booted = true;
                    break;
                }

                if cpuctl_ok && cpuctl & 0x10 != 0 && i > 200 {
                    tracing::warn!(
                        cpuctl = format_args!("{cpuctl:#010x}"),
                        debug_pc = format_args!("{debug_pc:#010x}"),
                        falcon_pc = format_args!("{falcon_pc:#010x}"),
                        exci = format_args!("{fecs_exci:#010x}"),
                        "FECS stuck in HRESET (0x10) — STARTCPU not consumed"
                    );
                    break;
                }
            }
        }

        if booted {
            let ctx_size = r(FECS + 0x804);
            tracing::info!(
                ctx_size = format_args!("{ctx_size:#010x}"),
                "Kepler FECS/GPCCS boot complete (internal) — GR engine ready"
            );
        } else {
            clear_all_faults(&r, &w);
            let cpuctl = r(FECS + 0x100);
            let mailbox0 = r(CTXSW_MAILBOX0);
            let scratch0 = r(kepler_falcon::FECS_SCRATCH0);
            let scratch1 = r(kepler_falcon::FECS_SCRATCH1);
            let gpc0_cpuctl = r(0x50_2000 + 0x100);
            let gr_hub = r(0x40_0000);
            let fecs_pc = r(FECS + 0x0A8);
            let fecs_exci = r(FECS + 0x04C);

            // FECS exception + TRAP diagnostic registers
            let fecs_exc_stat = r(0x40_9018);
            let fecs_trap = r(0x40_9800 + 0x070);
            let fecs_intr = r(0x40_0100);
            let fecs_mailbox1 = r(0x40_9804);
            let fecs_idlestate = r(FECS + 0x04C);
            let gpc0_pc = r(0x50_2000 + 0x0A8);
            let gpc0_exci = r(0x50_2000 + 0x04C);

            tracing::warn!(
                cpuctl = format_args!("{cpuctl:#010x}"),
                mailbox0 = format_args!("{mailbox0:#010x}"),
                mailbox1 = format_args!("{fecs_mailbox1:#010x}"),
                gr_hub = format_args!("{gr_hub:#010x}"),
                scratch0 = format_args!("{scratch0:#010x}"),
                scratch1 = format_args!("{scratch1:#010x}"),
                gpc0_gpccs = format_args!("{gpc0_cpuctl:#010x}"),
                pc = format_args!("{fecs_pc:#010x}"),
                exci = format_args!("{fecs_exci:#010x}"),
                exc_stat = format_args!("{fecs_exc_stat:#010x}"),
                trap = format_args!("{fecs_trap:#010x}"),
                gr_intr = format_args!("{fecs_intr:#010x}"),
                idlestate = format_args!("{fecs_idlestate:#010x}"),
                gpc0_pc = format_args!("{gpc0_pc:#010x}"),
                gpc0_exci = format_args!("{gpc0_exci:#010x}"),
                "Kepler FECS did not reach ready state (internal — 0x409800 bit 31)"
            );

            if fecs_exci != 0 || fecs_exc_stat != 0 {
                tracing::error!(
                    exci = format_args!("{fecs_exci:#010x}"),
                    exc_stat = format_args!("{fecs_exc_stat:#010x}"),
                    pc = format_args!("{fecs_pc:#010x}"),
                    is_trap4 = fecs_exc_stat == 0x0000_8704,
                    "FECS exception — if TRAP#4 (0x8704), csdata/GR init is likely incomplete"
                );
            }
        }
    } else {
        // ================================================================
        // EXTERNAL firmware boot protocol (gf100_gr_init_ctxctl_ext)
        // ================================================================
        //
        // External firmware requires:
        //   a) CTXSW_MAILBOX0 (0x409800) = 0
        //   b) Topology registers at 0x409600-0x4096FF
        //   c) GPCCS DMACTL = 0, FECS DMACTL = 0
        //   d) Start GPCCS first, then FECS
        //   e) Poll CTXSW_MAILBOX0 bit 0

        tracing::info!("Using EXTERNAL firmware boot protocol (nouveau gf100_gr_init_ctxctl_ext)");

        // Exact Nouveau gf100_gr_init_ctxctl_ext() ordering:
        //   1. Upload firmware (done above, with unk260 bracket)
        //   2. wr(0x409800, 0) — clear CTXSW_MAILBOX0 BEFORE start
        //   3. wr(0x41a10c, 0) — clear GPCCS DMACTL (broadcast)
        //   4. wr(0x40910c, 0) — clear FECS DMACTL
        //   5. nvkm_falcon_start(gpccs) — start GPCCS
        //   6. nvkm_falcon_start(fecs) — start FECS
        //   7. Poll 0x409800 bit 0
        //
        // NO ENGCTL cycle, NO ITFEN write, NO BOOTVEC write.
        // External firmware discovers topology from hardware fuse mirrors.

        super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        w(0x40_0100, 0xFFFF_FFFF); // GR_INTR: clear all

        // NOTE: Per-GPC clock gating (BLCG/SLCG) and GPC MMU init are
        // already applied in Step 3a/3c before firmware upload. Do NOT
        // re-apply them here — writing BLCG=0x42 after firmware upload
        // enables block-level clock gating on the GPCCS falcon CPU, which
        // prevents STARTCPU from being consumed on GK210B.

        // Nouveau gf100_gr_init_ctxctl_ext() — exact sequence:
        //   wr(0x409800, 0)  — clear CTXSW_MAILBOX0
        //   wr(0x41a10c, 0)  — GPCCS DMACTL = 0 (broadcast)
        //   wr(0x40910c, 0)  — FECS DMACTL = 0
        //   start(gpccs)     — GPCCS STARTCPU (broadcast)
        //   start(fecs)      — FECS STARTCPU
        //   poll(0x409800)   — wait for bit 0
        //
        // Nouveau v6.8 does NOT set ITFEN — falcons fetch from IMEM directly.
        // Do NOT write clock gating — BLCG was applied in Step 3c; re-applying
        // per-GPC after upload gates the falcon CPU clock on GK210B.

        w(CTXSW_MAILBOX0, 0x0000_0000);

        // DMACTL = 0 (per-GPC for GPCCS since broadcast is dropped on GK210B)
        w(FECS + 0x10C, 0x0000_0000);
        let mut gpc_count = 0u32;
        for gpc in 0..8u32 {
            let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
            let probe = r(gpccs_base + 0x100);
            if probe == 0xDEAD_DEAD || probe & 0xBAD0_0000 == 0xBAD0_0000 {
                continue;
            }
            w(gpccs_base + 0x10C, 0x0000_0000);
            gpc_count += 1;
        }

        {
            let fecs_state = (r(FECS + 0x100), r(FECS + 0x048), r(FECS + 0x10C));
            let gpc0_state = (
                r(0x50_2000 + 0x100),
                r(0x50_2000 + 0x048),
                r(0x50_2000 + 0x10C),
            );
            tracing::info!(
                gpc_count,
                fecs_cpuctl = format_args!("{:#010x}", fecs_state.0),
                fecs_itfen = format_args!("{:#010x}", fecs_state.1),
                fecs_dmactl = format_args!("{:#010x}", fecs_state.2),
                gpc0_cpuctl = format_args!("{:#010x}", gpc0_state.0),
                gpc0_itfen = format_args!("{:#010x}", gpc0_state.1),
                gpc0_dmactl = format_args!("{:#010x}", gpc0_state.2),
                "Pre-STARTCPU state (DMACTL=0, ITFEN=0 — Nouveau v6.8 mode)"
            );
        }

        // Disable GPCCS BLCG before STARTCPU.
        // Step 3c applied BLCG=0x42 via broadcast (0x41a890), which DOES reach
        // per-GPC GPCCS on GK210B (correcting earlier assumption that broadcast
        // writes are dropped). BLCG=0x42 gates the falcon CPU clock, making
        // STARTCPU silently ignored. Clear BLCG to 0 so the CPU clock runs.
        {
            let bar0 = guard.inner();
            let mut ungated = 0u32;
            for gpc in 0..8u32 {
                let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
                let probe = bar0
                    .read_u32((gpccs_base + 0x100) as usize)
                    .unwrap_or(0xDEAD_DEAD);
                if probe == 0xDEAD_DEAD || probe & 0xBAD0_0000 == 0xBAD0_0000 {
                    continue;
                }
                let _ = bar0.write_u32((gpccs_base + 0x890) as usize, 0x0000_0000);
                let _ = bar0.write_u32((gpccs_base + 0x8b0) as usize, 0x0000_0000);
                ungated += 1;
            }
            let gpc0_blcg = bar0.read_u32(0x50_2890_usize).unwrap_or(0xDEAD_DEAD);
            tracing::info!(
                ungated,
                gpc0_blcg = format_args!("{gpc0_blcg:#010x}"),
                "GPCCS BLCG disabled per-GPC (was 0x42 from Step 3c broadcast)"
            );
        }

        // Start GPCCS per-GPC via raw bar0
        {
            let bar0 = guard.inner();
            for gpc in 0..8u32 {
                let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
                let probe = bar0
                    .read_u32((gpccs_base + 0x100) as usize)
                    .unwrap_or(0xDEAD_DEAD);
                if probe == 0xDEAD_DEAD || probe & 0xBAD0_0000 == 0xBAD0_0000 {
                    continue;
                }
                let _ = bar0.write_u32((gpccs_base + 0x100) as usize, 0x0000_0002);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
            let gpc0_cpuctl = bar0.read_u32(0x50_2100_usize).unwrap_or(0xDEAD_DEAD);
            tracing::info!(
                gpc0_cpuctl = format_args!("{gpc0_cpuctl:#010x}"),
                started = gpc0_cpuctl & 0x10 == 0,
                "GPCCS after STARTCPU (BLCG disabled)"
            );
        }

        // Start FECS
        w(FECS + 0x100, 0x0000_0002); // STARTCPU

        std::thread::sleep(std::time::Duration::from_millis(5));
        {
            let fecs_cpuctl = r(FECS + 0x100);
            let fecs_idle = r(FECS + 0x04C);
            let fecs_pc = r(0x40_9C20);
            let gpc0_cpuctl = r(0x50_2000 + 0x100);
            let gpc0_pc = r(0x50_2000 + 0x0C20);
            let gpc0_exci = r(0x50_2000 + 0x04C);
            let mailbox0 = r(CTXSW_MAILBOX0);
            tracing::info!(
                fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
                fecs_idle = format_args!("{fecs_idle:#010x}"),
                fecs_pc = format_args!("{fecs_pc:#010x}"),
                gpc0_cpuctl = format_args!("{gpc0_cpuctl:#010x}"),
                gpc0_pc = format_args!("{gpc0_pc:#010x}"),
                gpc0_exci = format_args!("{gpc0_exci:#010x}"),
                mailbox0 = format_args!("{mailbox0:#010x}"),
                "Post-STARTCPU state (5ms after both falcons started)"
            );
        }

        // Poll CTXSW_MAILBOX0 bit 0.
        w(0x40_0138, 0x0000_0000);
        w(0x40_0140, 0x0000_0000);
        w(0x40_0100, 0xFFFF_FFFF);

        let mut booted = false;
        for i in 0..2000 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            clear_all_faults(&r, &w);

            if i % 50 == 0 {
                clear_all_faults(&r, &w);
                let mailbox0 = r(CTXSW_MAILBOX0);
                let cpuctl = r(FECS + 0x100);
                let gpc0_cpuctl = r(0x50_2000 + 0x100);
                let fecs_pc = r(FECS + 0x0A8);
                let fecs_exci = r(FECS + 0x04C);

                let mailbox_ok = mailbox0 != 0xDEAD_DEAD && mailbox0 & 0xBAD0_0000 != 0xBAD0_0000;
                let cpuctl_ok = cpuctl != 0xDEAD_DEAD && cpuctl & 0xBAD0_0000 != 0xBAD0_0000;

                tracing::info!(
                    poll_ms = i,
                    mailbox0 = format_args!("{mailbox0:#010x}"),
                    cpuctl = format_args!("{cpuctl:#010x}"),
                    gpc0_gpccs = format_args!("{gpc0_cpuctl:#010x}"),
                    pc = format_args!("{fecs_pc:#010x}"),
                    exci = format_args!("{fecs_exci:#010x}"),
                    "FECS boot poll (external firmware — CTXSW_MAILBOX0 bit 0)"
                );

                if mailbox_ok && mailbox0 & 0x01 != 0 {
                    tracing::info!(
                        mailbox0 = format_args!("{mailbox0:#010x}"),
                        "FECS boot confirmed (CTXSW_MAILBOX0 bit 0 set)"
                    );
                    booted = true;
                    break;
                }

                if cpuctl_ok && cpuctl & 0x10 != 0 && i > 200 {
                    tracing::warn!(
                        cpuctl = format_args!("{cpuctl:#010x}"),
                        pc = format_args!("{fecs_pc:#010x}"),
                        exci = format_args!("{fecs_exci:#010x}"),
                        "FECS stuck in HRESET (0x10) — STARTCPU not consumed"
                    );
                    break;
                }
            }
        }

        if booted {
            let ctx_size = r(FECS + 0x804);
            tracing::info!(
                ctx_size = format_args!("{ctx_size:#010x}"),
                "Kepler FECS/GPCCS boot complete (external) — GR engine ready"
            );
        } else {
            clear_all_faults(&r, &w);
            let cpuctl = r(FECS + 0x100);
            let mailbox0 = r(CTXSW_MAILBOX0);
            let scratch0 = r(kepler_falcon::FECS_SCRATCH0);
            let scratch1 = r(kepler_falcon::FECS_SCRATCH1);
            let gpc0_cpuctl = r(0x50_2000 + 0x100);
            let gr_hub = r(0x40_0000);
            let fecs_pc = r(FECS + 0x0A8);
            let fecs_exci = r(FECS + 0x04C);

            tracing::warn!(
                cpuctl = format_args!("{cpuctl:#010x}"),
                mailbox0 = format_args!("{mailbox0:#010x}"),
                gr_hub = format_args!("{gr_hub:#010x}"),
                scratch0 = format_args!("{scratch0:#010x}"),
                scratch1 = format_args!("{scratch1:#010x}"),
                gpc0_gpccs = format_args!("{gpc0_cpuctl:#010x}"),
                pc = format_args!("{fecs_pc:#010x}"),
                exci = format_args!("{fecs_exci:#010x}"),
                "Kepler FECS did not reach ready state (external — CTXSW_MAILBOX0 bit 0)"
            );
        }
    }
}
