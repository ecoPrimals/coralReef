// SPDX-License-Identifier: AGPL-3.0-only

//! Boot strategies: mailbox command, direct FECS, HRESET, EMEM, nouveau-style SEC2.

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

use super::boot_result::{AcrBootResult, make_fail_result};
use super::firmware::AcrFirmwareSet;
use super::sec2_hal::{
    Sec2Probe, falcon_dmem_upload, falcon_engine_reset, falcon_imem_upload_nouveau,
    falcon_prepare_physical_dma, falcon_start_cpu, sec2_dmem_read, sec2_emem_read,
    sec2_emem_write, sec2_prepare_physical_first,
};
use super::wpr::falcon_id;

/// Attempt to command the (potentially still-running) SEC2 ACR firmware
/// to re-bootstrap FECS via the mailbox command interface.
///
/// After Nouveau boots, SEC2 runs the ACR firmware which enters an idle
/// loop waiting for commands. If SEC2 survived VFIO binding, we can send
/// it a `BOOTSTRAP_FALCON(FECS)` command without resetting anything.
///
/// Nouveau's ACR protocol (gv100_acr.c):
///   1. Host writes ACR_CMD_BOOTSTRAP_FALCON to MAILBOX0 or FALCON_MTHD
///   2. Host writes falcon ID (FECS=1) as parameter
///   3. SEC2 processes command, loads FECS firmware from WPR
///   4. SEC2 releases FECS from HRESET
///   5. SEC2 writes completion status
pub fn attempt_acr_mailbox_command(bar0: &MappedBar) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    // Check if SEC2 appears to be running (from Nouveau's boot)
    let cpuctl = r(falcon::CPUCTL);
    let mb0 = r(falcon::MAILBOX0);
    let mb1 = r(falcon::MAILBOX1);
    notes.push(format!(
        "SEC2 pre-command: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x}"
    ));

    // Try multiple ACR command approaches:

    // Pre-bootstrap: apply GR engine configuration that FECS firmware expects.
    // Nouveau does this in gf100_gr_init() BEFORE gf100_gr_init_ctxctl().
    //
    // GP100+ FECS exceptions: gp100_gr_init_fecs_exceptions
    let _ = bar0.write_u32(0x0040_9c24, 0x000e_0002);
    // SCC init (ctxgf100.c): needed for context switch
    let _ = bar0.write_u32(0x0040_802c, 0x0000_0001);

    const ACR_CMD_BOOTSTRAP_FALCON: u32 = 1;
    const FALCON_ID_FECS: u32 = falcon_id::FECS; // 2
    const FALCON_ID_GPCCS: u32 = falcon_id::GPCCS; // 3

    // Bootstrap both FECS and GPCCS together using a bitmask.
    // Nouveau uses nvkm_acr_bootstrap_falcons(device, FECS|GPCCS)
    // which passes a bitmask: (1<<2)|(1<<3) = 0x0C.
    let falcon_mask = (1u32 << FALCON_ID_FECS) | (1u32 << FALCON_ID_GPCCS);
    notes.push(format!(
        "Sending BOOTSTRAP_FALCON mask={falcon_mask:#06x} (FECS+GPCCS)"
    ));
    w(falcon::MAILBOX1, falcon_mask);
    w(falcon::MAILBOX0, ACR_CMD_BOOTSTRAP_FALCON);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let mb0_after = r(falcon::MAILBOX0);
    let mb1_after = r(falcon::MAILBOX1);
    let fecs_cpu_check = bar0
        .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    let gpccs_cpu_check = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    notes.push(format!(
        "After BOOTSTRAP(mask): mb0={mb0_after:#010x} mb1={mb1_after:#010x} FECS={fecs_cpu_check:#010x} GPCCS={gpccs_cpu_check:#010x}"
    ));

    // If mask approach didn't work (mb0 unchanged), try individual FECS
    if mb0_after == 0 || mb0_after == 0xcafe_beef {
        notes.push("Mask approach failed, trying individual FECS...".to_string());
        w(falcon::MAILBOX1, FALCON_ID_FECS);
        w(falcon::MAILBOX0, ACR_CMD_BOOTSTRAP_FALCON);
        std::thread::sleep(std::time::Duration::from_millis(200));
        let mb0_fecs = r(falcon::MAILBOX0);
        let mb1_fecs = r(falcon::MAILBOX1);
        notes.push(format!(
            "After BOOTSTRAP_FALCON(FECS): mb0={mb0_fecs:#010x} mb1={mb1_fecs:#010x}"
        ));
    }

    // Approach 2: Dump SEC2 DMEM to find active command queue structures.
    // The ACR firmware uses CMDQ/MSGQ in DMEM for host communication.
    // Read first 4KB of DMEM to find all non-zero data.
    {
        let hwcfg = r(falcon::HWCFG);
        let dmem_sz = falcon::dmem_size_bytes(hwcfg);
        let read_sz = (dmem_sz as usize).min(4096);
        let sec2_dmem = sec2_dmem_read(bar0, 0, read_sz);
        let mut ranges = Vec::new();
        let mut in_nonzero = false;
        let mut start = 0;
        for (i, &word) in sec2_dmem.iter().enumerate() {
            if word != 0 && word != 0xDEAD_DEAD {
                if !in_nonzero {
                    start = i;
                    in_nonzero = true;
                }
            } else if in_nonzero {
                ranges.push(format!("[{:#05x}..{:#05x}]", start * 4, i * 4));
                in_nonzero = false;
            }
        }
        if in_nonzero {
            ranges.push(format!(
                "[{:#05x}..{:#05x}]",
                start * 4,
                sec2_dmem.len() * 4
            ));
        }
        notes.push(format!(
            "SEC2 DMEM size={dmem_sz}B, non-zero ranges: {}",
            if ranges.is_empty() {
                "NONE".to_string()
            } else {
                ranges.join(", ")
            }
        ));

        // Dump first 128 bytes in detail for analysis
        let mut detail = Vec::new();
        for (i, &word) in sec2_dmem.iter().take(32).enumerate() {
            if word != 0 {
                detail.push(format!("[{:#05x}]={word:#010x}", i * 4));
            }
        }
        if !detail.is_empty() {
            notes.push(format!("SEC2 DMEM[0..128]: {}", detail.join(" ")));
        }

        // Also dump around common queue descriptor offsets (0x100-0x200)
        let mut queue_detail = Vec::new();
        for (i, &word) in sec2_dmem.iter().skip(64).take(64).enumerate() {
            if word != 0 && word != 0xDEAD_DEAD {
                queue_detail.push(format!("[{:#05x}]={word:#010x}", (i + 64) * 4));
            }
        }
        if !queue_detail.is_empty() {
            notes.push(format!(
                "SEC2 DMEM[0x100..0x200]: {}",
                queue_detail.join(" ")
            ));
        }
    }

    // Reset mailbox before GPCCS command — SEC2 needs to see mb0 transition
    // from 0 to command_id to detect a new command.
    w(falcon::MAILBOX0, 0);
    w(falcon::MAILBOX1, 0);
    std::thread::sleep(std::time::Duration::from_millis(50));

    w(falcon::MAILBOX1, FALCON_ID_GPCCS);
    w(falcon::MAILBOX0, ACR_CMD_BOOTSTRAP_FALCON);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let mb0_gpccs = r(falcon::MAILBOX0);
    let mb1_gpccs = r(falcon::MAILBOX1);
    notes.push(format!(
        "After BOOTSTRAP_FALCON(GPCCS): mb0={mb0_gpccs:#010x} mb1={mb1_gpccs:#010x}"
    ));
    let gpccs_cpuctl_pre = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    notes.push(format!(
        "GPCCS cpuctl after ACR bootstrap attempt: {gpccs_cpuctl_pre:#010x}"
    ));

    // If GPCCS wasn't loaded by ACR (still in HRESET or empty IMEM),
    // directly upload GPCCS firmware via IMEM/DMEM ports.
    if gpccs_cpuctl_pre == 0x10 || gpccs_cpuctl_pre == 0x00 {
        notes.push("GPCCS not bootstrapped by ACR — trying direct IMEM upload...".to_string());
        let boot0 = bar0.read_u32(0x0).unwrap_or(0);
        let sm = ((boot0 >> 20) & 0x1F0) | ((boot0 >> 15) & 0x00F);
        let gpccs_chip = if sm >= 100 {
            "ga102"
        } else if sm >= 80 {
            "ga100"
        } else {
            "gv100"
        };
        match super::super::fecs_boot::boot_gpccs(bar0, gpccs_chip) {
            Ok(result) => {
                notes.push(format!("GPCCS direct boot: {result}"));
            }
            Err(e) => {
                notes.push(format!("GPCCS direct boot failed: {e}"));
            }
        }
    }

    // ── Layer 9: Post-ACR falcon start (Exp 088) ──
    //
    // Nouveau's gf100_gr_init_ctxctl_ext performs these steps AFTER ACR's
    // BOOTSTRAP_FALCON returns. The falcons are loaded but sit in HRESET;
    // the host must explicitly issue STARTCPU.
    //
    // Sequence from nouveau gf100.c:
    //   1. nvkm_mc_unk260(device, 1)           — clock-gating restore
    //   2. 0x409800=0, 0x41a10c=0, 0x40910c=0  — clear mailbox + status
    //   3. nvkm_falcon_start(GPCCS)             — GPCCS first
    //   4. nvkm_falcon_start(FECS)              — FECS second
    //   5. poll 0x409800 bit 0 for 2000ms       — FECS ready
    let fecs_pre_start = bar0
        .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    let gpccs_pre_start = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    notes.push(format!(
        "Pre-start: FECS cpuctl={fecs_pre_start:#010x} GPCCS cpuctl={gpccs_pre_start:#010x}"
    ));

    // Step 1: Clock-gating restore — nouveau's nvkm_mc_unk260(device, 1)
    let _ = bar0.write_u32(0x000260, 1);

    // Step 1b: Configure FECS/GPCCS interrupt enables before starting.
    // Nouveau has these set from its GR init. Without them FECS can't
    // complete its initialization (stuck polling for interrupts).
    // FECS INTR_ENABLE: nouveau value = 0xfc24
    let _ = bar0.write_u32(falcon::FECS_BASE + 0x00c, 0x0000_fc24);
    // GPCCS INTR_ENABLE: matching value
    let _ = bar0.write_u32(falcon::GPCCS_BASE + 0x00c, 0x0000_fc24);
    // FECS ITFEN (interface enable): nouveau = 0x04
    let _ = bar0.write_u32(falcon::FECS_BASE + 0x048, 0x0000_0004);
    // GPCCS ITFEN
    let _ = bar0.write_u32(falcon::GPCCS_BASE + 0x048, 0x0000_0004);

    // Clear status registers before starting falcons
    const FECS_CTXSW_MAILBOX: usize = 0x409800; // FECS_BASE + 0x800
    let _ = bar0.write_u32(FECS_CTXSW_MAILBOX, 0);
    let _ = bar0.write_u32(0x41a10c, 0); // GPCCS status clear
    let _ = bar0.write_u32(0x40910c, 0); // FECS status clear

    const GPCCS_BL_IMEM_OFF: u32 = 0x3400;
    const FECS_BL_IMEM_OFF: u32 = 0x7E00;

    // Exp 091b: Verify GPCCS IMEM contents before STARTCPU.
    // If IMEM is all-zero, ACR's BOOTSTRAP_FALCON DMA failed silently.
    {
        let gpccs_imem_base = falcon::GPCCS_BASE;
        let mut imem_at_0000 = [0u32; 4];
        let mut imem_at_3400 = [0u32; 4];

        // Read IMEM[0x0000..0x0010]
        let _ = bar0.write_u32(gpccs_imem_base + falcon::IMEMC, 0x0200_0000);
        for w in &mut imem_at_0000 {
            *w = bar0.read_u32(gpccs_imem_base + falcon::IMEMD).unwrap_or(0xDEAD_DEAD);
        }
        // Read IMEM[0x3400..0x3410]
        let _ = bar0.write_u32(gpccs_imem_base + falcon::IMEMC, 0x0200_3400);
        for w in &mut imem_at_3400 {
            *w = bar0.read_u32(gpccs_imem_base + falcon::IMEMD).unwrap_or(0xDEAD_DEAD);
        }

        let zero_0 = imem_at_0000.iter().all(|&w| w == 0);
        let zero_bl = imem_at_3400.iter().all(|&w| w == 0);
        notes.push(format!(
            "GPCCS IMEM[0x0000]: {:08x} {:08x} {:08x} {:08x} ({})",
            imem_at_0000[0], imem_at_0000[1], imem_at_0000[2], imem_at_0000[3],
            if zero_0 { "EMPTY" } else { "HAS DATA" }
        ));
        notes.push(format!(
            "GPCCS IMEM[0x3400]: {:08x} {:08x} {:08x} {:08x} ({})",
            imem_at_3400[0], imem_at_3400[1], imem_at_3400[2], imem_at_3400[3],
            if zero_bl { "EMPTY — ACR DMA FAILED" } else { "HAS FIRMWARE" }
        ));

        if zero_bl {
            tracing::warn!("GPCCS IMEM[0x3400] is empty after BOOTSTRAP_FALCON — ACR DMA likely failed");
        }
    }

    let gpccs_bootvec = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::BOOTVEC)
        .unwrap_or(0xDEAD);
    let fecs_bootvec = bar0
        .read_u32(falcon::FECS_BASE + falcon::BOOTVEC)
        .unwrap_or(0xDEAD);
    notes.push(format!(
        "Pre-fix BOOTVEC: GPCCS={gpccs_bootvec:#010x} FECS={fecs_bootvec:#010x}"
    ));

    if gpccs_bootvec == 0 {
        let _ = bar0.write_u32(falcon::GPCCS_BASE + falcon::BOOTVEC, GPCCS_BL_IMEM_OFF);
        notes.push(format!("GPCCS BOOTVEC fixed: 0 → {GPCCS_BL_IMEM_OFF:#06x}"));
    }
    if fecs_bootvec == 0 {
        let _ = bar0.write_u32(falcon::FECS_BASE + falcon::BOOTVEC, FECS_BL_IMEM_OFF);
        notes.push(format!("FECS BOOTVEC fixed: 0 → {FECS_BL_IMEM_OFF:#06x}"));
    }

    // Start GPCCS first (FECS expects GPCCS to be running)
    tracing::info!("Layer 9: issuing STARTCPU to GPCCS");
    falcon_start_cpu(bar0, falcon::GPCCS_BASE);
    std::thread::sleep(std::time::Duration::from_millis(20));
    let gpccs_post_start = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    let gpccs_post_pc = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::PC)
        .unwrap_or(0xDEAD);
    let gpccs_post_exci = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::EXCI)
        .unwrap_or(0xDEAD);
    notes.push(format!(
        "GPCCS after STARTCPU: cpuctl={gpccs_post_start:#010x} pc={gpccs_post_pc:#06x} exci={gpccs_post_exci:#010x}"
    ));

    // Start FECS second
    tracing::info!("Layer 9: issuing STARTCPU to FECS");
    falcon_start_cpu(bar0, falcon::FECS_BASE);

    // Poll FECS_CTXSW_MAILBOX (0x409800) bit 0 for FECS ready
    let poll_timeout = std::time::Duration::from_millis(2000);
    let poll_start = std::time::Instant::now();
    let mut fecs_ready = false;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let status = bar0.read_u32(FECS_CTXSW_MAILBOX).unwrap_or(0);
        let fecs_cpu = bar0
            .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
            .unwrap_or(0xDEAD);

        if status & 1 != 0 {
            notes.push(format!(
                "FECS READY: 0x409800={status:#010x} cpuctl={fecs_cpu:#010x} ({}ms)",
                poll_start.elapsed().as_millis()
            ));
            fecs_ready = true;
            break;
        }
        // Also check if FECS is running (HRESET cleared, not halted)
        if fecs_cpu & (falcon::CPUCTL_HRESET | falcon::CPUCTL_HALTED) == 0 {
            notes.push(format!(
                "FECS RUNNING (no ready signal yet): 0x409800={status:#010x} cpuctl={fecs_cpu:#010x} ({}ms)",
                poll_start.elapsed().as_millis()
            ));
            fecs_ready = true;
            break;
        }
        if poll_start.elapsed() > poll_timeout {
            notes.push(format!(
                "FECS ready timeout (2s): 0x409800={status:#010x} cpuctl={fecs_cpu:#010x}"
            ));
            break;
        }
    }

    // Set watchdog timeout (nouveau: 0x7fffffff)
    if fecs_ready {
        let _ = bar0.write_u32(falcon::FECS_BASE + 0x034, 0x7fff_ffff);
        notes.push("Set FECS watchdog timeout 0x7fffffff".to_string());
    }

    // Check final state with full PC/EXCI verification
    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);
    notes.push(format!(
        "FECS final: cpuctl={:#010x} pc={:#06x} exci={:#010x} mb0={:#010x}",
        post.fecs_cpuctl, post.fecs_pc, post.fecs_exci, post.fecs_mailbox0
    ));
    notes.push(format!(
        "GPCCS final: cpuctl={:#010x} pc={:#06x} exci={:#010x}",
        post.gpccs_cpuctl, post.gpccs_pc, post.gpccs_exci
    ));

    post.into_result(
        "ACR mailbox command (live SEC2) + falcon start",
        sec2_before,
        sec2_after,
        notes,
    )
}

