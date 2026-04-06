// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

use super::super::boot_result::AcrBootResult;
use super::super::firmware::AcrFirmwareSet;
use super::super::sec2_hal::{
    Sec2Probe, falcon_engine_reset, falcon_prepare_physical_dma, falcon_start_cpu, sec2_emem_write,
};

/// Attempt nouveau-style SEC2 boot: falcon reset + IMEM code + EMEM descriptor.
///
/// Matches nouveau's `gm200_flcn_fw_load()` + `gm200_flcn_fw_boot()`:
/// 1. Reset SEC2 falcon (engine reset via ENGCTL)
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
    tracing::info!("Resetting SEC2 falcon via engine reset");
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

    // ── SEC2 Conversation probe ──
    super::super::sec2_queue::probe_and_bootstrap(bar0, &mut notes);

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::super::boot_result::PostBootCapture::capture(bar0);

    post.into_result(
        "nouveau-style IMEM+EMEM SEC2 boot",
        sec2_before,
        sec2_after,
        notes,
    )
}
