// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kepler POST-done and cold-path FECS/GPCCS boot — GK110/GK210 firmware upload and start.
//!
//! Split from [`super::init`] for readability. Implements Nouveau-aligned GR init ordering,
//! internal vs external firmware protocols, and topology-aware register setup.

/// POST-done FECS boot using Nouveau's external firmware protocol.
///
/// After Nouveau POST + unbind, PGRAPH is powered but GR HUB may have
/// stale PRI ring faults from Nouveau's shutdown. These faults cause
/// register accesses to return `0xbadf1002`, making FECS firmware trap
/// on its first GR HUB access.
///
/// Strategy:
/// 1. Clear PRI ring faults (Nouveau's `gk104_privring_intr`)
/// 2. If GR HUB still faulted, PMC PGRAPH toggle + re-clear
/// 3. Upload GK210 external firmware (no ENGCTL reset — it prevents STARTCPU)
/// 4. Start GPCCS then FECS (Nouveau's `gf100_gr_init_ctxctl_ext` order)
/// 5. Poll 0x409800 bit 0 (external firmware ready protocol)
pub(super) fn kepler_post_done_boot_fecs(
    guard: &super::hardware_guard::GuardedBar<'_>,
    cached_gpc_count: u32,
    cached_tpc_total: u32,
    _cached_tpc_counts: &[(u32, u32); 8],
) {
    use crate::nv::kepler_falcon;

    const FECS: u32 = kepler_falcon::FECS_BASE;
    const GPCCS: u32 = kepler_falcon::GPCCS_BASE;

    let bar0 = guard.inner();
    let rd = |off: u32| -> u32 { bar0.read_u32(off as usize).unwrap_or(0xDEAD_DEAD) };
    let wr = |off: u32, val: u32| {
        let _ = bar0.write_u32(off as usize, val);
    };

    // ── Step 0: Try warm restart — use Nouveau's already-loaded firmware ──
    // After warm-catch, Nouveau has fully initialized GR + loaded internal
    // firmware + loaded csdata. The Falcon is stopped (CPUCTL=0x10). We can
    // try just restarting it without any reset/re-upload.
    {
        let f_cpuctl = rd(FECS + 0x100);
        let f_itfen = rd(FECS + 0x048);
        let g_cpuctl = rd(GPCCS + 0x100);
        let mailbox0 = rd(0x40_9800);
        tracing::info!(
            fecs_cpuctl = format_args!("{f_cpuctl:#010x}"),
            fecs_itfen = format_args!("{f_itfen:#010x}"),
            gpccs_cpuctl = format_args!("{g_cpuctl:#010x}"),
            mailbox0 = format_args!("{mailbox0:#010x}"),
            "POST-done: warm restart attempt — Nouveau state"
        );

        if f_cpuctl & 0x10 != 0 {
            // FECS is stopped — restore GR power and restart with Nouveau's FW

            // Ungate PGOB to restore GR power (Nouveau disables on teardown)
            super::pgob::gk110_pgob_ungate_only(guard);

            // Clear PRI ring faults from PGOB
            wr(0x12_004c, 0x2);
            for _ in 0..500 {
                std::thread::sleep(std::time::Duration::from_millis(1));
                if rd(0x12_004c) & 0x3f == 0 {
                    break;
                }
            }

            // Enable ITFEN on FECS + GPCCS (cleared by Nouveau teardown)
            wr(FECS + 0x048, 0x3);
            wr(GPCCS + 0x048, 0x3);
            for gpc in 0..cached_gpc_count.min(8) {
                wr(0x50_0000 + gpc * 0x8000 + 0x2000 + 0x048, 0x3);
            }

            // Internal protocol: FECS DMACTL=0, start via CPUCTL
            wr(FECS + 0x10C, 0x0);
            wr(0x40_9800, 0x0);
            wr(FECS + 0x100, 0x2);

            std::thread::sleep(std::time::Duration::from_millis(200));

            let post_cpuctl = rd(FECS + 0x100);
            let post_pc = rd(FECS + 0x0A8);
            let post_idle = rd(FECS + 0x04C);
            let post_mb = rd(0x40_9800);
            let post_exci = rd(FECS + 0x018);
            tracing::info!(
                cpuctl = format_args!("{post_cpuctl:#010x}"),
                pc = format_args!("{post_pc:#010x}"),
                idle = format_args!("{post_idle:#010x}"),
                mailbox = format_args!("{post_mb:#010x}"),
                exci = format_args!("{post_exci:#010x}"),
                "POST-done: warm restart result"
            );

            if post_mb & 0x8000_0000 != 0 {
                let ctx_size = rd(0x40_9804);
                tracing::info!(
                    ctx_size = format_args!("{ctx_size:#010x}"),
                    gpcs = cached_gpc_count,
                    tpcs = cached_tpc_total,
                    "POST-done: WARM RESTART SUCCESS — FECS booted with Nouveau FW"
                );
                return;
            }
            tracing::warn!("POST-done: warm restart failed — proceeding with full init");
        }
    }

    // ── Step 1: Diagnose initial state ──
    let pmc_enable = rd(0x200);
    let gr_hub_initial = rd(0x40_0000);
    let fecs_cpuctl = rd(FECS + 0x100);
    let gpccs_cpuctl = rd(GPCCS + 0x100);
    let fecs_sctl = rd(FECS + 0x240);
    tracing::info!(
        pmc_enable = format_args!("{pmc_enable:#010x}"),
        pgraph_on = pmc_enable & (1 << 12) != 0,
        gr_hub = format_args!("{gr_hub_initial:#010x}"),
        fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
        gpccs_cpuctl = format_args!("{gpccs_cpuctl:#010x}"),
        fecs_sctl = format_args!("{fecs_sctl:#010x}"),
        fecs_secret = (fecs_sctl >> 15) & 1,
        "POST-done: initial state"
    );

    // ── Step 1b: Read Nouveau-loaded FECS IMEM before we touch anything ──
    // Nouveau uploads internal firmware during POST. Capture it so we can
    // compare with our local firmware files and detect mismatches.
    {
        wr(FECS + 0x180, (1 << 25)); // IMEMC: offset 0, auto-increment read
        let mut nouveau_imem = Vec::with_capacity(16);
        for _ in 0..16 {
            nouveau_imem.push(rd(FECS + 0x184));
        }
        tracing::info!(
            w0 = format_args!("{:#010x}", nouveau_imem[0]),
            w1 = format_args!("{:#010x}", nouveau_imem[1]),
            w2 = format_args!("{:#010x}", nouveau_imem[2]),
            w3 = format_args!("{:#010x}", nouveau_imem[3]),
            "POST-done: Nouveau FECS IMEM[0..3] (pre-reset capture)"
        );

        // Read full Nouveau FECS IMEM (up to 4096 bytes = 1024 words)
        wr(FECS + 0x180, (1 << 25));
        let mut nouveau_fecs_imem = Vec::with_capacity(1024);
        for _ in 0..1024 {
            let w = rd(FECS + 0x184);
            if w == 0xDEAD_DEAD {
                break;
            }
            nouveau_fecs_imem.push(w);
        }
        // Trim trailing zeros
        while nouveau_fecs_imem.last() == Some(&0) {
            nouveau_fecs_imem.pop();
        }
        let imem_bytes = nouveau_fecs_imem.len() * 4;
        tracing::info!(
            words = nouveau_fecs_imem.len(),
            bytes = imem_bytes,
            "POST-done: Nouveau FECS IMEM total captured"
        );

        // Save to temp file for comparison
        let imem_flat: Vec<u8> = nouveau_fecs_imem
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        if let Err(e) = std::fs::write("/tmp/nouveau_fecs_imem.bin", &imem_flat) {
            tracing::warn!(error = %e, "Failed to save Nouveau FECS IMEM dump");
        } else {
            tracing::info!(
                bytes = imem_flat.len(),
                "POST-done: Nouveau FECS IMEM saved to /tmp/nouveau_fecs_imem.bin"
            );
        }

        // Also capture FECS DMEM (includes firmware data + csdata)
        wr(FECS + 0x1C0, 1 << 25); // DMEMC: offset 0, auto-increment read
        let mut fecs_dmem = Vec::with_capacity(512);
        for _ in 0..512 {
            fecs_dmem.push(rd(FECS + 0x1C4));
        }
        let dmem_flat: Vec<u8> = fecs_dmem.iter().flat_map(|w| w.to_le_bytes()).collect();
        let _ = std::fs::write("/tmp/nouveau_fecs_dmem.bin", &dmem_flat);
        tracing::info!(
            bytes = dmem_flat.len(),
            first_word = format_args!("{:#010x}", fecs_dmem[0]),
            second_word = format_args!("{:#010x}", fecs_dmem[1]),
            "POST-done: Nouveau FECS DMEM saved"
        );

        // Capture GPCCS IMEM via per-GPC instance 0 (broadcast may not read back)
        {
            let gpc0_gpccs = 0x50_2000u32;
            wr(gpc0_gpccs + 0x180, 1 << 25);
            let mut gpccs_imem = Vec::with_capacity(512);
            for _ in 0..512 {
                gpccs_imem.push(rd(gpc0_gpccs + 0x184));
            }
            while gpccs_imem.last() == Some(&0) {
                gpccs_imem.pop();
            }
            let gpccs_flat: Vec<u8> = gpccs_imem.iter().flat_map(|w| w.to_le_bytes()).collect();
            let _ = std::fs::write("/tmp/nouveau_gpccs_imem.bin", &gpccs_flat);
            tracing::info!(
                words = gpccs_imem.len(),
                bytes = gpccs_flat.len(),
                "POST-done: Nouveau GPCCS IMEM saved"
            );
        }
    }

    // ── Step 2: Full Nouveau gf100_gr_init sequence ──
    // Previous attempts tried to preserve Nouveau's POST state, but the
    // Falcons retain stale execution state that causes immediate PC=0 traps.
    // Nouveau's actual sequence: PMC PGRAPH reset → PGOB ungate → GR MMIO
    // init → firmware upload → start. Follow this exactly.

    // Step 2a: PMC PGRAPH reset (toggle bit 12) — clean-resets both Falcons
    {
        let pmc = rd(0x200);
        wr(0x200, pmc & !(1u32 << 12));
        rd(0x200); // flush
        std::thread::sleep(std::time::Duration::from_millis(5));
        wr(0x200, pmc | (1u32 << 12));
        rd(0x200); // flush
        std::thread::sleep(std::time::Duration::from_millis(50));
        tracing::info!(
            pmc_before = format_args!("{pmc:#010x}"),
            pmc_after = format_args!("{:#010x}", rd(0x200)),
            "POST-done: PMC PGRAPH reset (bit 12 toggle)"
        );
    }

    // Step 2b: Clear PRI ring faults generated by the PMC reset
    {
        let intr0 = rd(0x12_0058);
        let intr1 = rd(0x12_005c);
        let hubnr = rd(0x12_0070);
        let ropnr = rd(0x12_0074);
        let gpcnr = rd(0x12_0078);
        tracing::info!(
            intr0 = format_args!("{intr0:#010x}"),
            intr1 = format_args!("{intr1:#010x}"),
            hubnr,
            ropnr,
            gpcnr,
            "POST-done: PRI ring status after PMC reset"
        );

        wr(0x12_004c, 0x2);
        for i in 0..2000 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            if rd(0x12_004c) & 0x3f == 0 {
                tracing::info!(wait_ms = i, "POST-done: PRI ring faults cleared");
                break;
            }
        }
    }

    // Step 2c: PGOB ungate — restore power to GR subunits after PMC reset
    super::pgob::gk110_pgob_ungate_only(guard);
    {
        let is_fault = |v: u32| v == 0xDEAD_DEAD || v & 0xBAD0_0000 == 0xBAD0_0000;
        let h80 = rd(0x40_0080);
        let h100 = rd(0x40_0100);
        let h500 = rd(0x40_0500);
        tracing::info!(
            h80 = format_args!("{h80:#010x}"),
            h100 = format_args!("{h100:#010x}"),
            h500 = format_args!("{h500:#010x}"),
            ok = !is_fault(h80) || !is_fault(h100) || !is_fault(h500),
            "POST-done: GR HUB after PMC reset + PGOB ungate"
        );
    }

    // Step 2d: Clear any new PRI faults from PGOB
    {
        wr(0x12_004c, 0x2);
        for _ in 0..2000 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            if rd(0x12_004c) & 0x3f == 0 {
                break;
            }
        }
    }

    // Step 3: GR MMIO init (Nouveau's gf100_gr_init ordering)
    {
        wr(0x40_0500, 0x0000_0000); // disable traps (Nouveau's first GR write)

        // GPC MMU init (gf100_gr_init_gpc_mmu)
        let fb_mmu = rd(0x10_0C80) & 0x0000_0001;
        wr(0x41_8880, fb_mmu);
        wr(0x41_8890, 0x0000_0000);
        wr(0x41_8894, 0x0000_0000);

        // GK110 MMIO pack (gk110_gr_pack_mmio)
        let (gr_applied, gr_faulted) = super::kepler_gr_init::apply_gk110_gr_init(guard);
        tracing::info!(
            gr_applied,
            gr_faulted,
            "POST-done: GR MMIO init (gk110 pack)"
        );

        // sw_nonctx.bin — GK210B-specific register overrides
        let (nonctx_applied, nonctx_skipped) = super::pri::apply_sw_nonctx(guard, "gk210");
        tracing::info!(
            nonctx_applied,
            nonctx_skipped,
            "POST-done: sw_nonctx.bin GK210B overrides"
        );

        // Clear PRI faults from MMIO init
        wr(0x12_004c, 0x2);
        for _ in 0..2000 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            if rd(0x12_004c) & 0x3f == 0 {
                break;
            }
        }

        // Trap re-enable + interrupt clearing
        wr(0x40_0500, 0x0001_0001);
        wr(0x40_0100, 0xFFFF_FFFF);
        wr(0x40_013c, 0xFFFF_FFFF);
        wr(0x40_0124, 0x0000_0002);

        // Exception handling + HWW ESR
        wr(0x40_4000, 0xc000_0000);
        wr(0x40_4600, 0xc000_0000);
        wr(0x40_8030, 0xc000_0000);
        wr(0x40_6018, 0xc000_0000);
        wr(0x40_4490, 0xc000_0000);
        wr(0x40_5840, 0xc000_0000);
        wr(0x40_5844, 0x00ff_ffff);

        wr(0x40_0108, 0xFFFF_FFFF);
        wr(0x40_0138, 0xFFFF_FFFF);
        wr(0x40_0118, 0xFFFF_FFFF);
        wr(0x40_0130, 0xFFFF_FFFF);
        wr(0x40_011c, 0xFFFF_FFFF);
        wr(0x40_0134, 0xFFFF_FFFF);

        // GR_UNITS (0x400054) from live topology
        let mut gr_units: u32 = 0;
        for gpc in 0..8u32 {
            let tpc_reg = rd(0x50_0000 + gpc * 0x8000 + 0x2608);
            if tpc_reg != 0xDEAD_DEAD && tpc_reg & 0xBAD0_0000 != 0xBAD0_0000 {
                let tpc_cnt = tpc_reg & 0xFF;
                gr_units |= (tpc_cnt & 0xF) << (gpc * 4);
            }
        }
        wr(0x40_0054, gr_units);
        tracing::info!(
            gr_units = format_args!("{gr_units:#010x}"),
            "POST-done: GR_UNITS written"
        );
    }

    // Step 3b: Per-GPC and per-TPC exception init (Nouveau gf100_gr_init lines 2427-2444)
    // These writes are between the basic MMIO init and firmware start in Nouveau.
    {
        for gpc in 0..cached_gpc_count.min(8) {
            let gpc_base = 0x50_0000 + gpc * 0x8000;
            wr(gpc_base + 0x0420, 0xc000_0000);
            wr(gpc_base + 0x0900, 0xc000_0000);
            wr(gpc_base + 0x1028, 0xc000_0000);
            wr(gpc_base + 0x0824, 0xc000_0000);

            let tpc_reg = rd(gpc_base + 0x2608);
            let tpc_cnt = if tpc_reg != 0xDEAD_DEAD && tpc_reg & 0xBAD0_0000 != 0xBAD0_0000 {
                tpc_reg & 0xFF
            } else {
                0
            };
            for tpc in 0..tpc_cnt.min(5) {
                let tpc_base = gpc_base + 0x4000 + tpc * 0x800;
                wr(tpc_base + 0x508, 0xFFFF_FFFF);
                wr(tpc_base + 0x50c, 0xFFFF_FFFF);
                wr(tpc_base + 0x084, 0xc000_0000);
            }
            wr(gpc_base + 0x2c90, 0xFFFF_FFFF);
            wr(gpc_base + 0x2c94, 0xFFFF_FFFF);
        }
    }

    // Step 3c: FECS exceptions init (gf100_gr_init_fecs_exceptions)
    wr(0x40_9C24, 0x000f_0000);

    // Step 3d: ROP exceptions (gf100_gr_init_rop_exceptions)
    wr(0x40_8030, 0xc000_0000);

    // Step 3e: Clear PRI faults from per-GPC writes
    {
        wr(0x12_004c, 0x2);
        for _ in 0..2000 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            if rd(0x12_004c) & 0x3f == 0 {
                break;
            }
        }
    }

    // Step 4: Verify Falcon state after full GR init
    {
        let f_cpuctl = rd(FECS + 0x100);
        let f_itfen = rd(FECS + 0x048);
        let g_cpuctl = rd(GPCCS + 0x100);
        let g_itfen = rd(GPCCS + 0x048);
        let fecs_exc = rd(0x40_9C24);
        tracing::info!(
            fecs_cpuctl = format_args!("{f_cpuctl:#010x}"),
            fecs_itfen = format_args!("{f_itfen:#010x}"),
            gpccs_cpuctl = format_args!("{g_cpuctl:#010x}"),
            gpccs_itfen = format_args!("{g_itfen:#010x}"),
            fecs_exc = format_args!("{fecs_exc:#010x}"),
            "POST-done: Falcon state after full GR init"
        );
        for gpc in 0..cached_gpc_count.min(5) {
            let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
            let cpuctl = rd(gpccs_base + 0x100);
            let itfen = rd(gpccs_base + 0x048);
            tracing::info!(
                gpc,
                cpuctl = format_args!("{cpuctl:#010x}"),
                itfen = format_args!("{itfen:#010x}"),
                "POST-done: per-GPC GPCCS after full GR init"
            );
        }
    }

    // ── Step 5: Upload INTERNAL GK110 firmware ──
    // Nouveau defaults to internal firmware for GK110B/GK210 (NvGrUseFW=false).
    // Internal firmware (3KB FECS code) uses gf100_gr_init_ctxctl_int protocol:
    //   - Upload FECS DMEM+IMEM, then GPCCS DMEM+IMEM (broadcast)
    //   - Start FECS only (FECS manages GPCCS)
    //   - Poll 0x409800 for bit 31 (not bit 0)
    let fw_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/firmware/gk210");
    let try_read =
        |name: &str| -> Option<Vec<u8>> { std::fs::read(format!("{fw_dir}/{name}")).ok() };

    let (Some(fecs_code), Some(fecs_data), Some(gpccs_code), Some(gpccs_data)) = (
        try_read("gk110_internal_fecs_code.bin"),
        try_read("gk110_internal_fecs_data.bin"),
        try_read("gk110_internal_gpccs_code.bin"),
        try_read("gk110_internal_gpccs_data.bin"),
    ) else {
        tracing::warn!("POST-done boot: missing GK110 internal firmware");
        return;
    };

    tracing::info!(
        fecs_code = fecs_code.len(),
        fecs_data = fecs_data.len(),
        gpccs_code = gpccs_code.len(),
        gpccs_data = gpccs_data.len(),
        "POST-done boot: firmware loaded (gk110-internal)"
    );

    // Enable ITFEN on FECS + GPCCS before upload (cleared by PMC reset)
    wr(FECS + 0x048, 0x3);
    wr(GPCCS + 0x048, 0x3);
    for gpc in 0..cached_gpc_count.min(8) {
        let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
        wr(gpccs_base + 0x048, 0x3);
    }

    {
        let regs: &mut dyn crate::gsp::RegisterAccess = &mut GuardedBarRegAccess(guard);
        regs.write_u32(0x260, 0).ok(); // mc_unk260(0) — disable method dispatch

        // gf100_gr_init_ctxctl_int order: FECS DMEM+IMEM, then GPCCS DMEM+IMEM
        if let Err(e) = kepler_falcon::upload_dmem(regs, FECS, 0, &fecs_data) {
            tracing::warn!(error = %e, "FECS DMEM upload failed");
            return;
        }
        if let Err(e) = kepler_falcon::upload_imem(regs, FECS, 0, &fecs_code) {
            tracing::warn!(error = %e, "FECS IMEM upload failed");
            return;
        }
        if let Err(e) = kepler_falcon::upload_dmem(regs, GPCCS, 0, &gpccs_data) {
            tracing::warn!(error = %e, "GPCCS DMEM upload failed");
            return;
        }
        if let Err(e) = kepler_falcon::upload_imem(regs, GPCCS, 0, &gpccs_code) {
            tracing::warn!(error = %e, "GPCCS IMEM upload failed");
            return;
        }

        regs.write_u32(0x260, 1).ok(); // mc_unk260(1) — re-enable method dispatch
    }

    // Verify FECS IMEM readback
    {
        wr(FECS + 0x180, 1 << 25);
        let rb = rd(FECS + 0x184);
        let exp = u32::from_le_bytes([fecs_code[0], fecs_code[1], fecs_code[2], fecs_code[3]]);
        tracing::info!(
            readback = format_args!("{rb:#010x}"),
            expected = format_args!("{exp:#010x}"),
            ok = rb == exp,
            "POST-done boot: FECS IMEM verify"
        );
    }

    // Verify GPCCS broadcast IMEM upload reached per-GPC instances
    {
        let gpc0_base: u32 = 0x50_2000;
        wr(gpc0_base + 0x180, 1 << 25);
        let rb = rd(gpc0_base + 0x184);
        let exp = u32::from_le_bytes([gpccs_code[0], gpccs_code[1], gpccs_code[2], gpccs_code[3]]);
        tracing::info!(
            readback = format_args!("{rb:#010x}"),
            expected = format_args!("{exp:#010x}"),
            ok = rb == exp,
            "POST-done boot: GPC0 GPCCS IMEM verify"
        );
    }

    // ── Step 5b: Load csdata register lists (gf100_gr_init_csdata ×5) ──
    // Internal firmware reads context-switch register lists from DMEM on boot.
    // Without these, the firmware TRAPs (exci=0x00008704 = TRAP#4).
    {
        use super::kepler_csdata;
        let rd_fn = |off: u32| -> u32 { bar0.read_u32(off as usize).unwrap_or(0xDEAD_DEAD) };
        let wr_fn = |off: u32, val: u32| {
            let _ = bar0.write_u32(off as usize, val);
        };

        kepler_csdata::load_csdata(
            &rd_fn,
            &wr_fn,
            kepler_csdata::GK110B_GRCTX_PACK_HUB,
            0x40_9000,
            0x000,
            0x00_0000,
        );
        kepler_csdata::load_csdata(
            &rd_fn,
            &wr_fn,
            kepler_csdata::GK110B_GRCTX_PACK_GPC_0,
            0x41_A000,
            0x000,
            0x41_8000,
        );
        kepler_csdata::load_csdata(
            &rd_fn,
            &wr_fn,
            kepler_csdata::GK110B_GRCTX_PACK_GPC_1,
            0x41_A000,
            0x000,
            0x41_8000,
        );
        kepler_csdata::load_csdata(
            &rd_fn,
            &wr_fn,
            kepler_csdata::GK110B_GRCTX_PACK_TPC,
            0x41_A000,
            0x004,
            0x41_9800,
        );
        kepler_csdata::load_csdata(
            &rd_fn,
            &wr_fn,
            kepler_csdata::GK110B_GRCTX_PACK_PPC,
            0x41_A000,
            0x008,
            0x41_BE00,
        );

        tracing::info!("POST-done boot: csdata register lists loaded (5 packs)");
    }

    // ── Step 6: Start FECS (internal protocol — FECS only, it manages GPCCS) ──
    // Matches gf100_gr_init_ctxctl_int exactly:
    //   nvkm_wr32(device, 0x40910c, 0x00000000);  // FECS DMACTL = 0
    //   nvkm_wr32(device, 0x409100, 0x00000002);  // STARTCPU via CPUCTL
    //   poll 0x409800 for bit 31
    wr(FECS + 0x10C, 0x0000_0000); // FECS DMACTL = 0

    let fecs_itfen = rd(FECS + 0x048);
    let fecs_ctl = rd(FECS + 0x100);
    tracing::info!(
        fecs_ctl = format_args!("{fecs_ctl:#010x}"),
        fecs_itfen = format_args!("{fecs_itfen:#010x}"),
        "POST-done boot: FECS pre-start (internal protocol)"
    );

    // Internal protocol starts via CPUCTL directly (0x409100 = 0x2)
    wr(FECS + 0x100, 0x2);

    // Fine-grained polling: track firmware execution progress
    for delay_us in [10, 50, 100, 500, 1000, 5000, 10000u64] {
        std::thread::sleep(std::time::Duration::from_micros(delay_us));
        let c = rd(FECS + 0x100);
        let p = rd(FECS + 0x0A8);
        let i = rd(FECS + 0x04C);
        let e = rd(FECS + 0x018);
        let s = rd(FECS + 0x0A0);
        let mb = rd(0x40_9800);
        tracing::info!(
            us = delay_us,
            cpuctl = format_args!("{c:#010x}"),
            pc = format_args!("{p:#010x}"),
            idle = format_args!("{i:#010x}"),
            exci = format_args!("{e:#010x}"),
            sp = format_args!("{s:#010x}"),
            mb0 = format_args!("{mb:#010x}"),
            "POST-done boot: FECS trace"
        );
        if mb & 0x8000_0000 != 0 {
            tracing::info!("POST-done boot: FECS ready at {}μs!", delay_us);
            break;
        }
    }

    let fecs_post = rd(FECS + 0x100);
    let fecs_pc = rd(FECS + 0x0A8);
    let fecs_idle_post = rd(FECS + 0x04C);
    tracing::info!(
        fecs_post = format_args!("{fecs_post:#010x}"),
        fecs_pc = format_args!("{fecs_pc:#010x}"),
        fecs_idle = format_args!("{fecs_idle_post:#010x}"),
        consumed = fecs_post != fecs_ctl,
        "POST-done boot: FECS post-STARTCPU (internal)"
    );

    // ── Step 7: Poll for FECS ready (0x409800 bit 31 for internal firmware) ──
    let mut booted = false;
    for i in 0..2000 {
        std::thread::sleep(std::time::Duration::from_millis(1));

        if i % 50 == 0 {
            let mailbox0 = rd(0x40_9800);
            let cpuctl = rd(FECS + 0x100);
            let pc = rd(FECS + 0x0A8);
            let idle = rd(FECS + 0x04C);

            tracing::info!(
                poll_ms = i,
                mailbox0 = format_args!("{mailbox0:#010x}"),
                cpuctl = format_args!("{cpuctl:#010x}"),
                pc = format_args!("{pc:#010x}"),
                idle = format_args!("{idle:#010x}"),
                "POST-done boot: FECS poll (internal)"
            );

            let mb_ok = mailbox0 != 0xDEAD_DEAD && mailbox0 & 0xBAD0_0000 != 0xBAD0_0000;
            if mb_ok && mailbox0 & 0x8000_0000 != 0 {
                tracing::info!(
                    mailbox0 = format_args!("{mailbox0:#010x}"),
                    "POST-done boot: FECS ready (bit 31 set — internal FW booted)"
                );
                booted = true;
                break;
            }

            let cpu_ok = cpuctl != 0xDEAD_DEAD && cpuctl & 0xBAD0_0000 != 0xBAD0_0000;
            if cpu_ok && cpuctl == 0 && i > 100 {
                let exci = rd(FECS + 0x018);
                let sp = rd(FECS + 0x0A0);
                tracing::warn!(
                    cpuctl = format_args!("{cpuctl:#010x}"),
                    pc = format_args!("{pc:#010x}"),
                    idle = format_args!("{idle:#010x}"),
                    exci = format_args!("{exci:#010x}"),
                    sp = format_args!("{sp:#010x}"),
                    "POST-done boot: FECS exception (internal)"
                );
                break;
            }
            if cpu_ok && cpuctl & 0x10 != 0 && i > 200 {
                tracing::warn!(
                    cpuctl = format_args!("{cpuctl:#010x}"),
                    "POST-done boot: FECS still STOPPED"
                );
                break;
            }
        }
    }

    if booted {
        let ctx_size = rd(0x40_9804);
        tracing::info!(
            ctx_size = format_args!("{ctx_size:#010x}"),
            gpcs = cached_gpc_count,
            tpcs = cached_tpc_total,
            "POST-done boot: FECS/GPCCS boot complete — GR engine ready"
        );
    } else {
        let cpuctl = rd(FECS + 0x100);
        let gpccs_cpuctl = rd(GPCCS + 0x100);
        let mailbox0 = rd(0x40_9800);
        let scratch0 = rd(kepler_falcon::FECS_SCRATCH0);
        let scratch1 = rd(kepler_falcon::FECS_SCRATCH0 + 4);
        let gr_hub_intr = rd(0x40_0100);
        let exci = rd(FECS + 0x018);
        let sp = rd(FECS + 0x0A0);
        let pc = rd(FECS + 0x0A8);
        let itfen = rd(FECS + 0x048);
        let dbg0 = rd(FECS + 0x094);
        tracing::warn!(
            cpuctl = format_args!("{cpuctl:#010x}"),
            pc = format_args!("{pc:#010x}"),
            exci = format_args!("{exci:#010x}"),
            sp = format_args!("{sp:#010x}"),
            itfen = format_args!("{itfen:#010x}"),
            dbg0 = format_args!("{dbg0:#010x}"),
            gpccs = format_args!("{gpccs_cpuctl:#010x}"),
            mailbox0 = format_args!("{mailbox0:#010x}"),
            scratch0 = format_args!("{scratch0:#010x}"),
            scratch1 = format_args!("{scratch1:#010x}"),
            gr_intr = format_args!("{gr_hub_intr:#010x}"),
            "POST-done boot: FECS did not reach ready state (internal FW)"
        );
    }
}