/// Direct FECS boot — bypass SEC2/ACR entirely.
///
/// FECS is in HRESET (cpuctl=0x10) and accepts STARTCPU. Since SEC2 cannot
/// be reset in VFIO, we bypass the ACR chain and load FECS firmware directly:
///   1. Upload fecs_inst.bin (raw code) into FECS IMEM via PIO
///   2. Upload fecs_data.bin (raw data) into FECS DMEM via PIO
///   3. Do the same for GPCCS
///   4. Set BOOTVEC=0, STARTCPU to release FECS from HRESET
///
/// This works if the GR engine doesn't enforce ACR authentication after FLR.
pub fn attempt_direct_fecs_boot(bar0: &MappedBar, fw: &AcrFirmwareSet) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);

    let fecs_base = falcon::FECS_BASE;
    let gpccs_base = falcon::GPCCS_BASE;
    let fr = |base: usize, off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let fw_ = |base: usize, off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    // Check FECS state — must be in HRESET for STARTCPU to work
    let fecs_cpuctl = fr(fecs_base, falcon::CPUCTL);
    let fecs_hwcfg = fr(fecs_base, falcon::HWCFG);
    let gpccs_cpuctl = fr(gpccs_base, falcon::CPUCTL);
    let gpccs_hwcfg = fr(gpccs_base, falcon::HWCFG);
    notes.push(format!(
        "FECS: cpuctl={fecs_cpuctl:#010x} hwcfg={fecs_hwcfg:#010x} \
         imem={}B dmem={}B",
        falcon::imem_size_bytes(fecs_hwcfg),
        falcon::dmem_size_bytes(fecs_hwcfg)
    ));
    notes.push(format!(
        "GPCCS: cpuctl={gpccs_cpuctl:#010x} hwcfg={gpccs_hwcfg:#010x}"
    ));

    if fecs_cpuctl & falcon::CPUCTL_HRESET == 0 {
        notes.push("FECS is NOT in HRESET — cannot use STARTCPU".to_string());
        return make_fail_result("Direct FECS boot: not in HRESET", sec2_before, bar0, notes);
    }

    // Upload FECS firmware
    // fecs_inst.bin is raw code (no bin_hdr), load at IMEM offset 0
    notes.push(format!(
        "Uploading fecs_inst ({} bytes) to FECS IMEM@0",
        fw.fecs_inst.len()
    ));
    falcon_imem_upload_nouveau(bar0, fecs_base, 0, &fw.fecs_inst, 0);

    // Verify first 16 bytes of IMEM upload
    fw_(fecs_base, falcon::IMEMC, 0x0200_0000); // read mode, addr=0
    let mut readback = [0u32; 4];
    for word in &mut readback {
        *word = fr(fecs_base, falcon::IMEMD);
    }
    let expected = &fw.fecs_inst[..16.min(fw.fecs_inst.len())];
    let readback_bytes: Vec<u8> = readback.iter().flat_map(|w| w.to_le_bytes()).collect();
    let imem_match = readback_bytes[..expected.len()] == *expected;
    notes.push(format!(
        "FECS IMEM verify: match={imem_match} read={:02x?}",
        &readback_bytes[..expected.len()]
    ));

    // fecs_data.bin is raw data, load at DMEM offset 0
    notes.push(format!(
        "Uploading fecs_data ({} bytes) to FECS DMEM@0",
        fw.fecs_data.len()
    ));
    falcon_dmem_upload(bar0, fecs_base, 0, &fw.fecs_data);

    // Also do GPCCS if it's in HRESET
    if gpccs_cpuctl & falcon::CPUCTL_HRESET != 0 {
        notes.push(format!(
            "Uploading gpccs_inst ({} bytes) to GPCCS IMEM@0",
            fw.gpccs_inst.len()
        ));
        falcon_imem_upload_nouveau(bar0, gpccs_base, 0, &fw.gpccs_inst, 0);
        notes.push(format!(
            "Uploading gpccs_data ({} bytes) to GPCCS DMEM@0",
            fw.gpccs_data.len()
        ));
        falcon_dmem_upload(bar0, gpccs_base, 0, &fw.gpccs_data);
    }

    // Boot GPCCS first (FECS expects GPCCS to be running)
    if gpccs_cpuctl & falcon::CPUCTL_HRESET != 0 {
        fw_(gpccs_base, falcon::MAILBOX0, 0);
        fw_(gpccs_base, falcon::MAILBOX1, 0);
        fw_(gpccs_base, falcon::BOOTVEC, 0);
        fw_(gpccs_base, falcon::CPUCTL, falcon::CPUCTL_STARTCPU);
        std::thread::sleep(std::time::Duration::from_millis(50));
        let gpccs_after_cpuctl = fr(gpccs_base, falcon::CPUCTL);
        let gpccs_after_mb0 = fr(gpccs_base, falcon::MAILBOX0);
        notes.push(format!(
            "GPCCS after STARTCPU: cpuctl={gpccs_after_cpuctl:#010x} mb0={gpccs_after_mb0:#010x}"
        ));
    }

    // Boot FECS
    fw_(fecs_base, falcon::MAILBOX0, 0);
    fw_(fecs_base, falcon::MAILBOX1, 0);
    fw_(fecs_base, falcon::BOOTVEC, 0);
    notes.push("FECS: BOOTVEC=0, issuing STARTCPU".to_string());
    fw_(fecs_base, falcon::CPUCTL, falcon::CPUCTL_STARTCPU);

    // Poll for FECS to respond
    let timeout = std::time::Duration::from_secs(3);
    let start = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let cpuctl = fr(fecs_base, falcon::CPUCTL);
        let mb0 = fr(fecs_base, falcon::MAILBOX0);
        let mb1 = fr(fecs_base, falcon::MAILBOX1);

        let hreset = cpuctl & falcon::CPUCTL_HRESET != 0;
        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;

        if mb0 != 0 || halted || !hreset {
            notes.push(format!(
                "FECS responded: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} ({}ms)",
                start.elapsed().as_millis()
            ));
            break;
        }
        if start.elapsed() > timeout {
            notes.push(format!(
                "FECS timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x}"
            ));
            break;
        }
    }

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);

    post.into_result(
        "Direct FECS boot (bypass ACR)",
        sec2_before,
        sec2_after,
        notes,
    )
}

