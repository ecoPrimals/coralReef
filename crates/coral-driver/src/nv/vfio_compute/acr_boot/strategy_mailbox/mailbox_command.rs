// SPDX-License-Identifier: AGPL-3.0-only

use crate::vfio::channel::registers::{falcon, misc};
use crate::vfio::device::MappedBar;

use super::super::boot_result::{AcrBootResult, dmem_detail, dmem_nonzero_summary};
use super::super::sec2_hal::{Sec2Probe, falcon_start_cpu, sec2_dmem_read};
use super::super::wpr::falcon_id;
use super::bootvec::FalconBootvecOffsets;

/// Attempt to command the (potentially still-running) SEC2 ACR firmware
/// to re-bootstrap FECS via the mailbox command interface.
///
/// The `bootvec` parameter supplies firmware-derived IMEM offsets for the
/// BOOTVEC fix (Exp 091 discovery): if ACR's DMA loaded firmware but left
/// BOOTVEC at 0, the falcon would start at IMEM\[0\] instead of the BL
/// entry, causing an immediate exception. This function patches BOOTVEC
/// from the supplied offsets before issuing STARTCPU.
pub fn attempt_acr_mailbox_command(
    bar0: &MappedBar,
    bootvec: &FalconBootvecOffsets,
) -> AcrBootResult {
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
    let _ = bar0.write_u32(falcon::FECS_BASE + falcon::EXCEPTION_REG, 0x000e_0002);
    // SCC init (ctxgf100.c): needed for context switch
    let _ = bar0.write_u32(falcon::FECS_BASE + falcon::GR_CLASS_CFG, 0x0000_0001);

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
        notes.push(format!(
            "SEC2 DMEM size={dmem_sz}B, non-zero ranges: {}",
            dmem_nonzero_summary(&sec2_dmem)
        ));

        let detail = dmem_detail(&sec2_dmem, 0, 32);
        if !detail.is_empty() {
            notes.push(format!("SEC2 DMEM[0..128]: {}", detail.join(" ")));
        }
        let queue_detail = dmem_detail(&sec2_dmem, 64, 64);
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
        let sm = crate::nv::identity::boot0_to_sm(boot0).unwrap_or(70);
        let gpccs_chip = crate::nv::identity::chip_name(sm);
        match crate::nv::vfio_compute::fecs_boot::boot_gpccs(bar0, gpccs_chip) {
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
    //   1. nvkm_mc_unk260(device, 1)               — clock-gating restore
    //   2. clear MTHD_STATUS + DMACTL               — clear mailbox + status
    //   3. nvkm_falcon_start(GPCCS)                 — GPCCS first
    //   4. nvkm_falcon_start(FECS)                  — FECS second
    //   5. poll FECS MTHD_STATUS bit 0 for 2000ms   — FECS ready
    let fecs_pre_start = bar0
        .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    let gpccs_pre_start = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    notes.push(format!(
        "Pre-start: FECS cpuctl={fecs_pre_start:#010x} GPCCS cpuctl={gpccs_pre_start:#010x}"
    ));

    let _ = bar0.write_u32(misc::PMC_UNK260, 1);

    // Step 1b: Configure FECS/GPCCS interrupt enables before starting.
    // Nouveau has these set from its GR init. Without them FECS can't
    // complete its initialization (stuck polling for interrupts).
    let _ = bar0.write_u32(falcon::FECS_BASE + falcon::IRQMODE, 0x0000_fc24);
    let _ = bar0.write_u32(falcon::GPCCS_BASE + falcon::IRQMODE, 0x0000_fc24);
    // FECS ITFEN (interface enable): nouveau = 0x04
    let _ = bar0.write_u32(falcon::FECS_BASE + falcon::ITFEN, 0x0000_0004);
    // GPCCS ITFEN
    let _ = bar0.write_u32(falcon::GPCCS_BASE + falcon::ITFEN, 0x0000_0004);

    let _ = bar0.write_u32(falcon::FECS_BASE + falcon::MTHD_STATUS, 0);
    let _ = bar0.write_u32(falcon::GPCCS_BASE + falcon::DMACTL, 0);
    let _ = bar0.write_u32(falcon::FECS_BASE + falcon::DMACTL, 0);

    let gpccs_bl_imem_off = bootvec.gpccs;
    let fecs_bl_imem_off = bootvec.fecs;

    // Exp 091b: Verify GPCCS IMEM contents before STARTCPU.
    // If IMEM is all-zero, ACR's BOOTSTRAP_FALCON DMA failed silently.
    {
        let gpccs_imem_base = falcon::GPCCS_BASE;
        let mut imem_at_0000 = [0u32; 4];
        let mut imem_at_3400 = [0u32; 4];

        // Read IMEM[0x0000..0x0010]
        let _ = bar0.write_u32(gpccs_imem_base + falcon::IMEMC, 0x0200_0000);
        for w in &mut imem_at_0000 {
            *w = bar0
                .read_u32(gpccs_imem_base + falcon::IMEMD)
                .unwrap_or(0xDEAD_DEAD);
        }
        // Read IMEM[0x3400..0x3410]
        let _ = bar0.write_u32(gpccs_imem_base + falcon::IMEMC, 0x0200_3400);
        for w in &mut imem_at_3400 {
            *w = bar0
                .read_u32(gpccs_imem_base + falcon::IMEMD)
                .unwrap_or(0xDEAD_DEAD);
        }

        let zero_0 = imem_at_0000.iter().all(|&w| w == 0);
        let zero_bl = imem_at_3400.iter().all(|&w| w == 0);
        notes.push(format!(
            "GPCCS IMEM[0x0000]: {:08x} {:08x} {:08x} {:08x} ({})",
            imem_at_0000[0],
            imem_at_0000[1],
            imem_at_0000[2],
            imem_at_0000[3],
            if zero_0 { "EMPTY" } else { "HAS DATA" }
        ));
        notes.push(format!(
            "GPCCS IMEM[0x3400]: {:08x} {:08x} {:08x} {:08x} ({})",
            imem_at_3400[0],
            imem_at_3400[1],
            imem_at_3400[2],
            imem_at_3400[3],
            if zero_bl {
                "EMPTY — ACR DMA FAILED"
            } else {
                "HAS FIRMWARE"
            }
        ));

        if zero_bl {
            tracing::warn!(
                "GPCCS IMEM[0x3400] is empty after BOOTSTRAP_FALCON — ACR DMA likely failed"
            );
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
        let _ = bar0.write_u32(falcon::GPCCS_BASE + falcon::BOOTVEC, gpccs_bl_imem_off);
        notes.push(format!("GPCCS BOOTVEC fixed: 0 → {gpccs_bl_imem_off:#06x}"));
    }
    if fecs_bootvec == 0 {
        let _ = bar0.write_u32(falcon::FECS_BASE + falcon::BOOTVEC, fecs_bl_imem_off);
        notes.push(format!("FECS BOOTVEC fixed: 0 → {fecs_bl_imem_off:#06x}"));
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

    let fecs_mthd_status = falcon::FECS_BASE + falcon::MTHD_STATUS;
    let poll_timeout = std::time::Duration::from_millis(2000);
    let poll_start = std::time::Instant::now();
    let mut fecs_ready = false;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let status = bar0.read_u32(fecs_mthd_status).unwrap_or(0);
        let fecs_cpu = bar0
            .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
            .unwrap_or(0xDEAD);

        if status & 1 != 0 {
            notes.push(format!(
                "FECS READY: MTHD_STATUS={status:#010x} cpuctl={fecs_cpu:#010x} ({}ms)",
                poll_start.elapsed().as_millis()
            ));
            fecs_ready = true;
            break;
        }
        // Also check if FECS is running (HALTED cleared, not stopped)
        if fecs_cpu & (falcon::CPUCTL_HALTED | falcon::CPUCTL_STOPPED) == 0 {
            notes.push(format!(
                "FECS RUNNING (no ready signal yet): MTHD_STATUS={status:#010x} cpuctl={fecs_cpu:#010x} ({}ms)",
                poll_start.elapsed().as_millis()
            ));
            fecs_ready = true;
            break;
        }
        if poll_start.elapsed() > poll_timeout {
            notes.push(format!(
                "FECS ready timeout (2s): MTHD_STATUS={status:#010x} cpuctl={fecs_cpu:#010x}"
            ));
            break;
        }
    }

    // Set watchdog timeout (nouveau: 0x7fffffff)
    if fecs_ready {
        let _ = bar0.write_u32(falcon::FECS_BASE + falcon::WATCHDOG, 0x7fff_ffff);
        notes.push("Set FECS watchdog timeout 0x7fffffff".to_string());
    }

    // ── SEC2 Conversation probe ──
    super::super::sec2_queue::probe_and_bootstrap(bar0, &mut notes);

    // Check final state with full PC/EXCI verification
    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::super::boot_result::PostBootCapture::capture(bar0);
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