pub(super) fn kepler_load_and_boot_fecs(
    guard: &super::hardware_guard::GuardedBar<'_>,
    cached_gpc_count: u32,
    cached_tpc_total: u32,
    cached_tpc_counts: &[(u32, u32); 8],
) {
    use crate::gsp::RegisterAccess;
    use crate::nv::kepler_falcon;

    let r = guard.read_fn();
    let w = |reg: u32, val: u32| {
        if let Err(refusal) = guard.write_u32(reg, val) {
            tracing::error!(%refusal, "kepler_load_and_boot_fecs: guard refused write");
        }
    };

    const FECS: u32 = kepler_falcon::FECS_BASE;
    const GPCCS: u32 = kepler_falcon::GPCCS_BASE;

    // K80 is GK210B (GK110B die). Nouveau defaults to INTERNAL firmware
    // (embedded in nouveau.ko, ~3KB FECS code) with a distinct boot protocol.
    // The external firmware files (/lib/firmware/nvidia/gk210/, ~15KB) use a
    // different protocol. We must match the firmware to its protocol.
    //
    // Priority: internal (matches nouveau's default) → external → gk110 fallback
    let fw_search: &[(&str, &str, &str, &str, &str, bool)] = &[
        (
            "gk210-internal",
            concat!(env!("CARGO_MANIFEST_DIR"), "/firmware/gk210"),
            "gk110_internal_fecs_code.bin",
            "gk110_internal_fecs_data.bin",
            "gk110_internal_gpccs_code.bin",
            true,
        ),
        (
            "gk210-external",
            concat!(env!("CARGO_MANIFEST_DIR"), "/firmware/gk210"),
            "gk210_fecs_code.bin",
            "gk210_fecs_data.bin",
            "gk210_gpccs_code.bin",
            false,
        ),
        (
            "gk210-system",
            "/lib/firmware/nvidia/gk210",
            "fecs_inst.bin",
            "fecs_data.bin",
            "gpccs_inst.bin",
            false,
        ),
        (
            "gk110-fallback",
            concat!(env!("CARGO_MANIFEST_DIR"), "/firmware/gk110"),
            "gk110_fecs_code.bin",
            "gk110_fecs_data.bin",
            "gk110_gpccs_code.bin",
            false,
        ),
    ];

    let mut fecs_code = None;
    let mut fecs_data = None;
    let mut gpccs_code = None;
    let mut gpccs_data = None;
    let mut use_internal_protocol = false;

    for &(label, dir, fc, fd, gc, is_internal) in fw_search {
        let gd = fc
            .replace("fecs", "gpccs")
            .replace("inst", "data")
            .replace("code", "data");
        let gd_name = if std::fs::metadata(format!("{dir}/{gd}")).is_ok() {
            gd.clone()
        } else {
            gc.replace("inst", "data").replace("code", "data")
        };

        let try_read = |name: &str| -> Option<Vec<u8>> {
            let path = format!("{dir}/{name}");
            std::fs::read(&path).ok().map(|data| {
                tracing::info!(path, bytes = data.len(), label, "loaded firmware");
                data
            })
        };

        if let (Some(fc_data), Some(fd_data), Some(gc_data), Some(gd_data)) =
            (try_read(fc), try_read(fd), try_read(gc), try_read(&gd_name))
        {
            tracing::info!(
                label,
                is_internal,
                fecs_code = fc_data.len(),
                fecs_data = fd_data.len(),
                gpccs_code = gc_data.len(),
                gpccs_data = gd_data.len(),
                "Selected firmware set"
            );
            fecs_code = Some(fc_data);
            fecs_data = Some(fd_data);
            gpccs_code = Some(gc_data);
            gpccs_data = Some(gd_data);
            use_internal_protocol = is_internal;
            break;
        }
        tracing::debug!(label, dir, "firmware set not complete, trying next");
    }

    let (Some(fecs_code), Some(fecs_data), Some(gpccs_code), Some(gpccs_data)) =
        (fecs_code, fecs_data, gpccs_code, gpccs_data)
    else {
        tracing::warn!("No GK210/GK110 FECS/GPCCS firmware available — GR will not work");
        return;
    };

    tracing::info!(
        fecs_code = fecs_code.len(),
        fecs_data = fecs_data.len(),
        gpccs_code = gpccs_code.len(),
        gpccs_data = gpccs_data.len(),
        "Loading GK110 FECS/GPCCS firmware via PIO"
    );

    let regs: &mut dyn RegisterAccess = &mut GuardedBarRegAccess(guard);

    // K80 sovereign boot following nouveau's gf100_gr_init() ordering:
    //   1. Disable traps (0x400500)
    //   2. GPC MMU init
    //   3. MMIO init table
    //   4. Wait idle
    //   5. Interrupts, exceptions, trap re-enable
    //   6. GR_UNITS (0x400054) — written LATE, like nouveau
    //   7. Then firmware load+boot via gf100_gr_init_ctxctl

    super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

    // Step 1: Disable traps — nouveau's FIRST GR HUB write in gf100_gr_init.
    // nvkm_mask(0x400500, 0x00010001, 0x00000000) → clear bits 0 and 16.
    w(0x40_0500, 0x0000_0000);
    let trap_readback = r(0x40_0500);
    tracing::info!(
        trap_readback = format_args!("{trap_readback:#010x}"),
        "Step 1: TRAP_EN disabled (0x400500)"
    );

    // Step 2: GPC MMU init (gf100_gr_init_gpc_mmu).
    // Writes to GPC broadcast regs — not behind GR HUB.
    {
        let fb_mmu = r(0x10_0C80) & 0x0000_0001;
        w(0x41_8880, fb_mmu);
        w(0x41_8890, 0x0000_0000);
        w(0x41_8894, 0x0000_0000);
    }

    // Step 3a: MMIO init table (gk110_gr_pack_mmio — hardcoded baseline).
    {
        let (gr_applied, gr_faulted) = super::kepler_gr_init::apply_gk110_gr_init(guard);
        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        tracing::info!(
            gr_applied,
            gr_faulted,
            "Step 3a: GR MMIO init (hardcoded gk110 pack)"
        );
    }

    // Step 3b: Apply sw_nonctx.bin — GK210B-specific register overrides.
    //
    // Nouveau applies gf100_gr_mmio(gr, gr->sw_nonctx) AFTER the hardcoded
    // pack. These firmware-provided values override the GK110 defaults with
    // GK210B-specific register configurations that FECS firmware expects
    // during boot. Without these, FECS traps immediately (EXCI=1 at PC=0).
    {
        let (nonctx_applied, nonctx_skipped) = super::pri::apply_sw_nonctx(guard, "gk210");
        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        tracing::info!(
            nonctx_applied,
            nonctx_skipped,
            "Step 3b: sw_nonctx.bin GK210B overrides"
        );
    }

    // Step 4: Trap re-enable + interrupt clearing (matches nouveau ordering).
    w(0x40_0500, 0x0001_0001); // re-enable traps
    w(0x40_0100, 0xFFFF_FFFF); // GR_INTR: write-1-to-clear all
    w(0x40_013c, 0xFFFF_FFFF); // GR_INTR_NONSTALL: clear
    w(0x40_0124, 0x0000_0002); // INTR_NOTIFY_EN

    // Step 5: Exception handling + HWW ESR (matches gf100_gr_init).
    w(0x40_4000, 0xc000_0000); // PD
    w(0x40_4600, 0xc000_0000); // PD
    w(0x40_8030, 0xc000_0000); // BE
    w(0x40_6018, 0xc000_0000); // DS
    w(0x40_4490, 0xc000_0000); // PRI
    w(0x40_5840, 0xc000_0000); // DS_DEBUG
    w(0x40_5844, 0x00ff_ffff); // DS_DEBUG

    // Interrupt clear + exception2 (nouveau: gf100_gr_init_exception2).
    w(0x40_0108, 0xFFFF_FFFF); // GR_TRAP_NONSTALL
    w(0x40_0138, 0xFFFF_FFFF); // GR_EXCEPTION
    w(0x40_0118, 0xFFFF_FFFF);
    w(0x40_0130, 0xFFFF_FFFF);
    w(0x40_011c, 0xFFFF_FFFF); // EXCEPTION2
    w(0x40_0134, 0xFFFF_FFFF); // EXCEPTION2

    // Step 6: GR_UNITS (0x400054) — written LATE like nouveau's
    // gf100_gr_init_400054.  Nouveau writes a fixed 0x34ce3464;
    // 0x400054 may be read-only fuse mirror — write it but don't
    // depend on the readback.  Use cached topology (read before
    // the PMC reset cleared fuse mirrors at 0x502608).
    {
        let mut gr_units: u32 = 0;
        for &(gpc, tpc_nr) in cached_tpc_counts.iter().take(cached_gpc_count as usize) {
            gr_units |= (tpc_nr & 0xF) << (gpc * 4);
        }
        w(0x40_0054, gr_units);
        let readback = r(0x40_0054);
        tracing::info!(
            gr_units = format_args!("{gr_units:#010x}"),
            readback = format_args!("{readback:#010x}"),
            cached_gpcs = cached_gpc_count,
            cached_tpcs = cached_tpc_total,
            "Step 6: GR_UNITS (0x400054 — may be RO fuse)"
        );
    }

    // Log state after GR init.
    {
        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        let fecs_cpuctl = r(FECS + 0x100);
        let gpccs_cpuctl = r(GPCCS + 0x100);
        let gr_hub = r(0x40_0700);
        let gr_units = r(0x40_0054);
        let is_ok = |v: u32| v != 0xDEAD_DEAD && v & 0xBAD0_0000 != 0xBAD0_0000;
        tracing::info!(
            fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
            gpccs_cpuctl = format_args!("{gpccs_cpuctl:#010x}"),
            gr_hub = format_args!(
                "{gr_hub:#010x}[{}]",
                if is_ok(gr_hub) { "OK" } else { "FAULT" }
            ),
            gr_units = format_args!("{gr_units:#010x}"),
            "Pre-upload state"
        );
    }

    // Bare-metal Falcon CPU test: attempt STARTCPU on uninitialized IMEM.
    // If the Falcon CPU is clocked, it should try to execute and trap.
    // If CPUCTL stays 0x10, the CPU clock is gated/not running.
    if use_internal_protocol {
        let bar0 = guard.inner();
        let rd_diag = |off: u32| -> u32 { bar0.read_u32(off as usize).unwrap_or(0xDEAD_DEAD) };

        let cpuctl_before = rd_diag(FECS + 0x100);
        let _ = bar0.write_u32((FECS + 0x10C) as usize, 0x0000_0000); // DMACTL = 0
        let _ = bar0.write_u32((FECS + 0x104) as usize, 0x0000_0000); // BOOTVEC = 0
        let _ = bar0.write_u32((FECS + 0x100) as usize, 0x0000_0002); // STARTCPU
        std::thread::sleep(std::time::Duration::from_millis(5));

        let cpuctl_after = rd_diag(FECS + 0x100);
        let pc_after = rd_diag(FECS + 0x0A8);
        let exci_after = rd_diag(FECS + 0x04C);

        // Read extra diagnostic registers.
        let hwcfg1 = rd_diag(FECS + 0x12C);
        let hwcfg2 = rd_diag(FECS + 0x00C);
        let debug0 = rd_diag(0x40_9C14);
        let debug1 = rd_diag(0x40_9C18);
        let debug2 = rd_diag(0x40_9C1C);
        let cgctrl = rd_diag(FECS + 0x360);

        tracing::info!(
            cpuctl_before = format_args!("{cpuctl_before:#010x}"),
            cpuctl_after = format_args!("{cpuctl_after:#010x}"),
            pc = format_args!("{pc_after:#010x}"),
            exci = format_args!("{exci_after:#010x}"),
            hwcfg1 = format_args!("{hwcfg1:#010x}"),
            hwcfg2 = format_args!("{hwcfg2:#010x}"),
            cgctrl = format_args!("{cgctrl:#010x}"),
            dbg0 = format_args!("{debug0:#010x}"),
            dbg1 = format_args!("{debug1:#010x}"),
            dbg2 = format_args!("{debug2:#010x}"),
            cpu_responded = cpuctl_after != cpuctl_before,
            "BARE-METAL Falcon CPU test (no firmware, uninitialized IMEM)"
        );

        // Reset FECS back to HALT for proper firmware upload.
        let _ = bar0.write_u32((FECS + 0x3C0) as usize, 0x0000_0001);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let _ = bar0.write_u32((FECS + 0x3C0) as usize, 0x0000_0000);
        std::thread::sleep(std::time::Duration::from_millis(2));

        // ENGCTL cycle triggers IMEM/DMEM scrub — wait for it to complete
        // before uploading firmware, otherwise PIO writes race with the
        // scrub engine and produce corrupted IMEM.
        for _ in 0..400 {
            if rd_diag(FECS + 0x10C) & 0x06 == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    // 1. Disable GR method dispatch for firmware load (nouveau nvkm_mc_unk260(0))
    regs.write_u32(0x260, 0).ok();

    // 1b. ENGCTL HRESET cycle — SKIP for internal firmware protocol.
    //
    // After PMC GR reset, FECS is already in the correct initial HALT state
    // (CPUCTL=0x10). ENGCTL HRESET was intended for external firmware but
    // appears to put FECS into a state where STARTCPU is silently ignored.
    //
    // For internal protocol: rely on PMC GR reset's initial state.
    // For external protocol: cycle ENGCTL to force GPCCS instances clean.
    if !use_internal_protocol {
        // FECS ENGCTL via hub register:
        {
            let _ = guard.write_u32(FECS + 0x3C0, 0x01);
            std::thread::sleep(std::time::Duration::from_millis(2));
            let _ = guard.write_u32(FECS + 0x3C0, 0x00);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        // GPCCS ENGCTL via per-GPC direct PRI (broadcast faults on GK210B):
        for gpc in 0..8u32 {
            let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
            let probe = r(gpccs_base + 0x100);
            if probe == 0xDEAD_DEAD || probe & 0xBAD0_0000 == 0xBAD0_0000 {
                continue;
            }
            let _ = guard.write_u32(gpccs_base + 0x3C0, 0x01);
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
        for gpc in 0..8u32 {
            let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
            let probe = r(gpccs_base + 0x100);
            if probe == 0xDEAD_DEAD || probe & 0xBAD0_0000 == 0xBAD0_0000 {
                continue;
            }
            let _ = guard.write_u32(gpccs_base + 0x3C0, 0x00);
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    {
        let fecs_cpuctl_post = r(FECS + 0x100);
        let gpc0_cpuctl_post = r(0x50_2000 + 0x100);
        tracing::info!(
            fecs = format_args!("{fecs_cpuctl_post:#010x}"),
            gpc0_gpccs = format_args!("{gpc0_cpuctl_post:#010x}"),
            engctl_skipped = use_internal_protocol,
            "Post ENGCTL phase (FECS should be 0x10 = HALTED)"
        );
    }

    // Re-scan TPC topology in case GPCs became accessible after PRI re-enum.
    let (live_gpc_count, live_tpc_total, live_tpc_counts) = super::pri::scan_gpc_topology(guard);
    let (use_gpc_count, use_tpc_total, use_tpc_counts) = if live_tpc_total > 0 {
        (live_gpc_count, live_tpc_total, live_tpc_counts)
    } else {
        (cached_gpc_count, cached_tpc_total, *cached_tpc_counts)
    };
    tracing::info!(
        live_gpcs = live_gpc_count,
        live_tpcs = live_tpc_total,
        use_gpcs = use_gpc_count,
        use_tpcs = use_tpc_total,
        "Topology scan (live vs cached fallback)"
    );

    // 2. Diagnose GPCCS Falcon state before upload.
    //    Check DMACTL scrub bits, CPUCTL, and PIO accessibility.
    {
        let fecs_dmactl = r(FECS + 0x10C);
        let gpccs_bcast_dmactl = r(GPCCS + 0x10C);
        let gpc0_gpccs_dmactl = r(0x50_2000 + 0x10C);
        let gpc0_gpccs_cpuctl = r(0x50_2000 + 0x100);
        let gpccs_bcast_cpuctl = r(GPCCS + 0x100);
        let gr_gpc_bcast = r(0x41_8000);
        tracing::info!(
            fecs_dmactl = format_args!("{fecs_dmactl:#010x}"),
            gpccs_bcast_dmactl = format_args!("{gpccs_bcast_dmactl:#010x}"),
            gpc0_dmactl = format_args!("{gpc0_gpccs_dmactl:#010x}"),
            gpc0_cpuctl = format_args!("{gpc0_gpccs_cpuctl:#010x}"),
            gpccs_bcast_cpuctl = format_args!("{gpccs_bcast_cpuctl:#010x}"),
            gr_gpc_bcast = format_args!("{gr_gpc_bcast:#010x}"),
            "Pre-upload Falcon state (DMACTL scrub bits [2:1])"
        );

        // Wait for GPCCS memory scrub if active (bits [2:1] of DMACTL).
        if gpc0_gpccs_dmactl & 0x06 != 0 {
            let mut scrub_ok = false;
            for _ in 0..200 {
                std::thread::sleep(std::time::Duration::from_millis(10));
                if r(0x50_2000 + 0x10C) & 0x06 == 0 {
                    scrub_ok = true;
                    break;
                }
            }
            tracing::info!(scrub_ok, "GPCCS GPC0 memory scrub wait");
        }
    }

    // 3. Upload FECS (hub) DMEM + IMEM
    if let Err(e) = kepler_falcon::upload_dmem(regs, FECS, 0, &fecs_data) {
        tracing::warn!(error = %e, "FECS DMEM upload failed");
        return;
    }
    if let Err(e) = kepler_falcon::upload_imem(regs, FECS, 0, &fecs_code) {
        tracing::warn!(error = %e, "FECS IMEM upload failed");
        return;
    }

    // 4. Upload GPCCS DMEM + IMEM via broadcast (0x41A000).
    //    On Kepler, per-GPC GPCCS PIO interfaces (0x502180 etc.) may not
    //    support IMEM/DMEM upload — only the GR hub broadcast does.
    if let Err(e) = kepler_falcon::upload_dmem(regs, GPCCS, 0, &gpccs_data) {
        tracing::warn!(error = %e, "GPCCS DMEM upload failed");
        return;
    }
    if let Err(e) = kepler_falcon::upload_imem(regs, GPCCS, 0, &gpccs_code) {
        tracing::warn!(error = %e, "GPCCS IMEM upload failed");
        return;
    }

    // Verify GPCCS upload via broadcast readback AND per-GPC direct readback.
    {
        const IMEM_READ_AUTOINC: u32 = 1 << 25;
        const GPC0_GPCCS: u32 = 0x50_2000;
        let bar0 = guard.inner();

        // Broadcast readback
        let _ = bar0.write_u32((GPCCS as usize) + 0x180, IMEM_READ_AUTOINC);
        let bcast_word = bar0
            .read_u32((GPCCS as usize) + 0x184)
            .unwrap_or(0xDEAD_DEAD);

        // Per-GPC0 direct readback
        let _ = bar0.write_u32((GPC0_GPCCS + 0x180) as usize, IMEM_READ_AUTOINC as u32);
        let gpc0_word = bar0
            .read_u32((GPC0_GPCCS + 0x184) as usize)
            .unwrap_or(0xDEAD_DEAD);

        // Single-word PIO round-trip test on GPC0 GPCCS DMEM[0].
        // Save original, write test pattern, verify, then restore.
        let _ = bar0.write_u32((GPC0_GPCCS + 0x1C0) as usize, 1 << 25); // DMEMC: addr=0, AINCR
        let dmem0_orig = bar0
            .read_u32((GPC0_GPCCS + 0x1C4) as usize)
            .unwrap_or(0xDEAD_DEAD);
        let _ = bar0.write_u32((GPC0_GPCCS + 0x1C0) as usize, (1 << 24) | (1 << 30));
        let _ = bar0.write_u32((GPC0_GPCCS + 0x1C4) as usize, 0xCAFE_BEEF_u32);
        let _ = bar0.write_u32((GPC0_GPCCS + 0x1C0) as usize, 1 << 25);
        let pio_test = bar0
            .read_u32((GPC0_GPCCS + 0x1C4) as usize)
            .unwrap_or(0xDEAD_DEAD);
        // Restore original DMEM[0] so firmware data isn't corrupted.
        let _ = bar0.write_u32((GPC0_GPCCS + 0x1C0) as usize, (1 << 24) | (1 << 30));
        let _ = bar0.write_u32((GPC0_GPCCS + 0x1C4) as usize, dmem0_orig);

        let expected_first =
            u32::from_le_bytes([gpccs_code[0], gpccs_code[1], gpccs_code[2], gpccs_code[3]]);
        tracing::info!(
            bcast_imem0 = format_args!("{bcast_word:#010x}"),
            gpc0_imem0 = format_args!("{gpc0_word:#010x}"),
            expected = format_args!("{expected_first:#010x}"),
            pio_test = format_args!("{pio_test:#010x}"),
            pio_expected = format_args!("{:#010x}", 0xCAFE_BEEF_u32),
            pio_works = pio_test == 0xCAFE_BEEF_u32,
            "GPCCS upload verification"
        );
    }
    tracing::info!("FECS + GPCCS firmware uploaded");

    // Verify IMEM content survived upload (readback first 4 words)
    {
        const IMEM_READ_AUTOINC: u32 = 1 << 25;
        regs.write_u32(FECS + 0x180, IMEM_READ_AUTOINC).ok();
        let mut rb = [0u32; 4];
        for w in &mut rb {
            *w = regs.read_u32(FECS + 0x184).unwrap_or(0xDEAD_DEAD);
        }
        let expected = [
            u32::from_le_bytes([fecs_code[0], fecs_code[1], fecs_code[2], fecs_code[3]]),
            u32::from_le_bytes([fecs_code[4], fecs_code[5], fecs_code[6], fecs_code[7]]),
            u32::from_le_bytes([fecs_code[8], fecs_code[9], fecs_code[10], fecs_code[11]]),
            u32::from_le_bytes([fecs_code[12], fecs_code[13], fecs_code[14], fecs_code[15]]),
        ];
        let ok = rb == expected;
        tracing::info!(
            readback = format_args!("{:08x} {:08x} {:08x} {:08x}", rb[0], rb[1], rb[2], rb[3]),
            expected = format_args!(
                "{:08x} {:08x} {:08x} {:08x}",
                expected[0], expected[1], expected[2], expected[3]
            ),
            imem_ok = ok,
            "FECS IMEM readback after upload"
        );
    }

    // 4. Re-enable GR method dispatch (nouveau nvkm_mc_unk260(1))
    regs.write_u32(0x260, 0x0000_0001).ok();

    // Clear PRI ring faults accumulated during firmware upload.
    super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

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

        // Load csdata — register save/restore lists for context switching.
        // The internal firmware reads packed (addr, count) pairs from DMEM
        // to know which GR registers to manage. For GK110B, these come from
        // gk110b_grctx tables in nouveau.
        //
        // For initial boot testing, we write a minimal csdata terminator so
        // FECS doesn't read garbage from uninitialised DMEM.
        {
            let load_csdata_stub = |falcon: u32, starstar: u32| {
                let bar0 = guard.inner();
                let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };

                // Read the star pointer from DMEM[starstar].
                let _ = bar0.write_u32((falcon + 0x1C0) as usize, 0x0200_0000 + starstar);
                let star = rd(falcon + 0x1C4);
                let _temp = rd(falcon + 0x1C4);

                if star == 0xDEAD_DEAD || star & 0xBAD0_0000 == 0xBAD0_0000 || star == 0 {
                    tracing::warn!(
                        falcon = format_args!("{falcon:#010x}"),
                        starstar,
                        star = format_args!("{star:#010x}"),
                        "csdata star pointer invalid — skipping csdata"
                    );
                    return;
                }

                // Position at star offset and write a single terminator (0x00000000).
                let _ = bar0.write_u32((falcon + 0x1C0) as usize, 0x0100_0000 + star);
                let _ = bar0.write_u32((falcon + 0x1C4) as usize, 0x0000_0000);

                tracing::info!(
                    falcon = format_args!("{falcon:#010x}"),
                    starstar,
                    star = format_args!("{star:#010x}"),
                    "csdata stub terminator written"
                );
            };

            // FECS hub csdata at starstar=0x000
            load_csdata_stub(FECS, 0x000);
            // GPCCS csdata at starstar offsets: gpc_0=0x000, gpc_1=0x000, tpc=0x004, ppc=0x008
            load_csdata_stub(GPCCS, 0x000);
            load_csdata_stub(GPCCS, 0x004);
            load_csdata_stub(GPCCS, 0x008);
        }

        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
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
                tracing::warn!("PGRAPH gated before STARTCPU — re-disabling PGOB");
                super::pgob::gk110_pgob_disable(guard);
                std::thread::sleep(std::time::Duration::from_millis(10));
                super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
            }
        }

        // FECS boot: BOOTVEC=0, DMACTL=0, then STARTCPU via direct bar0.
        //
        // On Falcon v3, STARTCPU only works from "initial halt" (post-PMC-reset)
        // or "software halt" (post-HALT instruction). If an ENGCTL HRESET cycle
        // was done, the falcon may be in "hardware reset halt" where STARTCPU
        // is silently ignored. We detect this (CPUCTL stays 0x10 after START)
        // and fall back to ENGCTL deassert → IINVAL → STARTCPU.
        {
            let bar0 = guard.inner();
            let rd = |off: u32| -> u32 { bar0.read_u32(off as usize).unwrap_or(0xDEAD_DEAD) };

            let _ = bar0.write_u32((FECS + 0x10C) as usize, 0x0000_0000); // DMACTL = 0
            let _ = bar0.write_u32((FECS + 0x104) as usize, 0x0000_0000); // BOOTVEC = 0

            // Diagnostic: Falcon version and write verification.
            let falcon_ver = rd(FECS + 0x004);
            let sctl = rd(FECS + 0x240);
            let dmactl = rd(FECS + 0x10C);

            // Write/readback test on FECS MAILBOX0 (R/W register).
            let mb0_orig = rd(FECS + 0x040);
            let _ = bar0.write_u32((FECS + 0x040) as usize, 0xCAFE_1234);
            let mb0_test = rd(FECS + 0x040);
            let _ = bar0.write_u32((FECS + 0x040) as usize, mb0_orig); // restore
            let writes_work = mb0_test == 0xCAFE_1234;

            let cpuctl_pre = rd(FECS + 0x100);
            let engctl_pre = rd(FECS + 0x3C0);
            let bootvec_rb = rd(FECS + 0x104);
            let exci_pre = rd(FECS + 0x04C);
            tracing::info!(
                cpuctl = format_args!("{cpuctl_pre:#010x}"),
                engctl = format_args!("{engctl_pre:#010x}"),
                bootvec = format_args!("{bootvec_rb:#010x}"),
                exci = format_args!("{exci_pre:#010x}"),
                falcon_ver = format_args!("{falcon_ver:#010x}"),
                sctl = format_args!("{sctl:#010x}"),
                dmactl = format_args!("{dmactl:#010x}"),
                mb0_write_test = format_args!("{mb0_test:#010x}"),
                writes_work,
                "Pre-STARTCPU: FECS state + write test"
            );

            // Attempt 1: Direct STARTCPU (works if falcon is in initial/SW halt).
            let _ = bar0.write_u32((FECS + 0x100) as usize, 0x0000_0002);
            std::thread::sleep(std::time::Duration::from_micros(500));

            let cpuctl_post1 = rd(FECS + 0x100);
            let pc_post1 = rd(FECS + 0x0A8);
            let exci_post1 = rd(FECS + 0x04C);
            tracing::info!(
                cpuctl = format_args!("{cpuctl_post1:#010x}"),
                pc = format_args!("{pc_post1:#010x}"),
                exci = format_args!("{exci_post1:#010x}"),
                "Attempt 1: STARTCPU direct"
            );

            if cpuctl_post1 == 0x10 && exci_post1 == 0 {
                // STARTCPU was ignored — falcon still in HW reset halt.
                // Attempt 2: ENGCTL deassert, then IINVAL + STARTCPU.
                tracing::warn!(
                    "STARTCPU ignored (HW reset halt) — trying ENGCTL deassert + IINVAL"
                );

                // Ensure ENGCTL is deasserted.
                let _ = bar0.write_u32((FECS + 0x3C0) as usize, 0x0000_0000);
                std::thread::sleep(std::time::Duration::from_millis(5));

                let engctl_after = rd(FECS + 0x3C0);
                let cpuctl_after_deassert = rd(FECS + 0x100);
                tracing::info!(
                    engctl = format_args!("{engctl_after:#010x}"),
                    cpuctl = format_args!("{cpuctl_after_deassert:#010x}"),
                    "After ENGCTL deassert"
                );

                // IINVAL (bit 0) — invalidate instruction TLB.
                let _ = bar0.write_u32((FECS + 0x100) as usize, 0x0000_0001);
                std::thread::sleep(std::time::Duration::from_millis(1));

                // STARTCPU (bit 1)
                let _ = bar0.write_u32((FECS + 0x100) as usize, 0x0000_0002);
                std::thread::sleep(std::time::Duration::from_millis(1));

                let cpuctl_post2 = rd(FECS + 0x100);
                let pc_post2 = rd(FECS + 0x0A8);
                let exci_post2 = rd(FECS + 0x04C);
                tracing::info!(
                    cpuctl = format_args!("{cpuctl_post2:#010x}"),
                    pc = format_args!("{pc_post2:#010x}"),
                    exci = format_args!("{exci_post2:#010x}"),
                    "Attempt 2: ENGCTL deassert + IINVAL + STARTCPU"
                );

                if cpuctl_post2 == 0x10 && exci_post2 == 0 {
                    // Attempt 3: Full ENGCTL cycle (assert then deassert).
                    tracing::warn!("Still stuck — trying full ENGCTL cycle + STARTCPU");

                    let _ = bar0.write_u32((FECS + 0x3C0) as usize, 0x0000_0001);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    let _ = bar0.write_u32((FECS + 0x3C0) as usize, 0x0000_0000);
                    std::thread::sleep(std::time::Duration::from_millis(5));

                    // Re-upload BOOTVEC (ENGCTL cycle clears it).
                    let _ = bar0.write_u32((FECS + 0x104) as usize, 0x0000_0000);
                    let _ = bar0.write_u32((FECS + 0x10C) as usize, 0x0000_0000);
                    let _ = bar0.write_u32((FECS + 0x100) as usize, 0x0000_0002);
                    std::thread::sleep(std::time::Duration::from_millis(1));

                    let cpuctl_post3 = rd(FECS + 0x100);
                    let pc_post3 = rd(FECS + 0x0A8);
                    let exci_post3 = rd(FECS + 0x04C);
                    tracing::info!(
                        cpuctl = format_args!("{cpuctl_post3:#010x}"),
                        pc = format_args!("{pc_post3:#010x}"),
                        exci = format_args!("{exci_post3:#010x}"),
                        "Attempt 3: Full ENGCTL cycle + STARTCPU"
                    );

                    if cpuctl_post3 == 0x10 && exci_post3 == 0 {
                        // Attempt 4: CPUCTL alias register (0x130).
                        tracing::warn!("Still stuck — trying CPUCTL alias (0x130)");
                        let _ = bar0.write_u32((FECS + 0x130) as usize, 0x0000_0002);
                        std::thread::sleep(std::time::Duration::from_millis(1));

                        let cpuctl_post4 = rd(FECS + 0x100);
                        let pc_post4 = rd(FECS + 0x0A8);
                        let exci_post4 = rd(FECS + 0x04C);
                        tracing::info!(
                            cpuctl = format_args!("{cpuctl_post4:#010x}"),
                            pc = format_args!("{pc_post4:#010x}"),
                            exci = format_args!("{exci_post4:#010x}"),
                            "Attempt 4: CPUCTL alias (0x130)"
                        );
                    }
                }
            }
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
                        pc = format_args!("{fecs_pc:#010x}"),
                        exci = format_args!("{fecs_exci:#010x}"),
                        "FECS HALTED — firmware hit exception"
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

            tracing::warn!(
                cpuctl = format_args!("{cpuctl:#010x}"),
                mailbox0 = format_args!("{mailbox0:#010x}"),
                gr_hub = format_args!("{gr_hub:#010x}"),
                scratch0 = format_args!("{scratch0:#010x}"),
                scratch1 = format_args!("{scratch1:#010x}"),
                gpc0_gpccs = format_args!("{gpc0_cpuctl:#010x}"),
                pc = format_args!("{fecs_pc:#010x}"),
                exci = format_args!("{fecs_exci:#010x}"),
                "Kepler FECS did not reach ready state (internal — 0x409800 bit 31)"
            );
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

        tracing::info!("Using EXTERNAL firmware boot protocol (GPCCS+FECS start, poll bit 0)");

        // Topology registers for external firmware.
        {
            let pri_gpc_count = r(0x12_0074);
            let mut gpc_disable_mask: u32 = 0;
            for gpc in 0..pri_gpc_count.min(8) {
                let is_active = use_tpc_counts
                    .iter()
                    .take(use_gpc_count as usize)
                    .any(|&(g, _)| g == gpc);
                if !is_active {
                    gpc_disable_mask |= 1 << gpc;
                }
            }

            w(0x40_9604, use_gpc_count);
            w(0x40_9608, use_tpc_total);
            w(0x40_960C, gpc_disable_mask);

            for (idx, &(_, tpc_nr)) in use_tpc_counts
                .iter()
                .take(use_gpc_count as usize)
                .enumerate()
            {
                w(0x40_9640 + (idx as u32) * 4, tpc_nr);
                w(0x40_9680 + (idx as u32) * 4, tpc_nr);
            }

            let fbp_count = r(0x12_0078);
            let fbp_count_val =
                if fbp_count != 0xDEAD_DEAD && fbp_count & 0xBAD0_0000 != 0xBAD0_0000 {
                    fbp_count
                } else {
                    use_gpc_count
                };
            w(0x40_9A04, fbp_count_val);

            tracing::info!(
                gpc_count = use_gpc_count,
                tpc_total = use_tpc_total,
                gpc_disable_mask = format_args!("{gpc_disable_mask:#010x}"),
                fbp_count = fbp_count_val,
                tpc_per_gpc = ?use_tpc_counts.iter()
                    .take(use_gpc_count as usize)
                    .map(|&(g, t)| format!("GPC{g}:{t}T"))
                    .collect::<Vec<_>>(),
                "FECS topology registers (external)"
            );
        }

        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        w(0x40_0100, 0xFFFF_FFFF);

        w(CTXSW_MAILBOX0, 0x0000_0000);

        w(GPCCS + 0x10C, 0x0000_0000);
        for gpc in 0..8u32 {
            let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
            let probe = r(gpccs_base + 0x100);
            if probe == 0xDEAD_DEAD || probe & 0xBAD0_0000 == 0xBAD0_0000 {
                continue;
            }
            let _ = guard.write_u32(gpccs_base + 0x10C, 0x0000_0000);
        }
        w(FECS + 0x10C, 0x0000_0000);

        // Start GPCCS first.
        {
            let gpccs_cpuctl = r(GPCCS + 0x100);
            let gpccs_start_reg = if gpccs_cpuctl & (1 << 6) != 0 {
                0x130
            } else {
                0x100
            };
            w(GPCCS + gpccs_start_reg, 0x0000_0002);

            for gpc in 0..8u32 {
                let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
                let probe = r(gpccs_base + 0x100);
                if probe == 0xDEAD_DEAD || probe & 0xBAD0_0000 == 0xBAD0_0000 {
                    continue;
                }
                let start_reg = if probe & (1 << 6) != 0 { 0x130 } else { 0x100 };
                let _ = guard.write_u32(gpccs_base + start_reg, 0x0000_0002);
            }
        }

        // Start FECS.
        {
            let fecs_cpuctl = r(FECS + 0x100);
            let fecs_start_reg = if fecs_cpuctl & (1 << 6) != 0 {
                0x130
            } else {
                0x100
            };
            w(FECS + fecs_start_reg, 0x0000_0002);
            tracing::info!(
                fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
                start_reg = format_args!("{:#05x}", FECS + fecs_start_reg),
                "FECS STARTCPU issued (external protocol)"
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
                        "FECS HALTED — firmware hit exception"
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

/// `RegisterAccess` adapter routing through `GuardedBar` — writes go through
/// the blocklist/canary checks, reads through the link-alive check.
struct GuardedBarRegAccess<'a>(&'a super::hardware_guard::GuardedBar<'a>);

impl crate::gsp::RegisterAccess for GuardedBarRegAccess<'_> {
    fn read_u32(&self, offset: u32) -> Result<u32, crate::gsp::ApplyError> {
        self.0
            .read_u32(offset)
            .map_err(|refusal| crate::gsp::ApplyError::MmioFailed {
                offset,
                detail: refusal.to_string(),
            })
    }

    fn write_u32(&mut self, offset: u32, value: u32) -> Result<(), crate::gsp::ApplyError> {
        self.0
            .write_u32(offset, value)
            .map_err(|refusal| crate::gsp::ApplyError::MmioFailed {
                offset,
                detail: refusal.to_string(),
            })
    }
}