/// Attempt 081a: Direct HRESET release experiments.
///
/// Tries several low-cost approaches before committing to the full ACR chain:
/// 1. Direct write to FECS CPUCTL to clear HRESET bit
/// 2. PMC GR engine reset toggle
/// 3. SEC2 EMEM probe (verify accessibility)
pub fn attempt_direct_hreset(bar0: &MappedBar) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    notes.push(format!("SEC2 initial state: {:?}", sec2_before.state));

    let fecs_r = |off: usize| bar0.read_u32(falcon::FECS_BASE + off).unwrap_or(0xDEAD);

    // Experiment 1: Try direct CPUCTL write to clear HRESET
    let fecs_cpuctl_before = fecs_r(falcon::CPUCTL);
    notes.push(format!("FECS cpuctl before: {fecs_cpuctl_before:#010x}"));

    if fecs_cpuctl_before & falcon::CPUCTL_HRESET != 0 {
        // Try writing 0 to CPUCTL (clear all bits including HRESET)
        let _ = bar0.write_u32(falcon::FECS_BASE + falcon::CPUCTL, 0);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let after = fecs_r(falcon::CPUCTL);
        notes.push(format!("FECS cpuctl after direct clear: {after:#010x}"));

        if after & falcon::CPUCTL_HRESET == 0 {
            notes.push("Direct HRESET clear SUCCEEDED".to_string());
        } else {
            notes.push("Direct HRESET clear failed (expected — ACR-managed)".to_string());
        }
    }

    // Experiment 2: PMC GR engine reset toggle (bit 12)
    let pmc_enable: usize = 0x200;
    let pmc = bar0.read_u32(pmc_enable).unwrap_or(0);
    let gr_bit: u32 = 1 << 12;
    notes.push(format!("PMC before GR toggle: {pmc:#010x}"));

    let _ = bar0.write_u32(pmc_enable, pmc & !gr_bit);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _ = bar0.write_u32(pmc_enable, pmc | gr_bit);
    std::thread::sleep(std::time::Duration::from_millis(10));

    let fecs_after_pmc = fecs_r(falcon::CPUCTL);
    notes.push(format!(
        "FECS cpuctl after PMC GR toggle: {fecs_after_pmc:#010x}"
    ));

    // Experiment 3: SEC2 EMEM accessibility test
    let test_pattern: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];
    sec2_emem_write(bar0, 0, &test_pattern);
    let readback = sec2_emem_read(bar0, 0, 4);
    let expected_word: u32 = 0xEFBE_ADDE;
    let emem_ok = readback.first().copied() == Some(expected_word);
    notes.push(format!(
        "SEC2 EMEM write/read: wrote={:#010x} read={:#010x} match={}",
        expected_word,
        readback.first().copied().unwrap_or(0),
        emem_ok
    ));

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);

    post.into_result(
        "081a: direct HRESET experiments",
        sec2_before,
        sec2_after,
        notes,
    )
}

