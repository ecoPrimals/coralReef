// SPDX-License-Identifier: AGPL-3.0-or-later

//! NVDEC scrubber — runs the NVDEC memory scrubber to configure WPR2.
//!
//! On GV100, WPR (Write-Protected Region) must be configured before SEC2 can
//! transition to Heavy-Secure mode. WPR registers at 0x100CE4/CE8/CEC/CF0 are
//! NOT writable from BAR0 — they require hardware-privileged access.
//!
//! The NVDEC scrubber firmware (`nvdec/scrubber.bin`) runs on the NVDEC falcon
//! and has the hardware privilege to configure WPR boundaries. This is the
//! standard NVIDIA boot sequence:
//!   1. Host loads scrubber onto NVDEC falcon
//!   2. Scrubber scrubs VRAM and configures WPR2
//!   3. SEC2 BL loads ACR from WPR → transitions to HS mode
//!   4. ACR bootstraps FECS/GPCCS

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

/// Boot the NVDEC scrubber to configure WPR.
///
/// Returns (success, diagnostic notes).
pub fn boot_nvdec_scrubber(
    bar0: &MappedBar,
    scrubber_fw: &[u8],
    wpr_base: u32,
    wpr_end: u32,
) -> (bool, Vec<String>) {
    let mut notes = Vec::new();
    let base = falcon::NVDEC_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    // Pre-state
    let cpuctl_pre = r(falcon::CPUCTL);
    let sctl_pre = r(falcon::SCTL);
    let hwcfg = r(falcon::HWCFG);
    notes.push(format!(
        "NVDEC pre: cpuctl={cpuctl_pre:#x} sctl={sctl_pre:#x} hwcfg={hwcfg:#x}"
    ));

    // Parse scrubber firmware (nvidia bin header v1)
    if scrubber_fw.len() < 0x200 {
        notes.push("scrubber.bin too small".to_string());
        return (false, notes);
    }
    let magic = u32::from_le_bytes(scrubber_fw[0..4].try_into().unwrap());
    if magic != 0x10DE {
        notes.push(format!("bad magic: {magic:#x} (expected 0x10DE)"));
        return (false, notes);
    }
    let total_sz = u32::from_le_bytes(scrubber_fw[8..12].try_into().unwrap()) as usize;
    let data_off = u32::from_le_bytes(scrubber_fw[16..20].try_into().unwrap()) as usize;
    let data_sz = u32::from_le_bytes(scrubber_fw[20..24].try_into().unwrap()) as usize;
    notes.push(format!(
        "scrubber: total={total_sz:#x} data_off={data_off:#x} data_sz={data_sz:#x}"
    ));

    if data_off + data_sz > scrubber_fw.len() {
        notes.push("scrubber data exceeds file".to_string());
        return (false, notes);
    }

    // Step 1: PMC reset NVDEC
    // NVDEC PMC bit is typically bit 15 (0x8000) in PMC_ENABLE
    let pmc_val = bar0.read_u32(0x200).unwrap_or(0);
    let nvdec_bit: u32 = 1 << 15;
    if pmc_val & nvdec_bit != 0 {
        let _ = bar0.write_u32(0x200, pmc_val & !nvdec_bit);
        let _ = bar0.read_u32(0x200);
        std::thread::sleep(std::time::Duration::from_micros(20));
    }
    let _ = bar0.write_u32(0x200, pmc_val | nvdec_bit);
    let _ = bar0.read_u32(0x200);
    std::thread::sleep(std::time::Duration::from_micros(50));
    notes.push(format!("NVDEC PMC reset: pmc={pmc_val:#x}"));

    // Wait for NVDEC ROM halt
    let halt_start = std::time::Instant::now();
    let mut halted = false;
    loop {
        let cpuctl = r(falcon::CPUCTL);
        if cpuctl & falcon::CPUCTL_HRESET != 0 {
            halted = true;
            break;
        }
        if halt_start.elapsed() > std::time::Duration::from_millis(200) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }
    let cpuctl_post_reset = r(falcon::CPUCTL);
    notes.push(format!(
        "NVDEC halt: halted={halted} cpuctl={cpuctl_post_reset:#x} ({:?})",
        halt_start.elapsed()
    ));

    // Step 2: Load code into NVDEC IMEM via PIO
    let code = &scrubber_fw[data_off..data_off + data_sz];
    let code_words = code.len() / 4;

    // IMEMC: auto-increment + write mode, starting at address 0
    let imemc_val = (1u32 << 24) | (1u32 << 25);
    w(falcon::IMEMC, imemc_val);
    for i in 0..code_words {
        let word = u32::from_le_bytes(code[i * 4..(i + 1) * 4].try_into().unwrap());
        w(falcon::IMEMD, word);
    }
    notes.push(format!("NVDEC IMEM: {code_words} words loaded"));

    // Verify first few words
    let imemc_read = (1u32 << 24); // auto-inc, read mode
    w(falcon::IMEMC, imemc_read);
    let v0 = r(falcon::IMEMD);
    let v1 = r(falcon::IMEMD);
    let expected0 = u32::from_le_bytes(code[0..4].try_into().unwrap());
    notes.push(format!(
        "IMEM verify: [{v0:#010x},{v1:#010x}] expected={expected0:#010x} match={}",
        v0 == expected0
    ));

    // Step 3: Load DMEM with WPR configuration
    // The scrubber expects WPR bounds in DMEM. Format varies by firmware version.
    // For GV100 scrubber: DMEM[0x00..0x10] contains configuration struct:
    //   DMEM[0x00] = wpr_base (in 256-byte units)
    //   DMEM[0x04] = wpr_end (in 256-byte units)
    //   DMEM[0x08] = flags (0x01 = scrub enabled)
    let dmemc_val = (1u32 << 24) | (1u32 << 25); // auto-inc + write
    w(falcon::DMEMC, dmemc_val);
    w(falcon::DMEMD, wpr_base);
    w(falcon::DMEMD, wpr_end);
    w(falcon::DMEMD, 0x01); // scrub enable flag
    w(falcon::DMEMD, 0x00); // padding
    notes.push(format!(
        "DMEM: wpr_base={wpr_base:#x} wpr_end={wpr_end:#x} flags=0x01"
    ));

    // Step 4: Configure DMA (PHYS_OVERRIDE for VRAM access)
    w(falcon::DMACTL, 0);
    let fbif_cur = r(falcon::FBIF_TRANSCFG);
    w(falcon::FBIF_TRANSCFG, fbif_cur | falcon::FBIF_PHYSICAL_OVERRIDE);

    // Step 5: Set BOOTVEC and start
    w(falcon::BOOTVEC, 0);
    w(falcon::MAILBOX0, 0);
    w(falcon::MAILBOX1, 0);

    let cpuctl_before = r(falcon::CPUCTL);
    w(falcon::CPUCTL, falcon::CPUCTL_STARTCPU);
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Step 6: Poll for completion
    let poll_start = std::time::Instant::now();
    let mut completed = false;
    let mut final_mb0 = 0u32;
    loop {
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        if cpuctl & falcon::CPUCTL_HALTED != 0 || mb0 != 0 {
            final_mb0 = mb0;
            completed = true;
            let cpuctl_f = r(falcon::CPUCTL);
            let pc = r(falcon::PC);
            notes.push(format!(
                "NVDEC done: mb0={mb0:#x} cpuctl={cpuctl_f:#x} pc={pc:#x} ({:?})",
                poll_start.elapsed()
            ));
            break;
        }
        if poll_start.elapsed() > std::time::Duration::from_secs(2) {
            let pc = r(falcon::PC);
            let exci = r(falcon::EXCI);
            notes.push(format!(
                "NVDEC timeout: cpuctl={cpuctl:#x} mb0={mb0:#x} pc={pc:#x} exci={exci:#x}"
            ));
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    // Step 7: Check WPR registers
    let wpr1_beg = bar0.read_u32(0x100CE4).unwrap_or(0xDEAD);
    let wpr1_end = bar0.read_u32(0x100CE8).unwrap_or(0xDEAD);
    let wpr2_beg = bar0.read_u32(0x100CEC).unwrap_or(0xDEAD);
    let wpr2_end = bar0.read_u32(0x100CF0).unwrap_or(0xDEAD);
    notes.push(format!(
        "Post-scrubber WPR: WPR1=[{wpr1_beg:#x}..{wpr1_end:#x}] WPR2=[{wpr2_beg:#x}..{wpr2_end:#x}]"
    ));

    let wpr_changed = wpr2_beg != 0 || wpr2_end != 0;
    notes.push(format!(
        "WPR configured: {} (mb0={final_mb0:#x} completed={completed})",
        wpr_changed
    ));

    (completed && wpr_changed, notes)
}
