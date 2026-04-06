// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

use super::super::boot_result::AcrBootResult;
use super::super::firmware::AcrFirmwareSet;
use super::super::sec2_hal::{
    Sec2Probe, falcon_engine_reset, sec2_dmem_read, sec2_emem_read, sec2_emem_write,
};

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
    let tracepc_rom = r(falcon::PC);
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
    let tracepc_a = r(falcon::PC);
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
    let tracepc_b = r(falcon::PC);
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
    let tracepc_c = r(falcon::PC);
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
    let tracepc_d = r(falcon::PC);
    notes.push(format!(
        "D (mailbox signal): mb0={mb0_d:#010x} pc={tracepc_d:#010x} (changed={})",
        mb0_d != 0x1
    ));

    // ── SEC2 Conversation probe ──
    super::super::sec2_queue::probe_and_bootstrap(bar0, &mut notes);

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::super::boot_result::PostBootCapture::capture(bar0);
    notes.push(format!(
        "FECS: cpuctl={:#010x} pc={:#06x} exci={:#010x} mb0={:#010x}",
        post.fecs_cpuctl, post.fecs_pc, post.fecs_exci, post.fecs_mailbox0
    ));

    post.into_result("EMEM-based SEC2 boot", sec2_before, sec2_after, notes)
}
