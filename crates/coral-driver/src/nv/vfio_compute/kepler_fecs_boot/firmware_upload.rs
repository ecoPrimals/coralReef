// SPDX-License-Identifier: AGPL-3.0-or-later
//! FECS hub + per-GPC GPCCS PIO upload with IMEM verification (cold boot path).

use super::super::hardware_guard::GuardedBar;

#[must_use]
pub(super) fn upload_kepler_firmware(
    guard: &GuardedBar<'_>,
    blobs: &super::firmware::KeplerFirmwareBlobs,
) -> bool {
    use crate::gsp::RegisterAccess;
    use crate::nv::kepler_falcon;

    use super::reg_access::GuardedBarRegAccess;

    const FECS: u32 = kepler_falcon::FECS_BASE;
    const GPCCS: u32 = kepler_falcon::GPCCS_BASE;

    let fecs_code = blobs.fecs_code.as_slice();
    let fecs_data = blobs.fecs_data.as_slice();
    let gpccs_code = blobs.gpccs_code.as_slice();
    let gpccs_data = blobs.gpccs_data.as_slice();

    let r = guard.read_fn();
    let w = |reg: u32, val: u32| {
        if let Err(refusal) = guard.write_u32(reg, val) {
            tracing::error!(%refusal, "kepler_load_and_boot_fecs: guard refused write");
        }
    };

    let regs: &mut dyn RegisterAccess = &mut GuardedBarRegAccess(guard);
    // 3. Upload FECS (hub) DMEM + IMEM
    if let Err(e) = kepler_falcon::upload_dmem(regs, FECS, 0, fecs_data) {
        tracing::warn!(error = %e, "FECS DMEM upload failed");
        return false;
    }
    if let Err(e) = kepler_falcon::upload_imem(regs, FECS, 0, fecs_code) {
        tracing::warn!(error = %e, "FECS IMEM upload failed");
        return false;
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
        if let Err(e) = kepler_falcon::upload_dmem(regs, GPCCS, 0, gpccs_data) {
            tracing::warn!(error = %e, "GPCCS DMEM broadcast upload failed");
        }
        if let Err(e) = kepler_falcon::upload_imem(regs, GPCCS, 0, gpccs_code) {
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
            if let Err(e) = kepler_falcon::upload_dmem(regs, gpccs_base, 0, gpccs_data) {
                tracing::warn!(gpc, error = %e, "GPCCS per-GPC DMEM upload failed");
                continue;
            }
            if let Err(e) = kepler_falcon::upload_imem(regs, gpccs_base, 0, gpccs_code) {
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
        let bcast_word = bar0
            .read_u32((GPCCS as usize) + 0x184)
            .unwrap_or(0xDEAD_DEAD);

        // Per-GPC0 direct readback
        let _ = bar0.write_u32((GPC0_GPCCS + 0x180) as usize, IMEM_READ_AUTOINC);
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
        for word in &mut rb {
            *word = regs.read_u32(FECS + 0x184).unwrap_or(0xDEAD_DEAD);
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
    super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

    // GR HUB check after PRI ring fault clear
    {
        let gh = r(0x40_0000);
        tracing::info!(
            gr_hub = format_args!("{gh:#010x}"),
            ok = gh != 0xDEAD_DEAD && gh & 0xBAD0_0000 != 0xBAD0_0000,
            "GR HUB check AFTER PRI fault clear"
        );
    }

    true
}
