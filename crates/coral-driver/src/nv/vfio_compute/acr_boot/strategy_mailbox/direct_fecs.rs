// SPDX-License-Identifier: AGPL-3.0-only

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

use super::super::boot_result::{AcrBootResult, make_fail_result};
use super::super::firmware::AcrFirmwareSet;
use super::super::sec2_hal::{Sec2Probe, falcon_dmem_upload, falcon_imem_upload_nouveau};

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

    // ── SEC2 Conversation probe ──
    super::super::sec2_queue::probe_and_bootstrap(bar0, &mut notes);

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::super::boot_result::PostBootCapture::capture(bar0);

    post.into_result(
        "Direct FECS boot (bypass ACR)",
        sec2_before,
        sec2_after,
        notes,
    )
}
