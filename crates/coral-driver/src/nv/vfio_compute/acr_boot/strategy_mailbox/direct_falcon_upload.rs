// SPDX-License-Identifier: AGPL-3.0-only

use crate::vfio::channel::registers::{falcon, misc};
use crate::vfio::device::MappedBar;

use super::super::boot_result::AcrBootResult;
use super::super::firmware::AcrFirmwareSet;
use super::super::sec2_hal::{
    Sec2Probe, falcon_dmem_upload, falcon_imem_upload_nouveau, falcon_start_cpu,
};

/// Exp 091b: Direct host-driven GPCCS/FECS firmware upload, bypassing SEC2/ACR DMA.
///
/// SEC2's inst block bind fails (bind_stat stuck at 3), so ACR cannot DMA
/// firmware into GPCCS IMEM. Instead, we upload firmware directly via
/// IMEMC/IMEMD host ports while the falcons are in HRESET.
pub fn attempt_direct_falcon_upload(bar0: &MappedBar, fw: &AcrFirmwareSet) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);

    let pmc_val = bar0.read_u32(misc::PMC_ENABLE).unwrap_or(0);
    let gr_bit = 1u32 << 12;
    notes.push(format!("PMC pre-reset: {pmc_val:#010x}"));

    let _ = bar0.write_u32(misc::PMC_ENABLE, pmc_val & !gr_bit);
    let _ = bar0.read_u32(misc::PMC_ENABLE);
    std::thread::sleep(std::time::Duration::from_micros(20));
    let _ = bar0.write_u32(misc::PMC_ENABLE, pmc_val | gr_bit);
    let _ = bar0.read_u32(misc::PMC_ENABLE);

    std::thread::sleep(std::time::Duration::from_millis(5));

    let gpccs_cpuctl = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    let fecs_cpuctl = bar0
        .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    notes.push(format!(
        "Post-PMC-reset: GPCCS cpuctl={gpccs_cpuctl:#010x} FECS cpuctl={fecs_cpuctl:#010x}"
    ));

    // Upload GPCCS firmware: inst → IMEM[0], BL → IMEM[bl_imem_off], data → DMEM[0]
    let gpccs_bl_off = fw.gpccs_bl.bl_imem_off();
    notes.push(format!(
        "GPCCS firmware: inst={}B bl={}B(tag={:#x}, off={gpccs_bl_off:#x}) data={}B",
        fw.gpccs_inst.len(),
        fw.gpccs_bl.code.len(),
        fw.gpccs_bl.start_tag,
        fw.gpccs_data.len()
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
    let imem0 = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::IMEMD)
        .unwrap_or(0);
    let _ = bar0.write_u32(
        falcon::GPCCS_BASE + falcon::IMEMC,
        0x0200_0000 | gpccs_bl_off,
    );
    let imem_bl = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::IMEMD)
        .unwrap_or(0);
    notes.push(format!(
        "GPCCS IMEM verify: [0x0000]={imem0:#010x} [{gpccs_bl_off:#06x}]={imem_bl:#010x}"
    ));

    // Configure GPCCS: BOOTVEC, ITFEN, INTR_ENABLE
    let _ = bar0.write_u32(falcon::GPCCS_BASE + falcon::BOOTVEC, gpccs_bl_off);
    let _ = bar0.write_u32(falcon::GPCCS_BASE + falcon::ITFEN, 0x04);
    let _ = bar0.write_u32(falcon::GPCCS_BASE + falcon::IRQMODE, 0xfc24);
    let bv_rb = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::BOOTVEC)
        .unwrap_or(0xDEAD);
    notes.push(format!(
        "GPCCS BOOTVEC={bv_rb:#010x} ITFEN=0x04 INTR_EN=0xfc24"
    ));

    // Start GPCCS
    tracing::info!("Exp 091b: STARTCPU on GPCCS with host-loaded firmware");
    falcon_start_cpu(bar0, falcon::GPCCS_BASE);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let gpccs_pc = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::PC)
        .unwrap_or(0xDEAD);
    let gpccs_exci = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::EXCI)
        .unwrap_or(0xDEAD);
    let gpccs_cpuctl2 = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    let gpccs_mb0 = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::MAILBOX0)
        .unwrap_or(0xDEAD);
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
            pcs.push(
                bar0.read_u32(falcon::GPCCS_BASE + falcon::PC)
                    .unwrap_or(0xDEAD),
            );
        }
        notes.push(format!("GPCCS PC samples: {pcs:08x?}"));
    }

    // Upload FECS firmware
    let fecs_bl_off = fw.fecs_bl.bl_imem_off();
    notes.push(format!(
        "FECS firmware: inst={}B bl={}B(tag={:#x}, off={fecs_bl_off:#x}) data={}B",
        fw.fecs_inst.len(),
        fw.fecs_bl.code.len(),
        fw.fecs_bl.start_tag,
        fw.fecs_data.len()
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
    let _ = bar0.write_u32(falcon::FECS_BASE + falcon::ITFEN, 0x04);
    let _ = bar0.write_u32(falcon::FECS_BASE + falcon::IRQMODE, 0xfc24);

    tracing::info!("Exp 091b: STARTCPU on FECS with host-loaded firmware");
    falcon_start_cpu(bar0, falcon::FECS_BASE);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let fecs_pc = bar0
        .read_u32(falcon::FECS_BASE + falcon::PC)
        .unwrap_or(0xDEAD);
    let fecs_exci = bar0
        .read_u32(falcon::FECS_BASE + falcon::EXCI)
        .unwrap_or(0xDEAD);
    let fecs_cpuctl2 = bar0
        .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    let fecs_mb0 = bar0
        .read_u32(falcon::FECS_BASE + falcon::MAILBOX0)
        .unwrap_or(0xDEAD);
    notes.push(format!(
        "FECS after start: cpuctl={fecs_cpuctl2:#010x} pc={fecs_pc:#06x} exci={fecs_exci:#010x} mb0={fecs_mb0:#010x}"
    ));

    // ── SEC2 Conversation probe ──
    super::super::sec2_queue::probe_and_bootstrap(bar0, &mut notes);

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::super::boot_result::PostBootCapture::capture(bar0);

    post.into_result(
        "Exp 091b: direct host IMEM/DMEM upload (bypass ACR DMA)",
        sec2_before,
        sec2_after,
        notes,
    )
}
