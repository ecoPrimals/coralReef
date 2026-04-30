// SPDX-License-Identifier: AGPL-3.0-or-later
//! POST-done external-firmware FECS/GPCCS sequence (upload → topology → STARTCPU → poll).

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
pub(in crate::nv::vfio_compute) fn kepler_post_done_boot_fecs(
    guard: &super::super::hardware_guard::GuardedBar<'_>,
    cached_gpc_count: u32,
    cached_tpc_total: u32,
    cached_tpc_counts: &[(u32, u32); 8],
) {
    use crate::nv::kepler_falcon;

    use super::reg_access::GuardedBarRegAccess;

    const FECS: u32 = kepler_falcon::FECS_BASE;
    const GPCCS: u32 = kepler_falcon::GPCCS_BASE;

    let bar0 = guard.inner();
    let rd = |off: u32| -> u32 { bar0.read_u32(off as usize).unwrap_or(0xDEAD_DEAD) };
    let wr = |off: u32, val: u32| {
        let _ = bar0.write_u32(off as usize, val);
    };

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
    let try_read =
        |name: &str| -> Option<Vec<u8>> { std::fs::read(format!("{fw_dir}/{name}")).ok() };
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
        for word in &mut rb {
            *word = regs.read_u32(FECS + 0x184).unwrap_or(0xDEAD_DEAD);
        }
        let expected = [
            u32::from_le_bytes([fecs_code[0], fecs_code[1], fecs_code[2], fecs_code[3]]),
            u32::from_le_bytes([fecs_code[4], fecs_code[5], fecs_code[6], fecs_code[7]]),
            u32::from_le_bytes([fecs_code[8], fecs_code[9], fecs_code[10], fecs_code[11]]),
            u32::from_le_bytes([fecs_code[12], fecs_code[13], fecs_code[14], fecs_code[15]]),
        ];
        tracing::info!(
            readback = format_args!("{:08x} {:08x} {:08x} {:08x}", rb[0], rb[1], rb[2], rb[3]),
            expected = format_args!(
                "{:08x} {:08x} {:08x} {:08x}",
                expected[0], expected[1], expected[2], expected[3]
            ),
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
            let is_active = cached_tpc_counts
                .iter()
                .take(cached_gpc_count as usize)
                .any(|&(g, _)| g == gpc);
            if !is_active {
                gpc_disable_mask |= 1 << gpc;
            }
        }

        wr(0x40_9604, cached_gpc_count);
        wr(0x40_9608, cached_tpc_total);
        wr(0x40_960C, gpc_disable_mask);

        for (idx, &(_, tpc_nr)) in cached_tpc_counts
            .iter()
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
    super::super::pri::clear_pri_ring_faults(guard.inner(), &guard.read_fn(), &|reg, val| {
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
        let start_reg = if gpccs_cpuctl & (1 << 6) != 0 {
            0x130
        } else {
            0x100
        };
        wr(GPCCS + start_reg, 0x0000_0002);

        for gpc in 0..cached_gpc_count.min(8) {
            let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
            let probe = rd(gpccs_base + 0x100);
            if probe == 0xDEAD_DEAD || probe & 0xBAD0_0000 == 0xBAD0_0000 {
                continue;
            }
            let sr = if probe & (1 << 6) != 0 { 0x130 } else { 0x100 };
            let _ = guard.write_u32(gpccs_base + sr, 0x0000_0002);
        }
        tracing::info!("POST-done boot: GPCCS started");
    }

    // Start FECS.
    {
        let fecs_ctl = rd(FECS + 0x100);
        let start_reg = if fecs_ctl & (1 << 6) != 0 {
            0x130
        } else {
            0x100
        };
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
            super::super::pri::clear_pri_ring_faults(
                guard.inner(),
                &guard.read_fn(),
                &|reg, val| {
                    let _ = guard.write_u32(reg, val);
                },
            );
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