/// Attempt EMEM-based SEC2 boot with signed ACR bootloader.
///
/// SEC2 has an internal ROM that runs after any reset. The ROM checks EMEM
/// for a signed bootloader. We try two approaches:
///
/// A) Write FULL bl.bin to EMEM, then engine reset → ROM finds it during init
/// B) Engine reset first, then write FULL bl.bin → ROM might be polling EMEM
///
/// The full file (with nvfw_bin_hdr + signature) is loaded, not just the payload,
/// since the ROM needs the signature for verification.
pub fn attempt_emem_boot(bar0: &MappedBar, fw: &AcrFirmwareSet) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);

    notes.push(format!("SEC2 state: {:?}", sec2_before.state));

    // We'll try both the full file and just the payload
    let bl_full = &fw.acr_bl_raw;
    let bl_payload = fw.acr_bl_parsed.payload(&fw.acr_bl_raw);
    notes.push(format!(
        "ACR BL: total={}B payload={}B data_off={:#x}",
        bl_full.len(),
        bl_payload.len(),
        fw.acr_bl_parsed.bin_hdr.data_offset
    ));

    // First: engine reset and dump DMEM to see what ROM initializes
    tracing::info!("EMEM: Resetting SEC2 and dumping post-ROM DMEM");
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("Pre-dump reset failed: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    let tracepc_rom = r(0x030);
    notes.push(format!("ROM idle PC: {tracepc_rom:#010x}"));

    // Dump DMEM after ROM init (first 256 bytes)
    let post_rom_dmem = sec2_dmem_read(bar0, 0, 256);
    let mut rom_data = Vec::new();
    for (i, &w) in post_rom_dmem.iter().enumerate() {
        if w != 0 && w != 0xDEAD_DEAD {
            rom_data.push(format!("[{:#05x}]={w:#010x}", i * 4));
        }
    }
    notes.push(format!(
        "DMEM after ROM: {}",
        if rom_data.is_empty() {
            "all zeros".to_string()
        } else {
            rom_data.join(" ")
        }
    ));

    // Also dump EMEM after ROM (ROM might have cleared it)
    let post_rom_emem = sec2_emem_read(bar0, 0, 64);
    let emem_nonzero: Vec<String> = post_rom_emem
        .iter()
        .enumerate()
        .filter(|&(_, w)| *w != 0 && *w != 0xDEAD_DEAD)
        .map(|(i, w)| format!("[{:#05x}]={w:#010x}", i * 4))
        .collect();
    notes.push(format!(
        "EMEM after ROM: {}",
        if emem_nonzero.is_empty() {
            "all zeros".to_string()
        } else {
            emem_nonzero.join(" ")
        }
    ));

    // ── Approach A: Write full bl.bin to EMEM offset 0, then reset ──
    tracing::info!("EMEM approach A: full bl.bin@0 → reset");
    sec2_emem_write(bar0, 0, bl_full);
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("A: reset failed: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    let cpuctl_a = r(falcon::CPUCTL);
    let mb0_a = r(falcon::MAILBOX0);
    let tracepc_a = r(0x030);
    notes.push(format!(
        "A (full@0): cpuctl={cpuctl_a:#010x} mb0={mb0_a:#010x} pc={tracepc_a:#010x}"
    ));

    // ── Approach B: Write payload to EMEM offset 0, then reset ──
    tracing::info!("EMEM approach B: payload@0 → reset");
    sec2_emem_write(bar0, 0, bl_payload);
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("B: reset failed: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    let cpuctl_b = r(falcon::CPUCTL);
    let tracepc_b = r(0x030);
    notes.push(format!(
        "B (payload@0): cpuctl={cpuctl_b:#010x} pc={tracepc_b:#010x}"
    ));

    // ── Approach C: Write full bl.bin to EMEM offset 0x200 (data_offset) ──
    tracing::info!("EMEM approach C: full bl.bin@0x200 → reset");
    // Clear EMEM first
    let zeros = vec![0u8; bl_full.len() + 0x200];
    sec2_emem_write(bar0, 0, &zeros);
    sec2_emem_write(bar0, 0x200, bl_full);
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("C: reset failed: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    let cpuctl_c = r(falcon::CPUCTL);
    let tracepc_c = r(0x030);
    notes.push(format!(
        "C (full@0x200): cpuctl={cpuctl_c:#010x} pc={tracepc_c:#010x}"
    ));

    // ── Approach D: Write BL desc to DMEM + payload to IMEM via PIO ──
    // After engine reset, ROM runs and enters idle. Then we halt? No...
    // Actually, let's try BOOTVEC=0xAC4 (ROM idle PC) + write to MAILBOX
    // to signal the ROM. Some ROMs check MAILBOX for commands.
    tracing::info!("Approach D: signal ROM via MAILBOX");
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("D: reset failed: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
    // Write "boot command" to MAILBOX0 (ROM might check this)
    let _ = bar0.write_u32(base + falcon::MAILBOX0, 0x1);
    std::thread::sleep(std::time::Duration::from_millis(200));
    let mb0_d = r(falcon::MAILBOX0);
    let tracepc_d = r(0x030);
    notes.push(format!(
        "D (mailbox signal): mb0={mb0_d:#010x} pc={tracepc_d:#010x} (changed={})",
        mb0_d != 0x1
    ));

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);
    notes.push(format!(
        "FECS: cpuctl={:#010x} pc={:#06x} exci={:#010x} mb0={:#010x}",
        post.fecs_cpuctl, post.fecs_pc, post.fecs_exci, post.fecs_mailbox0
    ));

    post.into_result(
        "EMEM-based SEC2 boot",
        sec2_before,
        sec2_after,
        notes,
    )
}

/// Attempt nouveau-style SEC2 boot: falcon reset + IMEM code + EMEM descriptor.
///
/// Matches nouveau's `gm200_flcn_fw_load()` + `gm200_flcn_fw_boot()`:
/// 1. Reset SEC2 falcon (engine reset via 0x3C0)
/// 2. Load BL CODE into IMEM at (code_limit - boot_size) with tag = start_tag
/// 3. Load BL DATA descriptor into EMEM at offset 0
/// 4. Set BOOTVEC = start_tag << 8 = 0xFD00
/// 5. Write mailbox0 = 0xcafebeef
/// 6. CPUCTL = 0x02 (STARTCPU)
/// 7. Poll for halt (CPUCTL & HRESET set)
pub fn attempt_nouveau_boot(bar0: &MappedBar, fw: &AcrFirmwareSet) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    notes.push(format!("SEC2 state: {:?}", sec2_before.state));

    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    // Step 1: Falcon engine reset (nouveau: gp102_flcn_reset_eng).
    tracing::info!("Resetting SEC2 falcon via engine reset (0x3C0)");
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("Engine reset failed: {e}"));
    } else {
        let cpuctl_after_reset = r(falcon::CPUCTL);
        let sctl_after_reset = r(falcon::SCTL);
        notes.push(format!(
            "After engine reset: cpuctl={cpuctl_after_reset:#010x} sctl={sctl_after_reset:#010x}"
        ));
    }

    // Parse BL descriptor from the sub-header at bin_hdr.header_offset.
    let bl_hdr = &fw.acr_bl_parsed;
    let sub_hdr = &bl_hdr.raw;
    let bl_start_tag = if sub_hdr.len() >= 4 {
        u32::from_le_bytes([sub_hdr[0], sub_hdr[1], sub_hdr[2], sub_hdr[3]])
    } else {
        0xFD
    };
    let bl_code_off = if sub_hdr.len() >= 12 {
        u32::from_le_bytes([sub_hdr[8], sub_hdr[9], sub_hdr[10], sub_hdr[11]])
    } else {
        0
    };
    let bl_code_size = if sub_hdr.len() >= 16 {
        u32::from_le_bytes([sub_hdr[12], sub_hdr[13], sub_hdr[14], sub_hdr[15]])
    } else {
        0x200
    };
    let bl_data_off = if sub_hdr.len() >= 20 {
        u32::from_le_bytes([sub_hdr[16], sub_hdr[17], sub_hdr[18], sub_hdr[19]])
    } else {
        0x200
    };
    let bl_data_size = if sub_hdr.len() >= 24 {
        u32::from_le_bytes([sub_hdr[20], sub_hdr[21], sub_hdr[22], sub_hdr[23]])
    } else {
        0x100
    };

    let boot_addr = bl_start_tag << 8;
    notes.push(format!(
        "BL desc: start_tag={bl_start_tag:#x} boot_addr={boot_addr:#x} \
         code=[{bl_code_off:#x}+{bl_code_size:#x}] data=[{bl_data_off:#x}+{bl_data_size:#x}]"
    ));

    // Extract code and data from the payload.
    let payload = bl_hdr.payload(&fw.acr_bl_raw);
    let code_end = (bl_code_off + bl_code_size) as usize;
    let data_end = (bl_data_off + bl_data_size) as usize;

    let bl_code = if code_end <= payload.len() {
        &payload[bl_code_off as usize..code_end]
    } else {
        payload
    };
    let bl_data = if data_end <= payload.len() {
        &payload[bl_data_off as usize..data_end]
    } else {
        &[]
    };

    // Step 2: Load BL code into IMEM at (code_limit - boot_size).
    // SEC2 HWCFG gives code_limit; on GV100 SEC2 it's 64KB = 0x10000.
    let hwcfg = r(falcon::HWCFG);
    let code_limit = falcon::imem_size_bytes(hwcfg);
    let imem_addr = code_limit.saturating_sub(bl_code.len() as u32);
    let imem_tag = boot_addr >> 8;

    notes.push(format!(
        "Loading BL code: {} bytes to IMEM@{imem_addr:#x} tag={imem_tag:#x} (code_limit={code_limit:#x})",
        bl_code.len()
    ));

    // Use the tagged IMEM upload matching nouveau's gm200_flcn_pio_imem_wr.
    let imemc_val = (1u32 << 24) | imem_addr;
    w(falcon::IMEMC, imemc_val);

    // Write tag for the first 256-byte page.
    w(falcon::IMEMT, imem_tag);
    for (i, chunk) in bl_code.chunks(4).enumerate() {
        let byte_off = (i * 4) as u32;
        // Set tag for each new 256-byte page boundary.
        if byte_off > 0 && byte_off & 0xFF == 0 {
            w(falcon::IMEMT, imem_tag + (byte_off >> 8));
        }
        let word = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
            2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
            1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
            _ => 0,
        };
        w(falcon::IMEMD, word);
    }

    // Step 3: Load BL data (descriptor) into EMEM at offset 0.
    // This tells the BL where to find the main ACR firmware via DMA.
    // For now, we load the raw BL data section — DMA addresses will be 0,
    // so the BL will try to DMA and fail, but we should see execution.
    if !bl_data.is_empty() {
        notes.push(format!(
            "Loading BL data: {} bytes to EMEM@0",
            bl_data.len()
        ));
        sec2_emem_write(bar0, 0, bl_data);
    }

    // Step 3b: Set up physical DMA mode (Nouveau: gm200_flcn_fw_load non-instance path).
    falcon_prepare_physical_dma(bar0, base);

    // Step 4-6: Boot sequence (nouveau: gm200_flcn_fw_boot).
    w(falcon::MAILBOX0, 0xcafe_beef_u32);
    w(falcon::BOOTVEC, boot_addr);
    let cpuctl_pre = r(falcon::CPUCTL);
    let alias_en = cpuctl_pre & (1 << 6) != 0;
    notes.push(format!(
        "BOOTVEC={boot_addr:#x} mailbox0=0xcafebeef cpuctl={cpuctl_pre:#010x} alias_en={alias_en}, issuing STARTCPU"
    ));
    falcon_start_cpu(bar0, base);

    // Step 7: Poll for halt (nouveau waits for CPUCTL & 0x10).
    let timeout = std::time::Duration::from_secs(2);
    let start = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let mb1 = r(falcon::MAILBOX1);

        // Nouveau waits for HRESET bit to be set (falcon halted).
        if cpuctl & falcon::CPUCTL_HRESET != 0 && cpuctl != sec2_before.cpuctl {
            notes.push(format!(
                "SEC2 halted: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} ({}ms)",
                start.elapsed().as_millis()
            ));
            break;
        }
        if mb0 != 0xcafe_beef && mb0 != 0 {
            notes.push(format!(
                "SEC2 mailbox changed: cpuctl={cpuctl:#010x} mb0={mb0:#010x} ({}ms)",
                start.elapsed().as_millis()
            ));
            break;
        }
        if start.elapsed() > timeout {
            notes.push(format!(
                "SEC2 timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x}"
            ));
            break;
        }
    }

    // Read TRACEPC for debugging (write indices 0-4 to EXCI, read TRACEPC).
    let exci = r(falcon::EXCI);
    let tidx_count = (exci >> 16) & 0xFF;
    let mut tracepc = Vec::new();
    for sp in 0..tidx_count.min(8) {
        w(falcon::EXCI, sp);
        tracepc.push(r(falcon::TRACEPC));
    }
    notes.push(format!(
        "EXCI={exci:#010x} TRACEPC({tidx_count}): {:?}",
        tracepc
            .iter()
            .map(|v| format!("{v:#010x}"))
            .collect::<Vec<_>>()
    ));

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);

    post.into_result(
        "nouveau-style IMEM+EMEM SEC2 boot",
        sec2_before,
        sec2_after,
        notes,
    )
}

