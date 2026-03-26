// SPDX-License-Identifier: AGPL-3.0-only

//! Legacy ACR chain and direct IMEM load strategies.

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::{DmaBackend, MappedBar};
use crate::vfio::dma::DmaBuffer;

use super::boot_result::{AcrBootResult, make_fail_result};
use super::firmware::{AcrFirmwareSet, ParsedAcrFirmware};
use super::instance_block;
use super::sec2_hal::{
    Sec2Probe, falcon_dmem_upload, falcon_engine_reset, falcon_imem_upload_nouveau,
    falcon_prepare_physical_dma, falcon_start_cpu,
};
use super::wpr::{ACR_IOVA_BASE, build_bl_dmem_desc};

/// 4. Engine-reset SEC2 falcon
/// 5. Configure physical DMA mode (register 0x624 + DMACTL)
/// 6. Load BL code → IMEM (per-page IMEMC init, matching Nouveau)
/// 7. Build flcn_bl_dmem_desc_v1 → DMEM (with DMA addresses)
/// 8. BOOTVEC + STARTCPU → poll for HRESET + mailbox check
pub fn attempt_acr_chain(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    container: DmaBackend,
) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    notes.push(format!("SEC2 state: {:?}", sec2_before.state));

    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    // ── Step 1: Parse firmware ──
    let parsed = match ParsedAcrFirmware::parse(fw) {
        Ok(p) => p,
        Err(e) => {
            notes.push(format!("Firmware parse failed: {e}"));
            return make_fail_result("ACR chain: parse failed", sec2_before, bar0, notes);
        }
    };
    notes.push(format!("{}", parsed.bl_desc));
    notes.push(format!(
        "ACR ucode: {}B, non_sec=[{:#x}+{:#x}] sec=[{:#x}+{:#x}] data=[{:#x}+{:#x}]",
        parsed.acr_payload.len(),
        parsed.load_header.non_sec_code_off,
        parsed.load_header.non_sec_code_size,
        parsed.load_header.apps.first().map(|a| a.0).unwrap_or(0),
        parsed.load_header.apps.first().map(|a| a.1).unwrap_or(0),
        parsed.load_header.data_dma_base,
        parsed.load_header.data_size,
    ));

    // ── Step 2: Allocate DMA for ACR firmware payload ──
    let acr_payload_size = parsed.acr_payload.len();
    let acr_iova = ACR_IOVA_BASE;
    let mut acr_dma = match DmaBuffer::new(container.clone(), acr_payload_size, acr_iova) {
        Ok(buf) => buf,
        Err(e) => {
            notes.push(format!("DMA alloc failed for ACR payload: {e}"));
            return make_fail_result("ACR chain: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    notes.push(format!(
        "ACR payload DMA: iova={acr_iova:#x} size={acr_payload_size:#x}"
    ));

    // Copy ACR payload into DMA buffer (with optional WPR patching)
    let payload_copy = parsed.acr_payload.clone();

    // ACR descriptor WPR fields are zero at this point — WPR patching is done
    // in the VRAM/sysmem strategies. The chain strategy tests BL DMA-load behavior
    // without WPR to isolate DMA fault root causes.
    let data_off = parsed.load_header.data_dma_base as usize;
    if data_off + 0x24 <= payload_copy.len() {
        notes.push(format!(
            "ACR desc at data_off={data_off:#x} (WPR fields zero — chain strategy test)"
        ));
    }

    acr_dma.as_mut_slice()[..payload_copy.len()].copy_from_slice(&payload_copy);

    let code_dma_base = acr_iova;
    let data_dma_base = acr_iova + data_off as u64;
    notes.push(format!(
        "DMA addrs: code={code_dma_base:#x} data={data_dma_base:#x}"
    ));

    // ── Step 3: Engine-reset SEC2 ──
    tracing::info!("Engine-resetting SEC2 for ACR chain boot");
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("Engine reset failed: {e}"));
    } else {
        let cpuctl_post = r(falcon::CPUCTL);
        notes.push(format!("Post-reset cpuctl={cpuctl_post:#010x}"));
    }

    // ── Step 4: Configure SEC2 DMA ──
    // Try all known methods to enable falcon DMA for system memory access.
    // Nouveau uses two different approaches depending on falcon generation:
    //
    // A) Instance block binding (gp102_sec2_flcn_bind_inst):
    //    Register 0x054 (falcon bind_inst) — see instance_block.rs
    //    Value: (1<<30) | (target<<28) | (addr>>12), target=2 for SYS_MEM_COH
    //    Then DMACTL=0x02 (enable IMEM DMA through instance block)
    //
    // B) Physical DMA mode (gm200_flcn_fw_boot, no instance block):
    //    Register 0x624 |= 0x80 (enable physical addressing)
    //    DMACTL=0 (no instance block translation)
    //
    // We try both: bind instance block first, then set physical DMA as fallback.
    use crate::vfio::channel::registers::PD3_IOVA;

    // Full nouveau-style bind sequence (Exp 084: missing trigger writes caused bind_stat=0)
    let inst_val = instance_block::encode_bind_inst(PD3_IOVA, 2);
    let (bind_ok, bind_notes) =
        instance_block::falcon_bind_context(&|off| r(off), &|off, val| w(off, val), inst_val);
    for n in &bind_notes {
        notes.push(n.clone());
    }
    w(falcon::DMACTL, 0x02);
    notes.push(format!(
        "DMA config: bind_ok={bind_ok} DMACTL={:#010x}",
        r(falcon::DMACTL)
    ));

    // Method B: Also enable physical DMA as fallback (does not conflict with inst block)
    w(0x624, r(0x624) | 0x80);
    notes.push(format!("Physical DMA fallback: 0x624={:#010x}", r(0x624)));

    // ── Step 5: Load BL code → IMEM ──
    let hwcfg = r(falcon::HWCFG);
    let code_limit = falcon::imem_size_bytes(hwcfg);
    let boot_size =
        ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;

    notes.push(format!(
        "IMEM: code_limit={code_limit:#x} boot_size={boot_size:#x} addr={imem_addr:#x} tag={start_tag:#x} boot_addr={boot_addr:#x}"
    ));

    falcon_imem_upload_nouveau(bar0, base, imem_addr, &parsed.bl_code, start_tag);

    // ── Step 6: Build BL descriptor → DMEM ──
    let bl_desc_bytes = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    notes.push(format!(
        "BL DMEM desc: {} bytes → DMEM@0 (ctx_dma=UCODE via instance block)",
        bl_desc_bytes.len()
    ));
    falcon_dmem_upload(bar0, base, 0, &bl_desc_bytes);

    // ── Step 7: Boot SEC2 ──
    w(falcon::MAILBOX0, 0);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, boot_addr);
    notes.push(format!("BOOTVEC={boot_addr:#x}, issuing STARTCPU"));
    w(falcon::CPUCTL, falcon::CPUCTL_STARTCPU);

    // ── Step 8: Poll for completion ──
    // Nouveau waits for CPUCTL bit 4 (HRESET) to be re-asserted = falcon halted.
    let timeout = std::time::Duration::from_secs(3);
    let start = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let mb1 = r(falcon::MAILBOX1);

        // Success: falcon halted (HRESET re-asserted) and mailbox indicates completion
        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;
        let hreset_back = cpuctl & falcon::CPUCTL_HRESET != 0;

        if hreset_back && cpuctl != sec2_before.cpuctl {
            notes.push(format!(
                "SEC2 halted: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} ({}ms)",
                start.elapsed().as_millis()
            ));
            break;
        }
        if mb0 != 0 {
            notes.push(format!(
                "SEC2 mailbox: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} ({}ms)",
                start.elapsed().as_millis()
            ));
            break;
        }
        if halted {
            notes.push(format!(
                "SEC2 halted (no mailbox): cpuctl={cpuctl:#010x} ({}ms)",
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

    // Diagnostics: EXCI + TRACEPC
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
    super::sec2_queue::probe_and_bootstrap(bar0, &mut notes);

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);

    drop(acr_dma);

    post.into_result(
        "ACR chain: DMA-backed SEC2 boot",
        sec2_before,
        sec2_after,
        notes,
    )
}

/// Direct ACR firmware load — bypasses the bootloader's DMA transfer.
///
/// Instead of: BL → (DMA) → ACR code → (DMA) → FECS
/// We do:      Host PIO → ACR code in IMEM/DMEM → Start SEC2
///
/// This eliminates the DMA dependency for loading ACR into SEC2, though
/// the ACR firmware itself will still need DMA to load FECS from a WPR.
/// Useful as a diagnostic to determine if the DMA is the sole blocker.
pub fn attempt_direct_acr_load(bar0: &MappedBar, fw: &AcrFirmwareSet) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    // ── Canary test: load tiny program that writes 0xBEEF to MAILBOX0 ──
    // Try multiple falcon ISA encodings since the correct one depends on
    // the falcon version (v5/v6 on GV100 SEC2).
    //
    // Encoding A: fuc5 16-bit immediate MOV (b0/b1 prefix)
    //   mov b16 $r0 0xbeef; mov b16 $r1 0x0040; iowr I[$r1] $r0; exit
    const CANARY_V5_16: &[u8] = &[
        0xb0, 0xef, 0xbe, // mov b16 $r0 0xbeef
        0xb1, 0x40, 0x00, // mov b16 $r1 0x0040
        0xf6, 0x10, 0x00, // iowr I[$r1] $r0
        0xf8, 0x02, // exit
    ];
    // Encoding B: fuc5 32-bit immediate MOV (f0/f1 prefix)
    const CANARY_V5_32: &[u8] = &[
        0xf0, 0xef, 0xbe, 0x00, 0x00, // mov b32 $r0 0x0000beef
        0xf1, 0x40, 0x00, 0x00, 0x00, // mov b32 $r1 0x00000040
        0xf6, 0x10, 0x00, // iowr I[$r1] $r0
        0xf8, 0x02, // exit
    ];
    // Encoding C: original (may be fuc0/fuc3 format)
    const CANARY_ORIG: &[u8] = &[
        0x80, 0xef, 0xbe, 0x00, 0x01, 0x40, 0xf6, 0x10, 0x00, 0xf8, 0x02,
    ];

    let canaries: &[(&str, &[u8])] = &[
        ("v5_16bit", CANARY_V5_16),
        ("v5_32bit", CANARY_V5_32),
        ("original", CANARY_ORIG),
    ];

    // Try each canary encoding with engine reset + IMEM upload + STARTCPU.
    // Also try CPUCTL_ALIAS (0x130) for starting.
    for (name, code) in canaries {
        if let Err(e) = falcon_engine_reset(bar0, base) {
            notes.push(format!("CANARY {name}: reset failed: {e}"));
            continue;
        }
        let tracepc_pre = r(0x030);
        w(falcon::CPUCTL, falcon::CPUCTL_IINVAL);
        std::thread::sleep(std::time::Duration::from_millis(1));
        falcon_imem_upload_nouveau(bar0, base, 0, code, 0);
        w(falcon::MAILBOX0, 0);
        w(falcon::MAILBOX1, 0);
        w(falcon::BOOTVEC, 0);
        let cpuctl_pre = r(falcon::CPUCTL);

        // Try both CPUCTL and CPUCTL_ALIAS for STARTCPU
        w(falcon::CPUCTL, falcon::CPUCTL_STARTCPU);
        w(falcon::CPUCTL_ALIAS, falcon::CPUCTL_STARTCPU);
        std::thread::sleep(std::time::Duration::from_millis(100));

        let cpuctl_post = r(falcon::CPUCTL);
        let tracepc_post = r(0x030);
        let mb0 = r(falcon::MAILBOX0);
        let ok = mb0 == 0xBEEF;
        notes.push(format!(
            "CANARY {name}: pre_cpuctl={cpuctl_pre:#010x} post={cpuctl_post:#010x} \
             pc_pre={tracepc_pre:#010x} pc_post={tracepc_post:#010x} mb0={mb0:#010x} ok={ok}"
        ));
        if ok {
            notes.push(format!(
                "*** CANARY {name} SUCCEEDED — falcon CAN execute code! ***"
            ));
            break;
        }
    }

    // Method B: HALT the running ROM, then upload + restart.
    // cpuctl=0 means the ROM is running. If we can HALT it (set bit 5),
    // then upload code and STARTCPU to restart with our code.
    w(falcon::CPUCTL, falcon::CPUCTL_HALTED);
    std::thread::sleep(std::time::Duration::from_millis(1));
    let cpuctl_after_halt = r(falcon::CPUCTL);
    notes.push(format!(
        "CANARY B: halt attempt: cpuctl={cpuctl_after_halt:#010x} (bit5={})",
        cpuctl_after_halt & falcon::CPUCTL_HALTED != 0
    ));

    // Also try writing to CPUCTL_ALIAS to halt
    w(falcon::CPUCTL_ALIAS, falcon::CPUCTL_HALTED);
    std::thread::sleep(std::time::Duration::from_millis(1));
    let alias_after_halt = r(falcon::CPUCTL_ALIAS);
    let cpuctl_after_alias_halt = r(falcon::CPUCTL);
    notes.push(format!(
        "CANARY B: alias halt: alias={alias_after_halt:#010x} cpuctl={cpuctl_after_alias_halt:#010x}"
    ));

    // If halted, try to upload and start
    if cpuctl_after_halt & falcon::CPUCTL_HALTED != 0
        || cpuctl_after_alias_halt & falcon::CPUCTL_HALTED != 0
    {
        w(falcon::CPUCTL, falcon::CPUCTL_IINVAL);
        std::thread::sleep(std::time::Duration::from_millis(1));
        falcon_imem_upload_nouveau(bar0, base, 0, CANARY_V5_16, 0);
        w(falcon::MAILBOX0, 0);
        w(falcon::BOOTVEC, 0);
        w(falcon::CPUCTL, falcon::CPUCTL_STARTCPU);
        std::thread::sleep(std::time::Duration::from_millis(100));
        let canary_b_mb0 = r(falcon::MAILBOX0);
        notes.push(format!(
            "CANARY B (halt+start): mb0={canary_b_mb0:#010x} ok={}",
            canary_b_mb0 == 0xBEEF
        ));
    }

    // Method C: Read SCTL (0x240) — security mode is informational, not a PIO gate.
    // SCTL is fuse-enforced on GV100 (always LS=0x3000). Writes are ineffective.
    let sctl = r(0x240);
    notes.push(format!("SEC2 SCTL: {sctl:#010x} (informational — does not block PIO)"));

    // Method D: Check EXCI (exception info) and TRACEPC for signs of life
    let exci = r(0x01C);
    let tracepc0 = r(0x030);
    let tracepc1 = r(0x034);
    notes.push(format!(
        "SEC2 EXCI={exci:#010x} TRACEPC[0]={tracepc0:#010x} TRACEPC[1]={tracepc1:#010x}"
    ));

    let parsed = match ParsedAcrFirmware::parse(fw) {
        Ok(p) => p,
        Err(e) => {
            notes.push(format!("Firmware parse failed: {e}"));
            return make_fail_result("Direct ACR: parse failed", sec2_before, bar0, notes);
        }
    };

    // Engine-reset SEC2 for ACR load
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("ACR reset failed: {e}"));
    }

    // Invalidate IMEM tags
    w(falcon::CPUCTL, falcon::CPUCTL_IINVAL);
    std::thread::sleep(std::time::Duration::from_millis(1));

    // Configure physical DMA mode (Nouveau: gm200_flcn_fw_load non-instance path)
    falcon_prepare_physical_dma(bar0, base);

    // Upload non_sec code to IMEM starting at offset 0, tags starting at 0
    let non_sec_off = parsed.load_header.non_sec_code_off as usize;
    let non_sec_size = parsed.load_header.non_sec_code_size as usize;
    let non_sec_end = (non_sec_off + non_sec_size).min(parsed.acr_payload.len());
    let non_sec_code = &parsed.acr_payload[non_sec_off..non_sec_end];
    falcon_imem_upload_nouveau(bar0, base, 0, non_sec_code, 0);
    notes.push(format!(
        "IMEM: non_sec [{non_sec_off:#x}..{non_sec_end:#x}] → IMEM@0 tag=0"
    ));

    // Upload sec code to IMEM at non_sec_size offset
    if let Some(&(sec_off, sec_size)) = parsed.load_header.apps.first() {
        let sec_off = sec_off as usize;
        let sec_end = (sec_off + sec_size as usize).min(parsed.acr_payload.len());
        let sec_code = &parsed.acr_payload[sec_off..sec_end];
        let imem_addr = non_sec_size as u32;
        let start_tag = (non_sec_size / 256) as u32;
        falcon_imem_upload_nouveau(bar0, base, imem_addr, sec_code, start_tag);
        notes.push(format!(
            "IMEM: sec [{sec_off:#x}..{sec_end:#x}] → IMEM@{imem_addr:#x} tag={start_tag:#x}"
        ));
    }

    // Verify IMEM upload by reading back first 16 bytes
    w(falcon::IMEMC, 0x0200_0000); // read mode, addr=0
    let mut readback = [0u32; 4];
    for word in &mut readback {
        *word = r(falcon::IMEMD);
    }
    let expected = &non_sec_code[..16.min(non_sec_code.len())];
    let readback_bytes: Vec<u8> = readback.iter().flat_map(|w| w.to_le_bytes()).collect();
    let imem_match = readback_bytes[..expected.len()] == *expected;
    notes.push(format!(
        "IMEM verify: read={:02x?} expected={:02x?} match={imem_match}",
        &readback_bytes[..expected.len()],
        expected
    ));

    // Upload data section to DMEM at offset 0
    let data_off = parsed.load_header.data_dma_base as usize;
    let data_size = parsed.load_header.data_size as usize;
    let data_end = (data_off + data_size).min(parsed.acr_payload.len());
    if data_off < parsed.acr_payload.len() {
        let data = &parsed.acr_payload[data_off..data_end];
        falcon_dmem_upload(bar0, base, 0, data);
        notes.push(format!(
            "DMEM: data [{data_off:#x}..{data_end:#x}] → DMEM@0"
        ));
    }

    // Boot SEC2 at the non_sec entry point (offset 0)
    w(falcon::MAILBOX0, 0);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, 0);
    let cpuctl_pre_start = r(falcon::CPUCTL);
    let alias_en = cpuctl_pre_start & (1 << 6) != 0;
    notes.push(format!(
        "Pre-start cpuctl={cpuctl_pre_start:#010x} alias_en={alias_en}, BOOTVEC=0x0, issuing STARTCPU"
    ));
    falcon_start_cpu(bar0, base);

    // Quick PC sampling (capture falcon state at very short intervals)
    let mut pc_samples = Vec::new();
    for _ in 0..5 {
        std::thread::sleep(std::time::Duration::from_millis(1));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        pc_samples.push(format!("cpuctl={cpuctl:#010x} mb0={mb0:#010x}"));
    }
    notes.push(format!("PC samples (1ms intervals): {:?}", pc_samples));

    // Poll for completion
    let timeout = std::time::Duration::from_secs(3);
    let start = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let mb1 = r(falcon::MAILBOX1);

        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;
        let hreset_back = cpuctl & falcon::CPUCTL_HRESET != 0;

        if mb0 != 0 || halted || hreset_back {
            notes.push(format!(
                "SEC2 stopped: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} ({}ms)",
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

    // Diagnostics
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
    super::sec2_queue::probe_and_bootstrap(bar0, &mut notes);

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);

    post.into_result(
        "Direct ACR IMEM load (no BL DMA)",
        sec2_before,
        sec2_after,
        notes,
    )
}
