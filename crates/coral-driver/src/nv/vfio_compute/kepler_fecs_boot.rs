// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kepler POST-done and cold-path FECS/GPCCS boot — GK110/GK210 firmware upload and start.
//!
//! Split from [`super::init`] for readability. Implements Nouveau-aligned GR init ordering,
//! internal vs external firmware protocols, and topology-aware register setup.


/// POST-done FECS boot using external firmware protocol.
///
/// After Nouveau POST + unbind, PGRAPH is powered and GPCs are alive.
/// Internal firmware (4KB) requires host-side GR MMIO init that we cannot
/// safely replicate without risking GPC power state. External firmware
/// (15KB) is self-configuring — it reads hardware registers directly.
///
/// Strategy (matches `gf100_gr_init_ctxctl_ext`):
/// 1. Clear PRI ring faults
/// 2. Upload GK210 external firmware (GPCCS then FECS)
/// 3. Write topology to CTXSW registers (0x409600+)
/// 4. Start GPCCS, then FECS
/// 5. Poll 0x409800 bit 0
pub(super) fn kepler_post_done_boot_fecs(
    guard: &super::hardware_guard::GuardedBar<'_>,
    cached_gpc_count: u32,
    cached_tpc_total: u32,
    cached_tpc_counts: &[(u32, u32); 8],
) {
    use crate::nv::kepler_falcon;

    const FECS: u32 = kepler_falcon::FECS_BASE;
    const GPCCS: u32 = kepler_falcon::GPCCS_BASE;

    let bar0 = guard.inner();
    let rd = |off: u32| -> u32 { bar0.read_u32(off as usize).unwrap_or(0xDEAD_DEAD) };
    let wr = |off: u32, val: u32| { let _ = bar0.write_u32(off as usize, val); };

    let fecs_cpuctl = rd(FECS + 0x100);
    let gpc0_tpc = rd(0x50_2608);
    let gpcs_alive = gpc0_tpc != 0xDEAD_DEAD && gpc0_tpc & 0xBAD0_0000 != 0xBAD0_0000;
    tracing::info!(
        fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
        gpc0_tpc = format_args!("{gpc0_tpc:#010x}"),
        gpcs_alive,
        pmc = format_args!("{:#010x}", rd(0x200)),
        "POST-done: external firmware warm boot — initial state"
    );

    // ── Step 0: Clear PRI ring faults ──
    {
        wr(0x12_004c, 0x2);
        for wait in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            if rd(0x12_004c) & 0x3f == 0 {
                tracing::info!(wait_ms = wait, "POST-done: PRI ring faults cleared");
                break;
            }
        }
    }

    // ── Step 1: Load external firmware files ──
    let fw_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/firmware/gk210");
    let try_read = |name: &str| -> Option<Vec<u8>> {
        std::fs::read(format!("{fw_dir}/{name}")).ok()
    };
    let (Some(fecs_code), Some(fecs_data), Some(gpccs_code), Some(gpccs_data)) = (
        try_read("gk210_fecs_code.bin"),
        try_read("gk210_fecs_data.bin"),
        try_read("gk210_gpccs_code.bin"),
        try_read("gk210_gpccs_data.bin"),
    ) else {
        tracing::warn!("POST-done boot: missing GK210 external firmware");
        return;
    };
    tracing::info!(
        fecs_code = fecs_code.len(),
        fecs_data = fecs_data.len(),
        gpccs_code = gpccs_code.len(),
        gpccs_data = gpccs_data.len(),
        "POST-done boot: firmware loaded (gk210-external)"
    );

    // ── Step 2: Upload firmware (Nouveau-aligned: no ENGCTL, no ITFEN) ──
    // PMC GR reset (done by caller) already puts falcons in clean initial-halt
    // state with invalidated caches. ENGCTL cycle is unnecessary and risks
    // putting the falcon in HW-reset-halt where STARTCPU is silently ignored.
    // ITFEN is not set by Nouveau before upload — PIO port works without it.
    {
        let regs: &mut dyn crate::gsp::RegisterAccess = &mut GuardedBarRegAccess(guard);
        regs.write_u32(0x260, 0).ok();

        if let Err(e) = kepler_falcon::upload_dmem(regs, FECS, 0, &fecs_data) {
            tracing::warn!(error = %e, "FECS DMEM upload failed"); return;
        }
        if let Err(e) = kepler_falcon::upload_imem(regs, FECS, 0, &fecs_code) {
            tracing::warn!(error = %e, "FECS IMEM upload failed"); return;
        }
        if let Err(e) = kepler_falcon::upload_dmem(regs, GPCCS, 0, &gpccs_data) {
            tracing::warn!(error = %e, "GPCCS DMEM upload failed"); return;
        }
        if let Err(e) = kepler_falcon::upload_imem(regs, GPCCS, 0, &gpccs_code) {
            tracing::warn!(error = %e, "GPCCS IMEM upload failed"); return;
        }

        regs.write_u32(0x260, 1).ok();
    }
    tracing::info!("POST-done boot: FECS + GPCCS firmware uploaded");

    // Nouveau gf100_gr_init_fw() sets ITFEN=3 + WDT=0 at end of each upload.
    // Broadcast GPCCS writes are dropped on GK210B, so fan out per-GPC.
    {
        wr(FECS + 0x048, 0x0000_0003);
        wr(FECS + 0x054, 0x0000_0000);
        wr(GPCCS + 0x048, 0x0000_0003);
        wr(GPCCS + 0x054, 0x0000_0000);
        for gpc in 0..cached_gpc_count.min(8) {
            let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
            let probe = rd(gpccs_base + 0x100);
            if probe != 0xDEAD_DEAD && probe & 0xBAD0_0000 != 0xBAD0_0000 {
                wr(gpccs_base + 0x048, 0x0000_0003);
                wr(gpccs_base + 0x054, 0x0000_0000);
            }
        }
    }

    // Verify IMEM content survived upload (readback first 4 words).
    {
        let regs: &mut dyn crate::gsp::RegisterAccess = &mut GuardedBarRegAccess(guard);
        regs.write_u32(FECS + 0x180, 1 << 25).ok(); // IMEMC: offset 0, auto-increment read
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
        tracing::info!(
            readback = format_args!("{:08x} {:08x} {:08x} {:08x}", rb[0], rb[1], rb[2], rb[3]),
            expected = format_args!("{:08x} {:08x} {:08x} {:08x}", expected[0], expected[1], expected[2], expected[3]),
            imem_ok = rb == expected,
            "POST-done boot: FECS IMEM readback"
        );
    }

    // ── Step 4: Write topology registers (external firmware protocol) ──
    {
        let mut gpc_disable_mask: u32 = 0;
        let pri_gpc_count = rd(0x12_0074);
        let max_gpcs = if pri_gpc_count != 0xDEAD_DEAD && pri_gpc_count < 16 {
            pri_gpc_count
        } else {
            8
        };
        for gpc in 0..max_gpcs {
            let is_active = cached_tpc_counts.iter()
                .take(cached_gpc_count as usize)
                .any(|&(g, _)| g == gpc);
            if !is_active {
                gpc_disable_mask |= 1 << gpc;
            }
        }

        wr(0x40_9604, cached_gpc_count);
        wr(0x40_9608, cached_tpc_total);
        wr(0x40_960C, gpc_disable_mask);

        for (idx, &(_, tpc_nr)) in cached_tpc_counts.iter()
            .take(cached_gpc_count as usize)
            .enumerate()
        {
            wr(0x40_9640 + (idx as u32) * 4, tpc_nr);
            wr(0x40_9680 + (idx as u32) * 4, tpc_nr);
        }

        let fbp_count = rd(0x12_0078);
        let fbp_count_val = if fbp_count != 0xDEAD_DEAD && fbp_count & 0xBAD0_0000 != 0xBAD0_0000 {
            fbp_count
        } else {
            cached_gpc_count
        };
        wr(0x40_9A04, fbp_count_val);

        tracing::info!(
            gpc_count = cached_gpc_count,
            tpc_total = cached_tpc_total,
            gpc_disable = format_args!("{gpc_disable_mask:#010x}"),
            fbp_count = fbp_count_val,
            "POST-done boot: topology registers written"
        );
    }

    // ── Step 4: Clear state and start falcons (external protocol) ──
    super::pri::clear_pri_ring_faults(guard.inner(), &guard.read_fn(), &|reg, val| {
        let _ = guard.write_u32(reg, val);
    });
    wr(0x40_0100, 0xFFFF_FFFF); // GR_INTR: clear all
    wr(0x40_9800, 0x0000_0000); // CTXSW_MAILBOX0 = 0

    wr(GPCCS + 0x10C, 0x0000_0000); // GPCCS DMACTL = 0
    for gpc in 0..cached_gpc_count.min(8) {
        let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
        let probe = rd(gpccs_base + 0x100);
        if probe != 0xDEAD_DEAD && probe & 0xBAD0_0000 != 0xBAD0_0000 {
            let _ = guard.write_u32(gpccs_base + 0x10C, 0x0000_0000);
        }
    }
    wr(FECS + 0x10C, 0x0000_0000); // FECS DMACTL = 0

    // Start GPCCS first (external protocol requirement).
    {
        let gpccs_cpuctl = rd(GPCCS + 0x100);
        let start_reg = if gpccs_cpuctl & (1 << 6) != 0 { 0x130 } else { 0x100 };
        wr(GPCCS + start_reg, 0x0000_0002);

        for gpc in 0..cached_gpc_count.min(8) {
            let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
            let probe = rd(gpccs_base + 0x100);
            if probe == 0xDEAD_DEAD || probe & 0xBAD0_0000 == 0xBAD0_0000 { continue; }
            let sr = if probe & (1 << 6) != 0 { 0x130 } else { 0x100 };
            let _ = guard.write_u32(gpccs_base + sr, 0x0000_0002);
        }
        tracing::info!("POST-done boot: GPCCS started");
    }

    // Start FECS.
    {
        let fecs_ctl = rd(FECS + 0x100);
        let start_reg = if fecs_ctl & (1 << 6) != 0 { 0x130 } else { 0x100 };
        wr(FECS + start_reg, 0x0000_0002);
        tracing::info!(
            fecs_cpuctl = format_args!("{fecs_ctl:#010x}"),
            start_reg = format_args!("{:#05x}", FECS + start_reg),
            "POST-done boot: FECS STARTCPU issued (external protocol)"
        );
    }

    // Fine-grained early trace with diagnostics.
    for delay_us in [10, 50, 100, 500, 1000, 5000, 10000u64] {
        std::thread::sleep(std::time::Duration::from_micros(delay_us));
        let c = rd(FECS + 0x100);
        let p = rd(FECS + 0x0A8);
        let e = rd(FECS + 0x018);
        let sp = rd(FECS + 0x0A0);
        let idle = rd(FECS + 0x04C);
        let dmactl = rd(FECS + 0x10C);
        let mb = rd(0x40_9800);
        tracing::info!(
            us = delay_us,
            cpuctl = format_args!("{c:#010x}"),
            pc = format_args!("{p:#010x}"),
            sp = format_args!("{sp:#010x}"),
            idle = format_args!("{idle:#010x}"),
            exci = format_args!("{e:#010x}"),
            dmactl = format_args!("{dmactl:#010x}"),
            mb0 = format_args!("{mb:#010x}"),
            "POST-done boot: FECS trace"
        );
        if mb & 0x1 != 0 {
            tracing::info!("POST-done boot: FECS ready (external) at {}μs!", delay_us);
        }
    }

    // ── Step 5: Poll CTXSW_MAILBOX0 bit 0 ──
    wr(0x40_0138, 0x0000_0000);
    wr(0x40_0140, 0x0000_0000);
    wr(0x40_0100, 0xFFFF_FFFF);

    let mut booted = false;
    for i in 0..2000 {
        std::thread::sleep(std::time::Duration::from_millis(1));

        if i % 50 == 0 {
            super::pri::clear_pri_ring_faults(guard.inner(), &guard.read_fn(), &|reg, val| {
                let _ = guard.write_u32(reg, val);
            });
            let mailbox0 = rd(0x40_9800);
            let cpuctl = rd(FECS + 0x100);
            let pc = rd(FECS + 0x0A8);
            let gpc0_cpuctl = rd(0x50_2000 + 0x100);

            tracing::info!(
                poll_ms = i,
                mailbox0 = format_args!("{mailbox0:#010x}"),
                cpuctl = format_args!("{cpuctl:#010x}"),
                pc = format_args!("{pc:#010x}"),
                gpc0_gpccs = format_args!("{gpc0_cpuctl:#010x}"),
                "POST-done boot: FECS poll (external — bit 0)"
            );

            let mb_ok = mailbox0 != 0xDEAD_DEAD && mailbox0 & 0xBAD0_0000 != 0xBAD0_0000;
            if mb_ok && mailbox0 & 0x1 != 0 {
                tracing::info!(
                    mailbox0 = format_args!("{mailbox0:#010x}"),
                    "POST-done boot: FECS ready (CTXSW_MAILBOX0 bit 0 set)"
                );
                booted = true;
                break;
            }

            let cpu_ok = cpuctl != 0xDEAD_DEAD && cpuctl & 0xBAD0_0000 != 0xBAD0_0000;
            if cpu_ok && cpuctl & 0x10 != 0 && i > 200 {
                let exci = rd(FECS + 0x018);
                tracing::warn!(
                    cpuctl = format_args!("{cpuctl:#010x}"),
                    pc = format_args!("{pc:#010x}"),
                    exci = format_args!("{exci:#010x}"),
                    "POST-done boot: FECS stuck in HRESET (0x10) — STARTCPU not consumed"
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
            "POST-done boot: FECS/GPCCS boot complete (external) — GR engine ready"
        );
    } else {
        let cpuctl = rd(FECS + 0x100);
        let gpccs_cpuctl = rd(GPCCS + 0x100);
        let mailbox0 = rd(0x40_9800);
        let exci = rd(FECS + 0x018);
        let pc = rd(FECS + 0x0A8);
        let gpc0_cpuctl = rd(0x50_2000 + 0x100);
        let gpc0_pc = rd(0x50_2000 + 0x0A8);
        let gpc0_exci = rd(0x50_2000 + 0x04C);
        tracing::warn!(
            cpuctl = format_args!("{cpuctl:#010x}"),
            pc = format_args!("{pc:#010x}"),
            exci = format_args!("{exci:#010x}"),
            gpccs = format_args!("{gpccs_cpuctl:#010x}"),
            gpc0_gpccs = format_args!("{gpc0_cpuctl:#010x}"),
            gpc0_pc = format_args!("{gpc0_pc:#010x}"),
            gpc0_exci = format_args!("{gpc0_exci:#010x}"),
            mailbox0 = format_args!("{mailbox0:#010x}"),
            "POST-done boot: FECS did not reach ready state (external FW)"
        );
    }
}

pub(super) fn kepler_load_and_boot_fecs(
    guard: &super::hardware_guard::GuardedBar<'_>,
    cached_gpc_count: u32,
    cached_tpc_total: u32,
    cached_tpc_counts: &[(u32, u32); 8],
) {
    use crate::nv::kepler_falcon;
    use crate::gsp::RegisterAccess;

    let r = guard.read_fn();
    let w = |reg: u32, val: u32| {
        if let Err(refusal) = guard.write_u32(reg, val) {
            tracing::error!(%refusal, "kepler_load_and_boot_fecs: guard refused write");
        }
    };

    const FECS: u32 = kepler_falcon::FECS_BASE;
    const GPCCS: u32 = kepler_falcon::GPCCS_BASE;

    // K80 is GK210B (GK110B die). We now prefer EXTERNAL firmware because
    // empirical testing shows both FECS and GPCCS start successfully via host
    // MMIO STARTCPU immediately after firmware PIO upload. External firmware
    // (/lib/firmware/nvidia/gk210/, ~15KB) is self-configuring and doesn't
    // need csdata in DMEM. Internal firmware (nouveau.ko embedded, ~3KB)
    // requires csdata and a different boot protocol.
    //
    // Priority: internal (FECS starts GPCCS via DMA — required on GK210B where
    // host MMIO STARTCPU is silently ignored for per-GPC GPCCS falcons) →
    // external → system → gk110 fallback
    let fw_search: &[(&str, &str, &str, &str, &str, bool)] = &[
        (
            "gk110-internal",
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

    const MIN_FECS_CODE_BYTES: usize = 8192;
    const MIN_GPCCS_CODE_BYTES: usize = 4096;

    for &(label, dir, fc, fd, gc, is_internal) in fw_search {
        let gd = fc.replace("fecs", "gpccs").replace("inst", "data").replace("code", "data");
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

        if let (Some(fc_data), Some(fd_data), Some(gc_data), Some(gd_data)) = (
            try_read(fc),
            try_read(fd),
            try_read(gc),
            try_read(&gd_name),
        ) {
            if !is_internal
                && (fc_data.len() < MIN_FECS_CODE_BYTES || gc_data.len() < MIN_GPCCS_CODE_BYTES)
            {
                tracing::warn!(
                    label,
                    fecs_code = fc_data.len(),
                    gpccs_code = gc_data.len(),
                    min_fecs = MIN_FECS_CODE_BYTES,
                    min_gpccs = MIN_GPCCS_CODE_BYTES,
                    "firmware set rejected — code blobs too small (likely truncated capture)"
                );
                continue;
            }
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

    let (Some(fecs_code), Some(fecs_data), Some(gpccs_code), Some(gpccs_data)) = (
        fecs_code, fecs_data, gpccs_code, gpccs_data,
    ) else {
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

    // Step 0: PMC GR engine reset + PGOB ungate in a single tight sequence.
    //
    // On GK210B, the GR HUB auto-clock-gates within nanoseconds of the last
    // PRI access. The PGOB disable sequence accesses GR registers for ~200ms,
    // keeping it alive. We chain the PMC reset, PGOB ungate, and CG disable
    // into a single uninterrupted burst so the domain stays accessible.
    {
        let pmc_pre = r(0x200);
        const GR_BIT: u32 = 1 << 12;
        w(0x200, pmc_pre & !GR_BIT);
        let _ = r(0x200);
        w(0x200, pmc_pre | GR_BIT);
        // PGOB ungate runs ~200ms of PRI writes, keeping GR HUB alive.
        // After it returns, immediately slam CG-disable writes — no logging.
        super::pgob::gk110_pgob_disable(guard);
        // IMMEDIATE: disable BLCG/SLCG before auto-gating kicks in
        w(0x40_41f0, 0x0000_0000);  // BLCG off
        w(0x40_41f4, 0x0000_0000);  // SLCG off
        w(0x40_9890, 0x0000_0000);  // FECS BLCG off
        w(0x40_98b0, 0x0000_0000);  // FECS BLCG2 off
        w(0x40_0500, 0x0000_0000);  // TRAP_EN off (keep GR HUB warm)
        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        let gr_hub = r(0x40_0000);
        let trap_rb = r(0x40_0500);
        tracing::info!(
            pmc_pre = format_args!("{pmc_pre:#010x}"),
            gr_hub = format_args!("{gr_hub:#010x}"),
            trap_rb = format_args!("{trap_rb:#010x}"),
            ok = gr_hub != 0xDEAD_DEAD && gr_hub & 0xBAD0_0000 != 0xBAD0_0000,
            "Step 0: PMC reset + PGOB + CG-disable burst"
        );
    }

    // Step 2: GPC MMU init (gf100_gr_init_gpc_mmu) — per-GPC.
    //
    // Broadcast 0x418xxx writes are silently dropped on GK210B.
    {
        let fb_mmu = r(0x10_0C80) & 0x0000_0001;
        for gpc in 0..8u32 {
            let base = 0x50_0000 + gpc * 0x8000;
            let probe = r(base + 0x2100);
            if probe == 0xDEAD_DEAD || probe & 0xBAD0_0000 == 0xBAD0_0000 {
                continue;
            }
            w(base + 0x0880, fb_mmu);
            w(base + 0x0890, 0x0000_0000);
            w(base + 0x0894, 0x0000_0000);
        }
    }

    // Step 3a: MMIO init table (gk110_gr_pack_mmio — hardcoded baseline).
    {
        let (gr_applied, gr_faulted) =
            super::kepler_gr_init::apply_gk110_gr_init(guard);
        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        tracing::info!(gr_applied, gr_faulted, "Step 3a: GR MMIO init (hardcoded gk110 pack)");
    }

    // Step 3b: Apply sw_nonctx.bin — GK210B-specific register overrides.
    {
        let (nonctx_applied, nonctx_skipped) =
            super::pri::apply_sw_nonctx(guard, "gk210");
        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        tracing::info!(
            nonctx_applied, nonctx_skipped,
            "Step 3b: sw_nonctx.bin GK210B overrides"
        );
    }

    // gf100_gr_wait_idle — Nouveau waits for GR idle after MMIO init.
    {
        let mut gr_idle = false;
        for _ in 0..2000 {
            let status = r(0x40_0700);
            if status != 0xDEAD_DEAD && status & 0x1 == 0 {
                gr_idle = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        tracing::info!(gr_idle, "gf100_gr_wait_idle after MMIO init (0x400700 bit 0)");
    }

    // Step 3c: Clock gating init (gk110_clkgate_pack — BLCG + SLCG).
    {
        let (cg_applied, cg_faulted) =
            super::kepler_gr_init::apply_gk110_clkgate(guard);
        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        tracing::info!(cg_applied, cg_faulted, "Step 3c: GK110 clock gating init");
    }

    // Step 3d: PGOB disable — ensures GR power domain is ungated.
    // Deferred to after MMIO + CG init because the PMC GR reset in Step 0
    // only briefly makes the GR HUB accessible; auto-clock-gating would
    // re-gate it if we ran PGOB first (which takes 200ms+).
    {
        let gr_hub_pre = r(0x40_0000);
        let gr_hub_ok = gr_hub_pre != 0xDEAD_DEAD && gr_hub_pre & 0xBAD0_0000 != 0xBAD0_0000;
        if !gr_hub_ok {
            tracing::info!(
                gr_hub = format_args!("{gr_hub_pre:#010x}"),
                "GR HUB still gated after MMIO init — running PGOB disable"
            );
            super::pgob::gk110_pgob_disable(guard);
            super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        }
        let gr_hub_post = r(0x40_0000);
        tracing::info!(
            gr_hub = format_args!("{gr_hub_post:#010x}"),
            ok = gr_hub_post != 0xDEAD_DEAD && gr_hub_post & 0xBAD0_0000 != 0xBAD0_0000,
            "Step 3d: GR HUB state after CG init + PGOB"
        );
    }

    // Step 4: Trap re-enable + interrupt clearing (matches nouveau ordering).
    w(0x40_0500, 0x0001_0001);  // re-enable traps
    w(0x40_0100, 0xFFFF_FFFF);  // GR_INTR: write-1-to-clear all
    w(0x40_013c, 0xFFFF_FFFF);  // GR_INTR_NONSTALL: clear
    w(0x40_0124, 0x0000_0002);  // INTR_NOTIFY_EN

    // Step 5: Exception handling + HWW ESR (matches gf100_gr_init).
    w(0x40_4000, 0xc000_0000);  // PD
    w(0x40_4600, 0xc000_0000);  // PD
    w(0x40_8030, 0xc000_0000);  // BE
    w(0x40_6018, 0xc000_0000);  // DS
    w(0x40_4490, 0xc000_0000);  // PRI
    w(0x40_5840, 0xc000_0000);  // DS_DEBUG
    w(0x40_5844, 0x00ff_ffff);  // DS_DEBUG

    // Interrupt clear + exception2 (nouveau: gf100_gr_init_exception2).
    w(0x40_0108, 0xFFFF_FFFF);  // GR_TRAP_NONSTALL
    w(0x40_0138, 0xFFFF_FFFF);  // GR_EXCEPTION
    w(0x40_0118, 0xFFFF_FFFF);
    w(0x40_0130, 0xFFFF_FFFF);
    w(0x40_011c, 0xFFFF_FFFF);  // EXCEPTION2
    w(0x40_0134, 0xFFFF_FFFF);  // EXCEPTION2

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
            gr_hub = format_args!("{gr_hub:#010x}[{}]", if is_ok(gr_hub) { "OK" } else { "FAULT" }),
            gr_units = format_args!("{gr_units:#010x}"),
            "Pre-upload state"
        );
    }

    // Immediate GR HUB recheck — is it still accessible right here?
    {
        let gh = r(0x40_0000);
        let fecs_r = r(FECS + 0x100);
        tracing::info!(
            gr_hub = format_args!("{gh:#010x}"),
            fecs_cpuctl = format_args!("{fecs_r:#010x}"),
            ok = gh != 0xDEAD_DEAD && gh & 0xBAD0_0000 != 0xBAD0_0000,
            "GR HUB IMMEDIATE recheck (no writes between this and Pre-upload state)"
        );
    }

    // MC_UNK260 (0x260) — GR method dispatch control.
    // Nouveau brackets firmware upload with unk260=0 (disable) / unk260=1 (enable).
    // Earlier we skipped this because 0x400000 returned 0xbadf1002 with unk260=0,
    // but that turned out to be "GR not initialized" status, not "GR gated."
    // FECS registers at 0x409xxx remain accessible with unk260=0, and PIO
    // uploads work correctly.  Restore the Nouveau bracket.
    w(0x260, 0);
    tracing::info!("MC_UNK260=0 (GR method dispatch disabled for firmware upload)");

    // Deep-dive diagnostics: read ENGCTL (0x058), SCTL (0x240), and
    // other undocumented control registers on GPCCS to understand
    // why STARTCPU is refused.
    {
        let bar0 = guard.inner();
        let gpc0 = 0x50_2000u32;
        let rd = |off: u32| -> u32 { bar0.read_u32(off as usize).unwrap_or(0xDEAD_DEAD) };

        let engctl_058 = rd(gpc0 + 0x058);
        let sctl_240   = rd(gpc0 + 0x240);
        let cpuctl      = rd(gpc0 + 0x100);
        let dmactl      = rd(gpc0 + 0x10C);
        let itfen       = rd(gpc0 + 0x048);
        let hwcfg       = rd(gpc0 + 0x108);
        let hwcfg2      = rd(gpc0 + 0x00C);
        let irqstat     = rd(gpc0 + 0x008);
        let falcon_ver  = rd(gpc0 + 0x004);
        let exci        = rd(gpc0 + 0x04C);
        let unk_3c0     = rd(gpc0 + 0x3C0);
        let bootvec     = rd(gpc0 + 0x104);
        let debugi      = rd(gpc0 + 0x0A8);

        // Also read FECS for comparison
        let f_engctl  = rd(FECS + 0x058);
        let f_sctl    = rd(FECS + 0x240);
        let f_cpuctl  = rd(FECS + 0x100);

        tracing::info!(
            engctl_058 = format_args!("{engctl_058:#010x}"),
            sctl_240 = format_args!("{sctl_240:#010x}"),
            cpuctl = format_args!("{cpuctl:#010x}"),
            dmactl = format_args!("{dmactl:#010x}"),
            itfen = format_args!("{itfen:#010x}"),
            hwcfg = format_args!("{hwcfg:#010x}"),
            hwcfg2 = format_args!("{hwcfg2:#010x}"),
            irqstat = format_args!("{irqstat:#010x}"),
            falcon_ver = format_args!("{falcon_ver:#010x}"),
            "GPC0 GPCCS deep-dive (pre-upload)"
        );
        tracing::info!(
            exci = format_args!("{exci:#010x}"),
            unk_3c0 = format_args!("{unk_3c0:#010x}"),
            bootvec = format_args!("{bootvec:#010x}"),
            debugi = format_args!("{debugi:#010x}"),
            f_engctl = format_args!("{f_engctl:#010x}"),
            f_sctl = format_args!("{f_sctl:#010x}"),
            f_cpuctl = format_args!("{f_cpuctl:#010x}"),
            "GPCCS vs FECS control state (pre-upload)"
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

    // Diagnose GPCCS Falcon state before upload.
    {
        let gpc0_gpccs_dmactl = r(0x50_2000 + 0x10C);
        let gpc0_gpccs_cpuctl = r(0x50_2000 + 0x100);
        tracing::info!(
            gpc0_dmactl = format_args!("{gpc0_gpccs_dmactl:#010x}"),
            gpc0_cpuctl = format_args!("{gpc0_gpccs_cpuctl:#010x}"),
            "Pre-upload GPCCS state (post ENGCTL HRESET)"
        );
    }

    // GR HUB check before firmware upload
    {
        let gh = r(0x40_0000);
        tracing::info!(
            gr_hub = format_args!("{gh:#010x}"),
            ok = gh != 0xDEAD_DEAD && gh & 0xBAD0_0000 != 0xBAD0_0000,
            "GR HUB check BEFORE firmware upload"
        );
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

    // 4. Upload GPCCS DMEM + IMEM — per-GPC PIO.
    //
    // Critical finding: broadcast IMEM PIO writes (0x41a180/184/188) land
    // the DATA correctly in per-GPC IMEM but may NOT propagate the IMEM TAGS
    // (0x188). On Falcon v3, each 256-byte IMEM block must have a valid tag
    // for the CPU to fetch instructions. Missing tags = STARTCPU silently
    // refused.
    //
    // Solution: upload GPCCS firmware via per-GPC PIO addresses so tags
    // are written directly to each GPC's GPCCS IMEM.
    {
        // First, broadcast upload (for any GPCs that DO accept broadcast tags)
        if let Err(e) = kepler_falcon::upload_dmem(regs, GPCCS, 0, &gpccs_data) {
            tracing::warn!(error = %e, "GPCCS DMEM broadcast upload failed");
        }
        if let Err(e) = kepler_falcon::upload_imem(regs, GPCCS, 0, &gpccs_code) {
            tracing::warn!(error = %e, "GPCCS IMEM broadcast upload failed");
        }

        // Then per-GPC upload (ensures tags land correctly on GK210B)
        let mut gpc_uploaded = 0u32;
        for gpc in 0..8u32 {
            let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
            let probe = r(gpccs_base + 0x100);
            if probe == 0xDEAD_DEAD || probe & 0xBAD0_0000 == 0xBAD0_0000 {
                continue;
            }
            if let Err(e) = kepler_falcon::upload_dmem(regs, gpccs_base, 0, &gpccs_data) {
                tracing::warn!(gpc, error = %e, "GPCCS per-GPC DMEM upload failed");
                continue;
            }
            if let Err(e) = kepler_falcon::upload_imem(regs, gpccs_base, 0, &gpccs_code) {
                tracing::warn!(gpc, error = %e, "GPCCS per-GPC IMEM upload failed");
                continue;
            }
            gpc_uploaded += 1;
        }
        tracing::info!(gpc_uploaded, "GPCCS per-GPC firmware upload (tags + data)");

    }

    // Verify GPCCS upload via broadcast readback AND per-GPC direct readback.
    {
        const IMEM_READ_AUTOINC: u32 = 1 << 25;
        const GPC0_GPCCS: u32 = 0x50_2000;
        let bar0 = guard.inner();

        // Broadcast readback
        let _ = bar0.write_u32((GPCCS as usize) + 0x180, IMEM_READ_AUTOINC);
        let bcast_word = bar0.read_u32((GPCCS as usize) + 0x184).unwrap_or(0xDEAD_DEAD);

        // Per-GPC0 direct readback
        let _ = bar0.write_u32((GPC0_GPCCS + 0x180) as usize, IMEM_READ_AUTOINC as u32);
        let gpc0_word = bar0.read_u32((GPC0_GPCCS + 0x184) as usize).unwrap_or(0xDEAD_DEAD);

        // Single-word PIO round-trip test on GPC0 GPCCS DMEM[0].
        // Save original, write test pattern, verify, then restore.
        let _ = bar0.write_u32((GPC0_GPCCS + 0x1C0) as usize, 1 << 25); // DMEMC: addr=0, AINCR
        let dmem0_orig = bar0.read_u32((GPC0_GPCCS + 0x1C4) as usize).unwrap_or(0xDEAD_DEAD);
        let _ = bar0.write_u32((GPC0_GPCCS + 0x1C0) as usize, (1 << 24) | (1 << 30));
        let _ = bar0.write_u32((GPC0_GPCCS + 0x1C4) as usize, 0xCAFE_BEEF_u32);
        let _ = bar0.write_u32((GPC0_GPCCS + 0x1C0) as usize, 1 << 25);
        let pio_test = bar0.read_u32((GPC0_GPCCS + 0x1C4) as usize).unwrap_or(0xDEAD_DEAD);
        // Restore original DMEM[0] so firmware data isn't corrupted.
        let _ = bar0.write_u32((GPC0_GPCCS + 0x1C0) as usize, (1 << 24) | (1 << 30));
        let _ = bar0.write_u32((GPC0_GPCCS + 0x1C4) as usize, dmem0_orig);

        let expected_first = u32::from_le_bytes([
            gpccs_code[0], gpccs_code[1], gpccs_code[2], gpccs_code[3],
        ]);
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
            expected = format_args!("{:08x} {:08x} {:08x} {:08x}", expected[0], expected[1], expected[2], expected[3]),
            imem_ok = ok,
            "FECS IMEM readback after upload"
        );
    }

    // Nouveau v6.8 gf100_gr_init_fw() does NOT set ITFEN or WDT.
    // Leaving ITFEN=0 (default after PMC reset) lets the falcon CPU fetch
    // directly from IMEM physical addressing without going through the
    // instruction buffer or tag validation.  Previous attempts with ITFEN=3
    // caused the CPU to stall at PC=0 (idle exception) — likely because
    // the instruction buffer requires valid tag state that PMC reset may
    // not provide.

    // GR HUB check after ITFEN+WDT writes
    {
        let gh = r(0x40_0000);
        tracing::info!(
            gr_hub = format_args!("{gh:#010x}"),
            ok = gh != 0xDEAD_DEAD && gh & 0xBAD0_0000 != 0xBAD0_0000,
            "GR HUB check AFTER upload + ITFEN+WDT"
        );
    }

    // Re-enable GR method dispatch (match Nouveau: unk260=1 after upload).
    w(0x260, 1);
    tracing::info!("MC_UNK260=1 (GR method dispatch re-enabled after upload)");

    // Clear PRI ring faults accumulated during firmware upload.
    super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

    // GR HUB check after PRI ring fault clear
    {
        let gh = r(0x40_0000);
        tracing::info!(
            gr_hub = format_args!("{gh:#010x}"),
            ok = gh != 0xDEAD_DEAD && gh & 0xBAD0_0000 != 0xBAD0_0000,
            "GR HUB check AFTER PRI fault clear"
        );
    }

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
            let wr_fn = |off: u32, val: u32| { let _ = bar0.write_u32(off as usize, val); };

            super::kepler_csdata::load_csdata(&rd_fn, &wr_fn,
                super::kepler_csdata::GK110B_GRCTX_PACK_HUB,
                FECS, 0x000, 0x00_0000);
            super::kepler_csdata::load_csdata(&rd_fn, &wr_fn,
                super::kepler_csdata::GK110B_GRCTX_PACK_GPC_0,
                GPCCS, 0x000, 0x41_8000);
            super::kepler_csdata::load_csdata(&rd_fn, &wr_fn,
                super::kepler_csdata::GK110B_GRCTX_PACK_GPC_1,
                GPCCS, 0x000, 0x41_8000);
            super::kepler_csdata::load_csdata(&rd_fn, &wr_fn,
                super::kepler_csdata::GK110B_GRCTX_PACK_TPC,
                GPCCS, 0x004, 0x41_9800);
            super::kepler_csdata::load_csdata(&rd_fn, &wr_fn,
                super::kepler_csdata::GK110B_GRCTX_PACK_PPC,
                GPCCS, 0x008, 0x41_BE00);

            tracing::info!("Internal boot: csdata register lists loaded (5 GK110B packs)");
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

        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
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
            let gpc0_state = (r(0x50_2000 + 0x100), r(0x50_2000 + 0x048), r(0x50_2000 + 0x10C));
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
                let probe = bar0.read_u32((gpccs_base + 0x100) as usize)
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
                let probe = bar0.read_u32((gpccs_base + 0x100) as usize)
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

/// `RegisterAccess` adapter routing through `GuardedBar` — writes go through
/// the blocklist/canary checks, reads through the link-alive check.
struct GuardedBarRegAccess<'a>(&'a super::hardware_guard::GuardedBar<'a>);

impl crate::gsp::RegisterAccess for GuardedBarRegAccess<'_> {
    fn read_u32(&self, offset: u32) -> Result<u32, crate::gsp::ApplyError> {
        self.0.read_u32(offset).map_err(|refusal| crate::gsp::ApplyError::MmioFailed {
            offset,
            detail: refusal.to_string(),
        })
    }

    fn write_u32(&mut self, offset: u32, value: u32) -> Result<(), crate::gsp::ApplyError> {
        self.0.write_u32(offset, value).map_err(|refusal| crate::gsp::ApplyError::MmioFailed {
            offset,
            detail: refusal.to_string(),
        })
    }
}