/// Exp 091b: Direct host-driven GPCCS/FECS firmware upload, bypassing SEC2/ACR DMA.
///
/// SEC2's inst block bind fails (bind_stat stuck at 3), so ACR cannot DMA
/// firmware into GPCCS IMEM. Instead, we upload firmware directly via
/// IMEMC/IMEMD host ports while the falcons are in HRESET.
pub fn attempt_direct_falcon_upload(bar0: &MappedBar, fw: &AcrFirmwareSet) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);

    // PMC GR reset to get clean HRESET on FECS/GPCCS
    let pmc_enable: usize = 0x200;
    let pmc_val = bar0.read_u32(pmc_enable).unwrap_or(0);
    let gr_bit = 1u32 << 12;
    notes.push(format!("PMC pre-reset: {pmc_val:#010x}"));

    // Disable GR
    let _ = bar0.write_u32(pmc_enable, pmc_val & !gr_bit);
    let _ = bar0.read_u32(pmc_enable); // barrier
    std::thread::sleep(std::time::Duration::from_micros(20));
    // Re-enable GR
    let _ = bar0.write_u32(pmc_enable, pmc_val | gr_bit);
    let _ = bar0.read_u32(pmc_enable); // barrier
    std::thread::sleep(std::time::Duration::from_millis(5));

    let gpccs_cpuctl = bar0.read_u32(falcon::GPCCS_BASE + falcon::CPUCTL).unwrap_or(0xDEAD);
    let fecs_cpuctl = bar0.read_u32(falcon::FECS_BASE + falcon::CPUCTL).unwrap_or(0xDEAD);
    notes.push(format!(
        "Post-PMC-reset: GPCCS cpuctl={gpccs_cpuctl:#010x} FECS cpuctl={fecs_cpuctl:#010x}"
    ));

    // Upload GPCCS firmware: inst → IMEM[0], BL → IMEM[bl_imem_off], data → DMEM[0]
    let gpccs_bl_off = fw.gpccs_bl.bl_imem_off();
    notes.push(format!(
        "GPCCS firmware: inst={}B bl={}B(tag={:#x}, off={gpccs_bl_off:#x}) data={}B",
        fw.gpccs_inst.len(), fw.gpccs_bl.code.len(), fw.gpccs_bl.start_tag, fw.gpccs_data.len()
    ));

    // IMEM upload: inst code first (tag starts at 0)
    falcon_imem_upload_nouveau(bar0, falcon::GPCCS_BASE, 0, &fw.gpccs_inst, 0);
    // IMEM upload: bootloader at bl_imem_off (tag = start_tag)
    falcon_imem_upload_nouveau(
        bar0,
        falcon::GPCCS_BASE,
        gpccs_bl_off,
        &fw.gpccs_bl.code,
        fw.gpccs_bl.start_tag,
    );
    // DMEM upload: data section
    falcon_dmem_upload(bar0, falcon::GPCCS_BASE, 0, &fw.gpccs_data);

    // Verify IMEM was written
    let _ = bar0.write_u32(falcon::GPCCS_BASE + falcon::IMEMC, 0x0200_0000);
    let imem0 = bar0.read_u32(falcon::GPCCS_BASE + falcon::IMEMD).unwrap_or(0);
    let _ = bar0.write_u32(falcon::GPCCS_BASE + falcon::IMEMC, 0x0200_0000 | gpccs_bl_off);
    let imem_bl = bar0.read_u32(falcon::GPCCS_BASE + falcon::IMEMD).unwrap_or(0);
    notes.push(format!(
        "GPCCS IMEM verify: [0x0000]={imem0:#010x} [{gpccs_bl_off:#06x}]={imem_bl:#010x}"
    ));

    // Configure GPCCS: BOOTVEC, ITFEN, INTR_ENABLE
    let _ = bar0.write_u32(falcon::GPCCS_BASE + falcon::BOOTVEC, gpccs_bl_off);
    let _ = bar0.write_u32(falcon::GPCCS_BASE + 0x048, 0x04); // ITFEN
    let _ = bar0.write_u32(falcon::GPCCS_BASE + 0x00c, 0xfc24); // INTR_ENABLE
    let bv_rb = bar0.read_u32(falcon::GPCCS_BASE + falcon::BOOTVEC).unwrap_or(0xDEAD);
    notes.push(format!("GPCCS BOOTVEC={bv_rb:#010x} ITFEN=0x04 INTR_EN=0xfc24"));

    // Start GPCCS
    tracing::info!("Exp 091b: STARTCPU on GPCCS with host-loaded firmware");
    falcon_start_cpu(bar0, falcon::GPCCS_BASE);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let gpccs_pc = bar0.read_u32(falcon::GPCCS_BASE + falcon::PC).unwrap_or(0xDEAD);
    let gpccs_exci = bar0.read_u32(falcon::GPCCS_BASE + falcon::EXCI).unwrap_or(0xDEAD);
    let gpccs_cpuctl2 = bar0.read_u32(falcon::GPCCS_BASE + falcon::CPUCTL).unwrap_or(0xDEAD);
    let gpccs_mb0 = bar0.read_u32(falcon::GPCCS_BASE + falcon::MAILBOX0).unwrap_or(0xDEAD);
    notes.push(format!(
        "GPCCS after start: cpuctl={gpccs_cpuctl2:#010x} pc={gpccs_pc:#06x} exci={gpccs_exci:#010x} mb0={gpccs_mb0:#010x}"
    ));

    let gpccs_ok = gpccs_exci == 0 && gpccs_pc != 0;
    if gpccs_ok {
        tracing::info!("GPCCS ALIVE: pc={gpccs_pc:#06x}");
    } else {
        tracing::warn!("GPCCS FAULT: pc={gpccs_pc:#06x} exci={gpccs_exci:#010x}");

        // PC sampling to detect any progression
        let mut pcs = Vec::new();
        for _ in 0..5 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            pcs.push(bar0.read_u32(falcon::GPCCS_BASE + falcon::PC).unwrap_or(0xDEAD));
        }
        notes.push(format!("GPCCS PC samples: {pcs:08x?}"));
    }

    // Upload FECS firmware
    let fecs_bl_off = fw.fecs_bl.bl_imem_off();
    notes.push(format!(
        "FECS firmware: inst={}B bl={}B(tag={:#x}, off={fecs_bl_off:#x}) data={}B",
        fw.fecs_inst.len(), fw.fecs_bl.code.len(), fw.fecs_bl.start_tag, fw.fecs_data.len()
    ));

    falcon_imem_upload_nouveau(bar0, falcon::FECS_BASE, 0, &fw.fecs_inst, 0);
    falcon_imem_upload_nouveau(
        bar0,
        falcon::FECS_BASE,
        fecs_bl_off,
        &fw.fecs_bl.code,
        fw.fecs_bl.start_tag,
    );
    falcon_dmem_upload(bar0, falcon::FECS_BASE, 0, &fw.fecs_data);

    let _ = bar0.write_u32(falcon::FECS_BASE + falcon::BOOTVEC, fecs_bl_off);
    let _ = bar0.write_u32(falcon::FECS_BASE + 0x048, 0x04); // ITFEN
    let _ = bar0.write_u32(falcon::FECS_BASE + 0x00c, 0xfc24); // INTR_ENABLE

    tracing::info!("Exp 091b: STARTCPU on FECS with host-loaded firmware");
    falcon_start_cpu(bar0, falcon::FECS_BASE);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let fecs_pc = bar0.read_u32(falcon::FECS_BASE + falcon::PC).unwrap_or(0xDEAD);
    let fecs_exci = bar0.read_u32(falcon::FECS_BASE + falcon::EXCI).unwrap_or(0xDEAD);
    let fecs_cpuctl2 = bar0.read_u32(falcon::FECS_BASE + falcon::CPUCTL).unwrap_or(0xDEAD);
    let fecs_mb0 = bar0.read_u32(falcon::FECS_BASE + falcon::MAILBOX0).unwrap_or(0xDEAD);
    notes.push(format!(
        "FECS after start: cpuctl={fecs_cpuctl2:#010x} pc={fecs_pc:#06x} exci={fecs_exci:#010x} mb0={fecs_mb0:#010x}"
    ));

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);

    post.into_result(
        "Exp 091b: direct host IMEM/DMEM upload (bypass ACR DMA)",
        sec2_before,
        sec2_after,
        notes,
    )
}

