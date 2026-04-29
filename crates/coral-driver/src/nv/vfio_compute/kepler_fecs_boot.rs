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

    // ── Step 2: ENGCTL cycle — resets Falcon CPU + caches, not GPC power ──
    // Without this, the instruction cache is stale from nouveau's firmware
    // and STARTCPU executes cached NOPs instead of new IMEM content.
    {
        wr(FECS + 0x3C0, 0x01);
        std::thread::sleep(std::time::Duration::from_millis(2));
        wr(FECS + 0x3C0, 0x00);
        std::thread::sleep(std::time::Duration::from_millis(2));

        for gpc in 0..cached_gpc_count.min(8) {
            let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
            let probe = rd(gpccs_base + 0x100);
            if probe == 0xDEAD_DEAD || probe & 0xBAD0_0000 == 0xBAD0_0000 { continue; }
            wr(gpccs_base + 0x3C0, 0x01);
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
        for gpc in 0..cached_gpc_count.min(8) {
            let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
            let probe = rd(gpccs_base + 0x100);
            if probe == 0xDEAD_DEAD || probe & 0xBAD0_0000 == 0xBAD0_0000 { continue; }
            wr(gpccs_base + 0x3C0, 0x00);
        }
        std::thread::sleep(std::time::Duration::from_millis(2));

        // Wait for IMEM/DMEM scrub to complete (DMACTL bits [2:1]).
        for _ in 0..200 {
            let fecs_dmactl = rd(FECS + 0x10C);
            if fecs_dmactl & 0x06 == 0 { break; }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let post_engctl = rd(FECS + 0x100);
        tracing::info!(
            fecs_cpuctl = format_args!("{post_engctl:#010x}"),
            gpc0_tpc_post = format_args!("{:#010x}", rd(0x50_2608)),
            "POST-done boot: ENGCTL cycle complete"
        );
    }

    // ── Step 3: Enable ITFEN and upload firmware ──
    wr(FECS + 0x048, 0x3);
    wr(GPCCS + 0x048, 0x3);
    for gpc in 0..cached_gpc_count.min(8) {
        wr(0x50_0000 + gpc * 0x8000 + 0x2000 + 0x048, 0x3);
    }

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

        regs.write_u32(0x260, 0x2000_0000).ok();
    }
    tracing::info!("POST-done boot: FECS + GPCCS firmware uploaded");

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
                    "POST-done boot: FECS HALTED — firmware hit exception"
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
            if fc_data.len() < MIN_FECS_CODE_BYTES || gc_data.len() < MIN_GPCCS_CODE_BYTES {
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
        let (gr_applied, gr_faulted) =
            super::kepler_gr_init::apply_gk110_gr_init(guard);
        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        tracing::info!(gr_applied, gr_faulted, "Step 3a: GR MMIO init (hardcoded gk110 pack)");
    }

    // Step 3b: Apply sw_nonctx.bin — GK210B-specific register overrides.
    //
    // Nouveau applies gf100_gr_mmio(gr, gr->sw_nonctx) AFTER the hardcoded
    // pack. These firmware-provided values override the GK110 defaults with
    // GK210B-specific register configurations that FECS firmware expects
    // during boot. Without these, FECS traps immediately (EXCI=1 at PC=0).
    {
        let (nonctx_applied, nonctx_skipped) =
            super::pri::apply_sw_nonctx(guard, "gk210");
        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        tracing::info!(
            nonctx_applied, nonctx_skipped,
            "Step 3b: sw_nonctx.bin GK210B overrides"
        );
    }

    // Step 3c: Clock gating init (gk110_clkgate_pack — BLCG + SLCG).
    //
    // Nouveau applies these via nvkm_therm_clkgate_init() after MMIO init.
    // Without them, GPC Falcon CPU clocks remain gated after PMC GR reset,
    // causing GPCCS STARTCPU to be silently ignored.
    {
        let (cg_applied, cg_faulted) =
            super::kepler_gr_init::apply_gk110_clkgate(guard);
        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        tracing::info!(cg_applied, cg_faulted, "Step 3c: GK110 clock gating init");
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
            if rd_diag(FECS + 0x10C) & 0x06 == 0 { break; }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    // 1. Disable GR method dispatch for firmware load (nouveau nvkm_mc_unk260(0))
    regs.write_u32(0x260, 0).ok();

    // After PMC GR reset, FECS/GPCCS are already in initial HALT state
    // (CPUCTL=0x10). ENGCTL HRESET is NOT needed and can be harmful:
    // it scrubs IMEM/DMEM (destroying PIO-uploaded firmware) and on GK210B
    // it resets ITFEN to 0x00, which may prevent instruction fetch.
    //
    // Nouveau's gf100_gr_init_ctxctl_ext does NOT use ENGCTL.
    // For internal protocol: also skip (rely on PMC GR reset state).
    {
        let fecs_cpuctl_post = r(FECS + 0x100);
        let gpc0_cpuctl_post = r(0x50_2000 + 0x100);
        tracing::info!(
            fecs = format_args!("{fecs_cpuctl_post:#010x}"),
            gpc0_gpccs = format_args!("{gpc0_cpuctl_post:#010x}"),
            "Pre-upload state (FECS should be 0x10 = HALTED after PMC reset)"
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
                if r(0x50_2000 + 0x10C) & 0x06 == 0 { scrub_ok = true; break; }
            }
            tracing::info!(scrub_ok, "GPCCS GPC0 memory scrub wait");
        }
    }

    // NOTE: ITFEN (0x048) is intentionally set AFTER firmware upload,
    // right before STARTCPU. Setting it earlier can auto-start the Falcon
    // (consuming a pending STARTCPU) before IMEM has been loaded, causing
    // the CPU to execute zeroed/scrubbed memory and stall at PC=0.

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

    // 4. Re-enable GR method dispatch (nouveau gk104_mc_unk260: 0x20000000)
    regs.write_u32(0x260, 0x2000_0000).ok();

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
            let _ = bar0.write_u32((FECS + 0x048) as usize, 0x0000_0003); // ITFEN = 3

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
                tracing::warn!("STARTCPU ignored (HW reset halt) — trying ENGCTL deassert + IINVAL");

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

        tracing::info!("Using EXTERNAL firmware boot protocol (nouveau-aligned: no ENGCTL/ITFEN/IINVAL)");

        // Modified nouveau-aligned external boot: gf100_gr_init_ctxctl_ext ordering
        // with explicit ITFEN=0x03 for all Falcons before any STARTCPU.
        //
        // GK210B powers on with ITFEN=0x00 after PMC GR reset. Without
        // enabling instruction fetch, STARTCPU transitions the Falcon out of
        // halt (CPUCTL: 0x10→0x00) but the CPU stalls at PC=0 because it
        // cannot fetch from IMEM.

        super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        w(0x40_0100, 0xFFFF_FFFF); // GR_INTR: clear all

        // Step 0: Enable ITFEN on ALL Falcons BEFORE any STARTCPU.
        w(FECS + 0x048, 0x0000_0003);
        w(GPCCS + 0x048, 0x0000_0003);
        for gpc in 0..8u32 {
            let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
            let probe = r(gpccs_base + 0x100);
            if probe == 0xDEAD_DEAD || probe & 0xBAD0_0000 == 0xBAD0_0000 { continue; }
            w(gpccs_base + 0x048, 0x0000_0003);
        }

        // Step 0.5: GPCCS ENGCTL reset cycle.
        //
        // mmiotrace reveals nouveau NEVER initializes GR on headless K80 —
        // zero PGRAPH register accesses. Our PMC GR reset (bit 12 of 0x200)
        // first-enables PGRAPH, leaving GPCCS Falcons in power-on reset state
        // (not initial halt). STARTCPU only works from initial/SW halt.
        //
        // ENGCTL cycle (assert → delay → deassert) transitions the Falcon
        // through its reset sequence into initial halt.
        {
            let gpccs_cpuctl_before = r(0x50_2000 + 0x100);
            let gpccs_engctl_before = r(0x50_2000 + 0x3C0);

            // Broadcast ENGCTL assert (reset all GPCCS Falcons).
            w(GPCCS + 0x3C0, 0x0000_0001);
            std::thread::sleep(std::time::Duration::from_millis(2));
            // Broadcast ENGCTL deassert (release from reset → initial halt).
            w(GPCCS + 0x3C0, 0x0000_0000);
            std::thread::sleep(std::time::Duration::from_millis(2));

            let gpccs_cpuctl_after = r(0x50_2000 + 0x100);
            let gpccs_engctl_after = r(0x50_2000 + 0x3C0);
            tracing::info!(
                cpuctl_before = format_args!("{gpccs_cpuctl_before:#010x}"),
                engctl_before = format_args!("{gpccs_engctl_before:#010x}"),
                cpuctl_after = format_args!("{gpccs_cpuctl_after:#010x}"),
                engctl_after = format_args!("{gpccs_engctl_after:#010x}"),
                "GPCCS ENGCTL reset cycle (power-on reset → initial halt)"
            );
        }

        // ENGCTL cycle clears IMEM/DMEM/ITFEN — must re-upload firmware.
        {
            let regs: &mut dyn crate::gsp::RegisterAccess =
                &mut GuardedBarRegAccess(guard);
            if let Err(e) = kepler_falcon::upload_dmem(regs, GPCCS, 0, &gpccs_data) {
                tracing::warn!(error = %e, "GPCCS DMEM re-upload after ENGCTL failed");
            }
            if let Err(e) = kepler_falcon::upload_imem(regs, GPCCS, 0, &gpccs_code) {
                tracing::warn!(error = %e, "GPCCS IMEM re-upload after ENGCTL failed");
            }
            tracing::info!("GPCCS firmware re-uploaded after ENGCTL cycle");
        }

        // Re-enable ITFEN on all GPCCS Falcons (ENGCTL clears it).
        w(GPCCS + 0x048, 0x0000_0003);
        for gpc in 0..8u32 {
            let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
            let itfen = r(gpccs_base + 0x048);
            if itfen == 0xDEAD_DEAD || itfen & 0xBAD0_0000 == 0xBAD0_0000 {
                continue;
            }
            w(gpccs_base + 0x048, 0x0000_0003);
        }

        // Step 1: GPCCS BOOTVEC=0, DMACTL=0 via broadcast, then STARTCPU per-GPC.
        //
        // On GK210B, the GR_GPC broadcast address (0x41A000) forwards
        // ITFEN, DMACTL, and BOOTVEC writes correctly but silently drops CPUCTL
        // writes. STARTCPU must be written to each per-GPC GPCCS directly.
        w(GPCCS + 0x104, 0x0000_0000); // BOOTVEC = 0 (broadcast)
        w(GPCCS + 0x10C, 0x0000_0000); // DMACTL = 0 (broadcast)
        {
            let mut gpccs_started = 0u32;
            for gpc in 0..8u32 {
                let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
                let itfen = r(gpccs_base + 0x048);
                if itfen == 0xDEAD_DEAD || itfen & 0xBAD0_0000 == 0xBAD0_0000 {
                    continue;
                }
                let _ = guard.write_u32(gpccs_base + 0x104, 0u32); // BOOTVEC = 0
                let _ = guard.write_u32(gpccs_base + 0x10C, 0u32); // DMACTL = 0
                let _ = guard.write_u32(gpccs_base + 0x100, 0x0000_0002u32); // STARTCPU
                gpccs_started += 1;
            }
            tracing::info!(gpccs_started, "GPCCS per-GPC STARTCPU issued");
        }

        // Step 2: Topology registers (written AFTER GPCCS start, per nouveau).
        {
            let pri_gpc_count = r(0x12_0074);
            let mut gpc_disable_mask: u32 = 0;
            for gpc in 0..pri_gpc_count.min(8) {
                let is_active = use_tpc_counts.iter()
                    .take(use_gpc_count as usize)
                    .any(|&(g, _)| g == gpc);
                if !is_active {
                    gpc_disable_mask |= 1 << gpc;
                }
            }

            w(0x40_9604, use_gpc_count);
            w(0x40_9608, use_tpc_total);
            w(0x40_960C, gpc_disable_mask);

            for (idx, &(_, tpc_nr)) in use_tpc_counts.iter()
                .take(use_gpc_count as usize)
                .enumerate()
            {
                w(0x40_9640 + (idx as u32) * 4, tpc_nr);
                w(0x40_9680 + (idx as u32) * 4, tpc_nr);
            }

            let fbp_count = r(0x12_0078);
            let fbp_count_val = if fbp_count != 0xDEAD_DEAD && fbp_count & 0xBAD0_0000 != 0xBAD0_0000 {
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
                "FECS topology registers (external)"
            );
        }

        // Step 3: FECS BOOTVEC=0, DMACTL=0, then STARTCPU (nvkm_falcon_start).
        w(FECS + 0x104, 0x0000_0000); // BOOTVEC = 0
        w(FECS + 0x10C, 0x0000_0000); // DMACTL = 0
        {
            let fecs_cpuctl_pre = r(FECS + 0x100);
            let itfen_verify = r(FECS + 0x048);
            let gpccs_cpuctl = r(0x50_2000 + 0x100);
            let gpccs_itfen = r(0x50_2000 + 0x048);
            w(FECS + 0x100, 0x0000_0002);
            std::thread::sleep(std::time::Duration::from_micros(100));
            let fecs_cpuctl_post = r(FECS + 0x100);
            let fecs_idle = r(FECS + 0x04C);
            let gpccs_cpuctl_post = r(0x50_2000 + 0x100);
            tracing::info!(
                fecs_cpuctl_pre = format_args!("{fecs_cpuctl_pre:#010x}"),
                fecs_cpuctl_post = format_args!("{fecs_cpuctl_post:#010x}"),
                fecs_idle = format_args!("{fecs_idle:#010x}"),
                fecs_itfen = format_args!("{itfen_verify:#010x}"),
                gpccs_cpuctl_pre = format_args!("{gpccs_cpuctl:#010x}"),
                gpccs_cpuctl_post = format_args!("{gpccs_cpuctl_post:#010x}"),
                gpccs_itfen = format_args!("{gpccs_itfen:#010x}"),
                "FECS STARTCPU issued (ITFEN first, then GPCCS start, then FECS start)"
            );
        }

        // Step 4: Clear handshake register (nouveau does this AFTER STARTCPU).
        w(CTXSW_MAILBOX0, 0x0000_0000);

        // Diagnostic: focused GPC0 GPCCS STARTCPU experiment.
        std::thread::sleep(std::time::Duration::from_millis(1));
        {
            const GPC0_GPCCS: u32 = 0x50_2000;
            let gpc0_pre = r(GPC0_GPCCS + 0x100);
            let gpc0_itfen = r(GPC0_GPCCS + 0x048);
            let gpc0_dmactl = r(GPC0_GPCCS + 0x10C);

            // Try STARTCPU again directly on GPC0 GPCCS (nvkm_falcon_start sequence).
            w(GPC0_GPCCS + 0x104, 0x0000_0000); // BOOTVEC = 0
            w(GPC0_GPCCS + 0x10C, 0x0000_0000); // DMACTL = 0
            w(GPC0_GPCCS + 0x100, 0x0000_0002); // STARTCPU
            let gpc0_post = r(GPC0_GPCCS + 0x100);

            // Check FECS real state.
            let fecs_cpuctl = r(FECS + 0x100);
            let fecs_trap = r(FECS + 0x018);
            let fecs_epc = r(FECS + 0x01C);

            // Try reading Falcon v3 actual UC_PC at offset 0x028.
            let fecs_uc_pc = r(FECS + 0x028);
            let gpc0_uc_pc = r(GPC0_GPCCS + 0x028);

            tracing::info!(
                gpc0_cpuctl_pre = format_args!("{gpc0_pre:#010x}"),
                gpc0_cpuctl_post = format_args!("{gpc0_post:#010x}"),
                gpc0_itfen = format_args!("{gpc0_itfen:#010x}"),
                gpc0_dmactl = format_args!("{gpc0_dmactl:#010x}"),
                fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
                fecs_trap = format_args!("{fecs_trap:#010x}"),
                fecs_epc = format_args!("{fecs_epc:#010x}"),
                fecs_uc_pc = format_args!("{fecs_uc_pc:#010x}"),
                gpc0_uc_pc = format_args!("{gpc0_uc_pc:#010x}"),
                "Post-start diagnostic (GPC0 GPCCS retry + Falcon UC_PC)"
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