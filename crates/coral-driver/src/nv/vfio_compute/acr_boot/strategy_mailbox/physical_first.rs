// SPDX-License-Identifier: AGPL-3.0-only

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

use super::super::boot_result::AcrBootResult;
use super::super::firmware::AcrFirmwareSet;
use super::super::sec2_hal::{
    Sec2Probe, falcon_imem_upload_nouveau, falcon_start_cpu, sec2_emem_write,
    sec2_prepare_physical_first,
};

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
    notes.push(format!("Post-boot: pc={pc:#06x} exci={exci:#010x}"));

    // ── SEC2 Conversation probe ──
    super::super::sec2_queue::probe_and_bootstrap(bar0, &mut notes);

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::super::boot_result::PostBootCapture::capture(bar0);

    post.into_result(
        "Physical-first SEC2 boot (no instance block)",
        sec2_before,
        sec2_after,
        notes,
    )
}