/// Physical-first SEC2 boot: PMC reset → physical DMA (no instance block) → BL upload → start.
///
/// This strategy eliminates the circular dependency that blocks the instance block bind:
/// - `sec2_prepare_direct_boot` tries to bind an instance block for virtual DMA, but
///   the bind walker needs FBIF in physical mode to read the page tables it's trying to
///   bind. The walker stalls at state 2.
/// - Nouveau boots with physical DMA for the initial bootloader, and only the BL
///   itself sets up virtual addressing internally after it's running.
///
/// After SBR (via Ember), this sequence gives SEC2 a fully clean hardware state:
/// 1. `sec2_prepare_physical_first` — PMC reset + physical DMA mode (no bind)
/// 2. Upload BL code to IMEM (PIO)
/// 3. Upload BL descriptor to EMEM/DMEM with **physical** VRAM addresses for WPR
/// 4. BOOTVEC + STARTCPU
/// 5. BL runs with physical DMA, loads ACR firmware from physical VRAM
pub fn attempt_physical_first_boot(bar0: &MappedBar, fw: &AcrFirmwareSet) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    notes.push(format!("SEC2 state: {:?}", sec2_before.state));

    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    // Step 1: Full reset + physical DMA setup (no instance block)
    tracing::info!("Physical-first: resetting SEC2 and configuring physical DMA");
    let (halted, prep_notes) = sec2_prepare_physical_first(bar0);
    notes.extend(prep_notes);
    if !halted {
        notes.push("WARNING: SEC2 did not halt after reset — continuing anyway".to_string());
    }

    // Step 2: Parse BL descriptor
    let bl_hdr = &fw.acr_bl_parsed;
    let sub_hdr = &bl_hdr.raw;
    let bl_start_tag = if sub_hdr.len() >= 4 {
        u32::from_le_bytes([sub_hdr[0], sub_hdr[1], sub_hdr[2], sub_hdr[3]])
    } else {
        0xFD
    };
    let bl_code_off = if sub_hdr.len() >= 12 {
        u32::from_le_bytes([sub_hdr[8], sub_hdr[9], sub_hdr[10], sub_hdr[11]])
    } else {
        0
    };
    let bl_code_size = if sub_hdr.len() >= 16 {
        u32::from_le_bytes([sub_hdr[12], sub_hdr[13], sub_hdr[14], sub_hdr[15]])
    } else {
        0x200
    };
    let bl_data_off = if sub_hdr.len() >= 20 {
        u32::from_le_bytes([sub_hdr[16], sub_hdr[17], sub_hdr[18], sub_hdr[19]])
    } else {
        0x200
    };
    let bl_data_size = if sub_hdr.len() >= 24 {
        u32::from_le_bytes([sub_hdr[20], sub_hdr[21], sub_hdr[22], sub_hdr[23]])
    } else {
        0x100
    };

    let boot_addr = bl_start_tag << 8;
    notes.push(format!(
        "BL: start_tag={bl_start_tag:#x} boot_addr={boot_addr:#x} \
         code=[{bl_code_off:#x}+{bl_code_size:#x}] data=[{bl_data_off:#x}+{bl_data_size:#x}]"
    ));

    // Step 3: Upload BL code to IMEM
    let payload = bl_hdr.payload(&fw.acr_bl_raw);
    let code_end = (bl_code_off + bl_code_size) as usize;
    let data_end = (bl_data_off + bl_data_size) as usize;
    let bl_code = if code_end <= payload.len() {
        &payload[bl_code_off as usize..code_end]
    } else {
        payload
    };
    let bl_data = if data_end <= payload.len() {
        &payload[bl_data_off as usize..data_end]
    } else {
        &[]
    };

    let hwcfg = r(falcon::HWCFG);
    let code_limit = falcon::imem_size_bytes(hwcfg);
    let imem_addr = code_limit.saturating_sub(bl_code.len() as u32);
    let imem_tag = boot_addr >> 8;

    notes.push(format!(
        "Uploading BL: {} bytes IMEM@{imem_addr:#x} tag={imem_tag:#x}",
        bl_code.len()
    ));
    falcon_imem_upload_nouveau(bar0, base, imem_addr, bl_code, imem_tag);

    // Step 4: Upload BL data to EMEM (descriptor with physical addresses)
    if !bl_data.is_empty() {
        notes.push(format!("Uploading BL data: {} bytes EMEM@0", bl_data.len()));
        sec2_emem_write(bar0, 0, bl_data);
    }

    // Step 5: BOOTVEC + STARTCPU
    w(falcon::MAILBOX0, 0xcafe_beef_u32);
    w(falcon::BOOTVEC, boot_addr);
    notes.push(format!(
        "BOOTVEC={boot_addr:#x} mailbox0=0xcafebeef — PHYSICAL DMA mode"
    ));
    falcon_start_cpu(bar0, base);

    // Step 6: Poll for halt/completion
    let timeout = std::time::Duration::from_secs(3);
    let start = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let mb1 = r(falcon::MAILBOX1);

        if cpuctl & falcon::CPUCTL_HRESET != 0 && cpuctl != sec2_before.cpuctl {
            notes.push(format!(
                "SEC2 halted: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} ({}ms)",
                start.elapsed().as_millis()
            ));
            break;
        }
        if mb0 != 0xcafe_beef && mb0 != 0 {
            notes.push(format!(
                "SEC2 mailbox changed: cpuctl={cpuctl:#010x} mb0={mb0:#010x} ({}ms)",
                start.elapsed().as_millis()
            ));
            break;
        }
        if start.elapsed() > timeout {
            notes.push(format!(
                "SEC2 timeout (3s): cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x}"
            ));
            break;
        }
    }

    // Diagnostics
    let exci = r(falcon::EXCI);
    let pc = bar0.read_u32(base + falcon::PC).unwrap_or(0xDEAD);
    notes.push(format!(
        "Post-boot: pc={pc:#06x} exci={exci:#010x}"
    ));

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);

    post.into_result(
        "Physical-first SEC2 boot (no instance block)",
        sec2_before,
        sec2_after,
        notes,
    )
}
