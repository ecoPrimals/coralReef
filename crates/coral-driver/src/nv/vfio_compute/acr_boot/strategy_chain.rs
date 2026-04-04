// SPDX-License-Identifier: AGPL-3.0-only

//! Legacy ACR chain and direct IMEM load strategies.

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::{DmaBackend, MappedBar};
use crate::vfio::dma::DmaBuffer;

use super::boot_result::{AcrBootResult, make_fail_result};
use super::firmware::{AcrFirmwareSet, ParsedAcrFirmware};
use super::sec2_hal::{
    Sec2Probe, falcon_dmem_upload, falcon_engine_reset, falcon_imem_upload_nouveau,
    falcon_prepare_physical_dma, falcon_start_cpu,
};
use super::wpr::{ACR_IOVA_BASE, build_bl_dmem_desc, build_wpr, patch_acr_desc};

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

    // ── Step 3b: Warm-up STARTCPU ──
    // On HS+ Volta, the first STARTCPU after engine reset often fails but
    // "primes" the boot ROM state machine. A second STARTCPU then works.
    // Evidence: Strategy 12's BL (second STARTCPU) runs while its stub
    // (first STARTCPU) doesn't. Strategy 10 works because it has heavy
    // register traffic (12KB IMEM + 4KB DMEM upload) between reset and start.
    w(falcon::BOOTVEC, 0);
    w(falcon::CPUCTL, falcon::CPUCTL_STARTCPU);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let cpuctl_after_prime = r(falcon::CPUCTL);
    let pc_after_prime = r(falcon::PC);
    notes.push(format!(
        "Prime STARTCPU: cpuctl={cpuctl_after_prime:#010x} pc={pc_after_prime:#06x}"
    ));

    // ── Step 4: Falcon-side DMA fixup stub ──
    //
    // HS+ mode locks FBIF_TRANSCFG and DMACTL from HOST MMIO writes, but
    // falcon-internal `iowr` instructions bypass this lock. We upload a
    // fuc5 program that sets FBIF_TRANSCFG → SYS_MEM_COH and DMACTL →
    // enabled, writes a sentinel to MAILBOX0, then exits.
    //
    // fuc5 encoding (verified against envytools falcon.c ISA tables):
    //   mov: 0x0N=imm8, 0x4N=imm16s, 0x8N=imm24s, 0xdN=imm32
    //   iowr I[$rX], $rY: 0xf6, (Y<<4)|X, 0x00
    //   exit: 0xf8, 0x02
    #[rustfmt::skip]
    const DMA_FIXUP_STUB: &[u8] = &[
        0x40, 0x82, 0x00,                   // mov $r0, 0x0082  (SYS_MEM_COH|PHYS_OVERRIDE)
        0xd1, 0x24, 0x06, 0x00, 0x00,       // mov $r1, 0x00000624  (FBIF_TRANSCFG)
        0xf6, 0x01, 0x00,                   // iowr I[$r1], $r0
        0x40, 0x02, 0x00,                   // mov $r0, 0x0002  (DMA enable)
        0xd1, 0x0c, 0x01, 0x00, 0x00,       // mov $r1, 0x0000010c  (DMACTL)
        0xf6, 0x01, 0x00,                   // iowr I[$r1], $r0
        0xd0, 0xce, 0xda, 0x00, 0x00,       // mov $r0, 0x0000dace  (sentinel)
        0x41, 0x40, 0x00,                   // mov $r1, 0x0040  (MAILBOX0)
        0xf6, 0x01, 0x00,                   // iowr I[$r1], $r0
        0xf8, 0x02,                         // exit
    ];
    const DMA_FIXUP_SENTINEL: u32 = 0xDACE;

    let fbif_pre = r(falcon::FBIF_TRANSCFG);
    let dmactl_pre = r(falcon::DMACTL);
    notes.push(format!(
        "Pre-stub: FBIF={fbif_pre:#010x} DMACTL={dmactl_pre:#010x}"
    ));

    w(falcon::CPUCTL, falcon::CPUCTL_IINVAL);
    std::thread::sleep(std::time::Duration::from_millis(1));
    falcon_imem_upload_nouveau(bar0, base, 0, DMA_FIXUP_STUB, 0);

    // Pad IMEM upload with zeros to simulate heavy register traffic that
    // the working Strategy 10 has (12KB upload seems to satisfy HS+ readiness).
    // Upload at least one more full 256-byte page with tag.
    {
        let pad = [0u8; 256];
        falcon_imem_upload_nouveau(bar0, base, 0x100, &pad, 1);
    }

    // Also do a DMEM write (Strategy 10 uploads 4KB DMEM before STARTCPU)
    falcon_dmem_upload(bar0, base, 0, &[0u8; 256]);

    // Verify stub upload by reading back first 4 words
    w(falcon::IMEMC, 0x0200_0000); // read mode, addr=0
    let mut stub_readback = [0u32; 4];
    for word in &mut stub_readback {
        *word = r(falcon::IMEMD);
    }
    let stub_expected: Vec<u8> = stub_readback.iter().flat_map(|w| w.to_le_bytes()).collect();
    let stub_match = stub_expected[..DMA_FIXUP_STUB.len().min(16)]
        == DMA_FIXUP_STUB[..DMA_FIXUP_STUB.len().min(16)];
    notes.push(format!(
        "Stub IMEM verify: read={:02x?} match={stub_match}",
        &stub_expected[..DMA_FIXUP_STUB.len().min(16)]
    ));

    w(falcon::MAILBOX0, 0);
    w(falcon::BOOTVEC, 0);

    let cpuctl_pre_stub = r(falcon::CPUCTL);
    notes.push(format!(
        "Stub launch: cpuctl={cpuctl_pre_stub:#010x} BOOTVEC=0"
    ));
    falcon_start_cpu(bar0, base);

    let stub_start = std::time::Instant::now();
    let stub_timeout = std::time::Duration::from_millis(500);
    let mut stub_ok = false;
    while stub_start.elapsed() < stub_timeout {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let mb0 = r(falcon::MAILBOX0);
        if mb0 == DMA_FIXUP_SENTINEL {
            stub_ok = true;
            break;
        }
        // Only check STOPPED, not HALTED (HALTED may already be set from boot ROM)
        let cpuctl = r(falcon::CPUCTL);
        if cpuctl & falcon::CPUCTL_STOPPED != 0 {
            notes.push(format!(
                "Stub stopped (mb0={mb0:#010x} cpuctl={cpuctl:#010x}), in {}ms",
                stub_start.elapsed().as_millis()
            ));
            stub_ok = mb0 == DMA_FIXUP_SENTINEL;
            break;
        }
    }

    let fbif_post = r(falcon::FBIF_TRANSCFG);
    let dmactl_post = r(falcon::DMACTL);
    notes.push(format!(
        "Post-stub: FBIF={fbif_post:#010x} DMACTL={dmactl_post:#010x} sentinel={stub_ok} ({}ms)",
        stub_start.elapsed().as_millis()
    ));

    if !stub_ok {
        notes.push("DMA fixup stub did not execute — falling back to host-side config".into());
        w(falcon::DMACTL, 0x02);
    }

    // ── Step 5: Load BL code → IMEM ──
    // Invalidate IMEM cache (stub code is stale) WITHOUT engine reset,
    // preserving the DMA config the stub just wrote.
    w(falcon::CPUCTL, falcon::CPUCTL_IINVAL);
    std::thread::sleep(std::time::Duration::from_millis(1));

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
    // Patch ctx_dma to PHYS_SYS_COH so the BL uses physical system memory DMA.
    // This matches the per-index FBIF_TRANSCFG we configured in Step 4.
    let mut bl_desc_bytes = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    use super::firmware::dma_idx;
    let ctx_dma = dma_idx::PHYS_SYS_COH;
    bl_desc_bytes[32..36].copy_from_slice(&ctx_dma.to_le_bytes());
    notes.push(format!(
        "BL DMEM desc: {} bytes → DMEM@0 (ctx_dma={ctx_dma} = PHYS_SYS_COH)",
        bl_desc_bytes.len()
    ));
    falcon_dmem_upload(bar0, base, 0, &bl_desc_bytes);

    // ── Step 7: Boot SEC2 ──
    // Verify DMA config set by the falcon stub persisted through IMEM/DMEM upload.
    let fbif_final = r(falcon::FBIF_TRANSCFG);
    let dmactl_final = r(falcon::DMACTL);
    notes.push(format!(
        "Pre-boot DMA check: FBIF={fbif_final:#010x} DMACTL={dmactl_final:#010x}"
    ));
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

        // Success: firmware-halted bit set (HALTED) and cpuctl changed vs pre-boot
        let stopped = cpuctl & falcon::CPUCTL_STOPPED != 0;
        let fw_halted = cpuctl & falcon::CPUCTL_HALTED != 0;

        if fw_halted && cpuctl != sec2_before.cpuctl {
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
        if stopped {
            notes.push(format!(
                "SEC2 stopped (no mailbox): cpuctl={cpuctl:#010x} ({}ms)",
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
    // Encoding A: fuc5 16-bit MOV (opcode 0x4N) + iowr (0xf6, reg_data<<4|reg_io)
    //   Corrected from envytools falcon.c ISA tables: mov=0x4N (16-bit), iowr byte1 low=io_reg
    const CANARY_V5_16: &[u8] = &[
        0xd0, 0xef, 0xbe, 0x00, 0x00, // mov $r0, 0x0000beef (32-bit, avoids sign-ext)
        0x41, 0x40, 0x00, // mov $r1, 0x0040 (MAILBOX0 I/O offset)
        0xf6, 0x01, 0x00, // iowr I[$r1], $r0
        0xf8, 0x02, // exit
    ];
    // Encoding B: all-32-bit variant
    const CANARY_V5_32: &[u8] = &[
        0xd0, 0xef, 0xbe, 0x00, 0x00, // mov $r0, 0x0000beef
        0xd1, 0x40, 0x00, 0x00, 0x00, // mov $r1, 0x00000040
        0xf6, 0x01, 0x00, // iowr I[$r1], $r0
        0xf8, 0x02, // exit
    ];
    // Encoding C: minimal 8-bit mov (opcode 0x0N) for small values
    const CANARY_ORIG: &[u8] = &[
        0xd0, 0xef, 0xbe, 0x00, 0x00, // mov $r0, 0x0000beef
        0xd1, 0x40, 0x00, 0x00, 0x00, // mov $r1, 0x00000040
        0xf6, 0x01, 0x00, // iowr I[$r1], $r0
        0xf8, 0x02, // exit
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
    w(falcon::CPUCTL, falcon::CPUCTL_STOPPED);
    std::thread::sleep(std::time::Duration::from_millis(1));
    let cpuctl_after_halt = r(falcon::CPUCTL);
    notes.push(format!(
        "CANARY B: halt attempt: cpuctl={cpuctl_after_halt:#010x} (bit5={})",
        cpuctl_after_halt & falcon::CPUCTL_STOPPED != 0
    ));

    // Also try writing to CPUCTL_ALIAS to halt
    w(falcon::CPUCTL_ALIAS, falcon::CPUCTL_STOPPED);
    std::thread::sleep(std::time::Duration::from_millis(1));
    let alias_after_halt = r(falcon::CPUCTL_ALIAS);
    let cpuctl_after_alias_halt = r(falcon::CPUCTL);
    notes.push(format!(
        "CANARY B: alias halt: alias={alias_after_halt:#010x} cpuctl={cpuctl_after_alias_halt:#010x}"
    ));

    // If halted, try to upload and start
    if cpuctl_after_halt & falcon::CPUCTL_STOPPED != 0
        || cpuctl_after_alias_halt & falcon::CPUCTL_STOPPED != 0
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
    notes.push(format!(
        "SEC2 SCTL: {sctl:#010x} (informational — does not block PIO)"
    ));

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
    let sctl_pre_start = r(falcon::SCTL);
    let alias_en = cpuctl_pre_start & (1 << 6) != 0;
    notes.push(format!(
        "Pre-start cpuctl={cpuctl_pre_start:#010x} sctl={sctl_pre_start:#010x} alias_en={alias_en}, BOOTVEC=0x0"
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

        let stopped = cpuctl & falcon::CPUCTL_STOPPED != 0;
        let fw_halted = cpuctl & falcon::CPUCTL_HALTED != 0;

        if mb0 != 0 || stopped || fw_halted {
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

/// Strategy 7b: PIO ACR upload with VRAM WPR pre-populated.
///
/// Combines the best of Strategies 2 and 7:
/// - Pre-populates WPR content in VRAM via PRAMIN (no falcon DMA needed)
/// - Loads full ACR firmware into SEC2 IMEM/DMEM via PIO (no BL DMA needed)
/// - Patches ACR descriptor with VRAM WPR boundaries + blob_size=0
///
/// The ACR firmware reads the descriptor, sees blob_size=0 (WPR already
/// populated), reads WPR headers from VRAM, verifies signatures, and starts
/// FECS/GPCCS. The only DMA the ACR does is intra-VRAM reads for WPR headers.
pub fn attempt_pio_acr_with_vram_wpr(bar0: &MappedBar, fw: &AcrFirmwareSet) -> AcrBootResult {
    use crate::vfio::memory::{MemoryRegion, PraminRegion};

    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    // ── Step 1: Build WPR content with FECS + GPCCS firmware ──
    let vram_wpr: u32 = 0x70000;
    let vram_shadow: u32 = 0x60000;
    let wpr_data = build_wpr(fw, vram_wpr as u64);
    let wpr_end = vram_wpr + wpr_data.len() as u32;
    notes.push(format!(
        "WPR: {}B at VRAM@{vram_wpr:#x}..{wpr_end:#x} shadow@{vram_shadow:#x}",
        wpr_data.len()
    ));

    // ── Step 2: Write WPR + shadow to VRAM via PRAMIN ──
    let write_pramin = |vram_addr: u32, data: &[u8], label: &str| -> bool {
        let mut off = 0usize;
        while off < data.len() {
            let chunk_vram = vram_addr + off as u32;
            let chunk_size = (data.len() - off).min(0xC000);
            match PraminRegion::new(bar0, chunk_vram, chunk_size) {
                Ok(mut region) => {
                    for wo in (0..chunk_size).step_by(4) {
                        let src = off + wo;
                        if src >= data.len() {
                            break;
                        }
                        let end = (src + 4).min(data.len());
                        let mut bytes = [0u8; 4];
                        bytes[..end - src].copy_from_slice(&data[src..end]);
                        if region.write_u32(wo, u32::from_le_bytes(bytes)).is_err() {
                            return false;
                        }
                    }
                    off += chunk_size;
                }
                Err(_) => return false,
            }
        }
        tracing::info!("{label}: {}B → VRAM@{vram_addr:#x}", data.len());
        true
    };

    if !write_pramin(vram_wpr, &wpr_data, "WPR") {
        notes.push("PRAMIN WPR write failed".to_string());
        return make_fail_result("PIO ACR+WPR: PRAMIN failed", sec2_before, bar0, notes);
    }
    if !write_pramin(vram_shadow, &wpr_data, "Shadow") {
        notes.push("PRAMIN shadow write failed".to_string());
    }

    // Verify WPR sentinel
    if let Ok(region) = PraminRegion::new(bar0, vram_wpr, 8) {
        let hdr0 = region.read_u32(0).unwrap_or(0);
        notes.push(format!("WPR verify: hdr[0]={hdr0:#010x} (expect FECS id=2)"));
    }

    // ── Step 3: Parse ACR firmware and patch descriptor ──
    let parsed = match ParsedAcrFirmware::parse(fw) {
        Ok(p) => p,
        Err(e) => {
            notes.push(format!("ACR parse failed: {e}"));
            return make_fail_result("PIO ACR+WPR: parse failed", sec2_before, bar0, notes);
        }
    };

    let mut payload = parsed.acr_payload.clone();
    let data_off = parsed.load_header.data_dma_base as usize;

    // Patch WPR boundaries. blob_size=0 tells ACR to skip DMA-ing the blob
    // (WPR is pre-populated in VRAM).
    patch_acr_desc(
        &mut payload,
        data_off,
        vram_wpr as u64,
        wpr_end as u64,
        vram_shadow as u64,
    );

    // Override blob_size=0: WPR is already pre-populated in VRAM via PRAMIN.
    // With blob_size=0 the ACR skips the DMA-based blob copy (which would fail
    // since SEC2's DMA engine is not functional in sovereign boot).
    if payload.len() >= data_off + 0x268 {
        payload[data_off + 0x258..data_off + 0x25C].copy_from_slice(&0u32.to_le_bytes());
        // Also set ucode_blob_base to 0 (ACR ignores it when blob_size=0).
        payload[data_off + 0x260..data_off + 0x268].copy_from_slice(&0u64.to_le_bytes());
    }

    // Verify the patch
    if payload.len() >= data_off + 0x268 {
        let r32 = |off: usize| u32::from_le_bytes(payload[off..off + 4].try_into().unwrap_or([0; 4]));
        let blob_sz = r32(data_off + 0x258);
        let wpr_id = r32(data_off + 0x210);
        let regions = r32(data_off + 0x21C);
        let start = r32(data_off + 0x220);
        let end = r32(data_off + 0x224);
        notes.push(format!(
            "ACR desc patched: wpr_id={wpr_id} regions={regions} start={start:#x} end={end:#x} blob_size={blob_sz:#x}"
        ));
    }

    // ── Step 4: Engine-reset SEC2 and configure ──
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("SEC2 reset failed: {e}"));
    }

    let _ = bar0.write_u32(base + falcon::CPUCTL, falcon::CPUCTL_IINVAL);
    std::thread::sleep(std::time::Duration::from_millis(1));

    falcon_prepare_physical_dma(bar0, base);

    // ── Step 5: Build modified non_sec code with inline DMA fixup ──
    //
    // On HS+ Volta, the boot ROM only allows STARTCPU at BOOTVEC=0 (validated
    // tag chain). Host MMIO writes to FBIF_TRANSCFG target bits and DMACTL are
    // locked. Solution: prepend fuc5 DMA configuration instructions to the
    // non_sec code page. They execute before the ACR entry, configuring DMA
    // for system memory access from within the falcon's privileged context.
    //
    // fuc5 DMA fixup: preserve original first instruction (magic word at IMEM[0]),
    // insert DMA config using $r14/$r15 after it, then continue with original code.
    //
    // Layout: [orig_insn0 (5B)] [DMA fixup (22B)] [orig_insns 1..N]
    #[rustfmt::skip]
    const DMA_FIXUP_INNER: &[u8] = &[
        0x4e, 0x82, 0x00,                   // mov $r14, 0x0082
        0xdf, 0x24, 0x06, 0x00, 0x00,       // mov $r15, 0x00000624  (FBIF_TRANSCFG)
        0xf6, 0xef, 0x00,                   // iowr I[$r15], $r14
        0x4e, 0x02, 0x00,                   // mov $r14, 0x0002
        0xdf, 0x0c, 0x01, 0x00, 0x00,       // mov $r15, 0x0000010c  (DMACTL)
        0xf6, 0xef, 0x00,                   // iowr I[$r15], $r14
    ];
    const FIRST_INSN_LEN: usize = 5; // `mov $r0, imm32` = 5 bytes

    let non_sec_off = parsed.load_header.non_sec_code_off as usize;
    let non_sec_size = parsed.load_header.non_sec_code_size as usize;
    let non_sec_end = (non_sec_off + non_sec_size).min(payload.len());
    let non_sec_code = &payload[non_sec_off..non_sec_end];

    // Build: [first_insn] [DMA_FIXUP_INNER] [rest of original non_sec]
    let mut modified_non_sec = vec![0u8; non_sec_size];
    let mut pos = 0usize;
    // Copy original first instruction (preserves boot ROM magic word check)
    let first_insn = &non_sec_code[..FIRST_INSN_LEN.min(non_sec_code.len())];
    modified_non_sec[pos..pos + first_insn.len()].copy_from_slice(first_insn);
    pos += first_insn.len();
    // Insert DMA fixup
    modified_non_sec[pos..pos + DMA_FIXUP_INNER.len()].copy_from_slice(DMA_FIXUP_INNER);
    pos += DMA_FIXUP_INNER.len();
    // Copy remaining original code
    let rest_start = FIRST_INSN_LEN.min(non_sec_code.len());
    let rest = &non_sec_code[rest_start..];
    let rest_to_copy = rest.len().min(non_sec_size - pos);
    modified_non_sec[pos..pos + rest_to_copy].copy_from_slice(&rest[..rest_to_copy]);

    notes.push(format!(
        "Modified non_sec: first_insn({}B) + DMA_fixup({}B) + rest({}B) → {}B IMEM@0",
        first_insn.len(), DMA_FIXUP_INNER.len(), rest_to_copy, modified_non_sec.len()
    ));
    notes.push(format!(
        "Original non_sec first 16B: {:02x?}",
        &non_sec_code[..16.min(non_sec_code.len())]
    ));
    notes.push(format!(
        "Modified non_sec first 32B: {:02x?}",
        &modified_non_sec[..32.min(modified_non_sec.len())]
    ));

    falcon_imem_upload_nouveau(bar0, base, 0, &modified_non_sec, 0);

    if let Some(&(sec_off, sec_size)) = parsed.load_header.apps.first() {
        let sec_off = sec_off as usize;
        let sec_end = (sec_off + sec_size as usize).min(payload.len());
        let sec_code = &payload[sec_off..sec_end];
        let imem_addr = non_sec_size as u32;
        let start_tag = (non_sec_size / 256) as u32;
        falcon_imem_upload_nouveau(bar0, base, imem_addr, sec_code, start_tag);
        notes.push(format!(
            "IMEM: sec [{sec_off:#x}..{sec_end:#x}] → IMEM@{imem_addr:#x} tag={start_tag:#x}"
        ));
    }

    // Verify IMEM: first word should be start of original first instruction
    let _ = bar0.write_u32(base + falcon::IMEMC, 0x0200_0000);
    let imem0 = r(falcon::IMEMD);
    let expected_word =
        u32::from_le_bytes(modified_non_sec[..4].try_into().unwrap_or([0; 4]));
    let match_ok = imem0 == expected_word;
    notes.push(format!(
        "IMEM verify: read={imem0:#010x} expected={expected_word:#010x} match={match_ok}"
    ));

    // ── Step 6: Upload patched data section to DMEM via PIO ──
    let data_size = parsed.load_header.data_size as usize;
    let data_end = (data_off + data_size).min(payload.len());
    if data_off < payload.len() {
        let data = &payload[data_off..data_end];
        falcon_dmem_upload(bar0, base, 0, data);
        notes.push(format!(
            "DMEM: data [{data_off:#x}..{data_end:#x}] → DMEM@0 (patched descriptor)"
        ));
    }

    // ── Step 7: Warm-up STARTCPU ──
    // HS+ Volta requires a "priming" STARTCPU after engine reset before the
    // real boot succeeds. Evidence: two-phase approach (Phase 1 fails but
    // Phase 2 succeeds) vs single STARTCPU (hangs at cpuctl=0x00).
    w(falcon::BOOTVEC, 0);
    w(falcon::CPUCTL, falcon::CPUCTL_STARTCPU);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let cpuctl_warmup = r(falcon::CPUCTL);
    notes.push(format!("Warm-up STARTCPU: cpuctl={cpuctl_warmup:#010x}"));

    w(falcon::CPUCTL, falcon::CPUCTL_IINVAL);
    std::thread::sleep(std::time::Duration::from_millis(1));

    // ── Step 8: Start SEC2 ──
    let fbif_pre = r(falcon::FBIF_TRANSCFG);
    let dmactl_pre = r(falcon::DMACTL);
    let sctl_pre = r(falcon::SCTL);
    notes.push(format!(
        "Pre-boot DMA: FBIF={fbif_pre:#010x} DMACTL={dmactl_pre:#010x} SCTL={sctl_pre:#010x}"
    ));

    w(falcon::MAILBOX0, 0);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, 0);

    let cpuctl_pre = r(falcon::CPUCTL);
    notes.push(format!(
        "Pre-start: cpuctl={cpuctl_pre:#010x} BOOTVEC=0x0, issuing STARTCPU"
    ));
    falcon_start_cpu(bar0, base);

    // ── Step 8: Poll for ACR completion ──
    let timeout = std::time::Duration::from_secs(5);
    let start = std::time::Instant::now();
    let mut last_mb0 = 0u32;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let mb1 = r(falcon::MAILBOX1);
        last_mb0 = mb0;

        let stopped = cpuctl & falcon::CPUCTL_STOPPED != 0;
        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;

        if mb0 != 0 || stopped || halted {
            notes.push(format!(
                "SEC2 done: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} ({}ms)",
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

    // Check if inline DMA fixup changed FBIF/DMACTL
    let fbif_post = r(falcon::FBIF_TRANSCFG);
    let dmactl_post = r(falcon::DMACTL);
    let dma_fix_applied = (fbif_post & 0x03) == 0x02 || dmactl_post != dmactl_pre;
    notes.push(format!(
        "Post-boot DMA: FBIF={fbif_post:#010x} DMACTL={dmactl_post:#010x} \
         fix_applied={dma_fix_applied} (was FBIF={fbif_pre:#010x} DMACTL={dmactl_pre:#010x})"
    ));

    // ── Step 9: Check FECS state ──
    let fecs_cpuctl = bar0
        .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    let fecs_pc = bar0
        .read_u32(falcon::FECS_BASE + falcon::PC)
        .unwrap_or(0xDEAD);
    let fecs_mb0 = bar0
        .read_u32(falcon::FECS_BASE + falcon::MAILBOX0)
        .unwrap_or(0xDEAD);
    let gpccs_cpuctl = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    notes.push(format!(
        "FECS: cpuctl={fecs_cpuctl:#010x} pc={fecs_pc:#06x} mb0={fecs_mb0:#010x}"
    ));
    notes.push(format!("GPCCS: cpuctl={gpccs_cpuctl:#010x}"));

    // Check WPR status
    if let Ok(region) = PraminRegion::new(bar0, vram_wpr, 64) {
        let fecs_status = region.read_u32(20).unwrap_or(0xDEAD);
        let gpccs_status = region.read_u32(44).unwrap_or(0xDEAD);
        notes.push(format!(
            "WPR status: FECS={fecs_status:#x} GPCCS={gpccs_status:#x} (0xFF=done, 1=copy)"
        ));
    }

    // Diagnostics
    let exci = r(falcon::EXCI);
    notes.push(format!("SEC2 EXCI={exci:#010x}"));

    let success = last_mb0 == 0 && fecs_cpuctl & falcon::CPUCTL_HALTED == 0 && fecs_pc > 0x100;

    super::sec2_queue::probe_and_bootstrap(bar0, &mut notes);

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);

    post.into_result(
        "PIO ACR + VRAM WPR (Strategy 7b)",
        sec2_before,
        sec2_after,
        notes,
    )
}

/// Strategy 7c: Dual-phase boot with hybrid VRAM/sysmem page tables.
///
/// Replicates Strategy 6's proven dual-phase boot (legacy PDEs → hot-swap),
/// with one key change: WPR page table entries point to system memory DMA
/// buffers via `encode_sysmem_pte` instead of VRAM. ACR payload pages use
/// VRAM identity-mapping (proven to work), so the BL can DMA the ACR from
/// VRAM during the physical-fallback phase. After hot-swap, the ACR reads
/// WPR from system memory through the hybrid page tables.
pub fn attempt_pio_acr_with_sysmem_wpr(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    container: DmaBackend,
) -> AcrBootResult {
    use super::sec2_hal::sec2_emem_write;
    use super::firmware::dma_idx;

    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    // ── Step 1: Parse firmware ──
    let parsed = match ParsedAcrFirmware::parse(fw) {
        Ok(p) => p,
        Err(e) => {
            notes.push(format!("ACR parse failed: {e}"));
            return make_fail_result("7c: parse failed", sec2_before, bar0, notes);
        }
    };
    let data_off = parsed.load_header.data_dma_base as usize;
    let payload_size = parsed.acr_payload.len();

    // ── Step 2: Allocate DMA for ACR payload ──
    let acr_iova: u64 = ACR_IOVA_BASE;
    let mut acr_dma = match DmaBuffer::new(container.clone(), payload_size, acr_iova) {
        Ok(buf) => buf,
        Err(e) => {
            notes.push(format!("DMA alloc failed for ACR: {e}"));
            return make_fail_result("7c: ACR DMA failed", sec2_before, bar0, notes);
        }
    };

    // ── Step 3: Allocate DMA for WPR ──
    let wpr_iova: u64 = ACR_IOVA_BASE + 0x10_0000;
    let wpr_data = build_wpr(fw, wpr_iova);
    let wpr_size = wpr_data.len();
    let wpr_end = wpr_iova + wpr_size as u64;
    let shadow_iova = wpr_iova;

    let mut wpr_dma = match DmaBuffer::new(container, wpr_size, wpr_iova) {
        Ok(buf) => buf,
        Err(e) => {
            notes.push(format!("DMA alloc failed for WPR: {e}"));
            return make_fail_result("7c: WPR DMA failed", sec2_before, bar0, notes);
        }
    };
    wpr_dma.as_mut_slice()[..wpr_size].copy_from_slice(&wpr_data);
    notes.push(format!(
        "DMA: ACR={}B@{acr_iova:#x} WPR={}B@{wpr_iova:#x}",
        payload_size, wpr_size
    ));

    // ── Step 4: Patch ACR descriptor for sysmem WPR ──
    let mut payload = parsed.acr_payload.clone();
    patch_acr_desc(&mut payload, data_off, wpr_iova, wpr_end, shadow_iova);

    if payload.len() >= data_off + 0x268 {
        let r32 =
            |off: usize| u32::from_le_bytes(payload[off..off + 4].try_into().unwrap_or([0; 4]));
        notes.push(format!(
            "ACR desc: wpr=[{:#x}..{:#x}] shadow={:#x} blob_size={:#x} blob_base={:#x}",
            r32(data_off + 0x220), r32(data_off + 0x224), r32(data_off + 0x238),
            r32(data_off + 0x258), r32(data_off + 0x260),
        ));
    }

    // Copy patched payload to DMA buffer
    acr_dma.as_mut_slice()[..payload_size].copy_from_slice(&payload);

    let code_dma_base = acr_iova;
    let data_dma_base = acr_iova + data_off as u64;
    notes.push(format!("DMA addrs: code={code_dma_base:#x} data={data_dma_base:#x}"));

    // ── Step 5: Engine-reset SEC2 ──
    // falcon_engine_reset runs PMC reset → boot ROM → halt.
    // Builds VRAM instance block with identity-mapped VRAM page tables at 0x10000.
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("Engine reset failed: {e}"));
    } else {
        notes.push(format!(
            "Post-reset cpuctl={:#010x} sctl={:#010x}",
            r(falcon::CPUCTL), r(falcon::SCTL)
        ));
    }

    // ── Step 6: Patch VRAM page tables with SYS_MEM PTEs ──
    // The instance block (from engine reset) identity-maps first 2MB of VRAM.
    // FBIF is locked in VIRT mode by HS+, so DMA goes through the falcon MMU.
    // We need SYS_MEM PTEs so the MMU routes ACR/WPR DMA to system memory
    // (via PCIe → IOMMU) instead of VRAM.
    {
        use super::instance_block::*;
        use crate::vfio::memory::{MemoryRegion, PraminRegion};

        let wv64 = |vram_addr: u32, offset: usize, val: u64| -> bool {
            let lo = (val & 0xFFFF_FFFF) as u32;
            let hi = (val >> 32) as u32;
            let ok_lo = PraminRegion::new(bar0, vram_addr, offset + 4)
                .ok()
                .and_then(|mut r| r.write_u32(offset, lo).ok())
                .is_some();
            let ok_hi = PraminRegion::new(bar0, vram_addr, offset + 8)
                .ok()
                .and_then(|mut r| r.write_u32(offset + 4, hi).ok())
                .is_some();
            ok_lo && ok_hi
        };
        let wv32 = |vram_addr: u32, offset: usize, val: u32| -> bool {
            PraminRegion::new(bar0, vram_addr, offset + 4)
                .ok()
                .and_then(|mut r| r.write_u32(offset, val).ok())
                .is_some()
        };

        // PT0 covers VA 0x000000..0x1FFFFF (pages 0..511).
        // ACR payload at IOVA acr_iova (0x180000): pages 384..388 (5 pages for 18176B).
        let acr_start_page = (acr_iova / 4096) as usize;
        let acr_pages = (payload_size + 4095) / 4096;
        let mut pt_ok = true;
        for i in 0..acr_pages {
            let page_iova = acr_iova + (i as u64) * 4096;
            let pte = encode_sysmem_pte(page_iova);
            let slot = (acr_start_page + i) * 8;
            pt_ok &= wv64(FALCON_PT0_VRAM, slot, pte);
        }
        notes.push(format!(
            "PT0: SYS_MEM pages {acr_start_page}..{} for ACR@{acr_iova:#x} (ok={pt_ok})",
            acr_start_page + acr_pages
        ));

        // WPR at IOVA wpr_iova (0x280000): in second 2MB range → needs PT1.
        // Allocate PT1 at VRAM 0x16000.
        const FALCON_PT1_VRAM: u32 = 0x16000;

        // Zero PT1
        for off in (0..0x1000).step_by(4) {
            wv32(FALCON_PT1_VRAM, off, 0);
        }

        // PD0[1] → PT1 (dual PDE at PD0 offset 16: lower 8B=0, upper 8B=PT1 pointer)
        let pd0_entry1_lo = 16usize; // offset of entry 1's lower 8 bytes
        let pd0_entry1_hi = 24usize; // offset of entry 1's upper 8 bytes
        wv64(FALCON_PD0_VRAM, pd0_entry1_lo, 0); // no big PT
        let pd0_ok = wv64(FALCON_PD0_VRAM, pd0_entry1_hi, encode_vram_pde(FALCON_PT1_VRAM as u64));
        notes.push(format!("PD0[1]→PT1@{FALCON_PT1_VRAM:#x} (ok={pd0_ok})"));

        // PT1 covers VA 0x200000..0x3FFFFF (pages 512..1023).
        // WPR page index within PT1 = (wpr_iova - 0x200000) / 4096
        let wpr_base_in_pt1 = ((wpr_iova - 0x200000) / 4096) as usize;
        let wpr_pages = (wpr_size + 4095) / 4096;
        let mut wpr_pt_ok = true;
        for i in 0..wpr_pages {
            let page_iova = wpr_iova + (i as u64) * 4096;
            let pte = encode_sysmem_pte(page_iova);
            let slot = (wpr_base_in_pt1 + i) * 8;
            wpr_pt_ok &= wv64(FALCON_PT1_VRAM, slot, pte);
        }
        notes.push(format!(
            "PT1: SYS_MEM pages {wpr_base_in_pt1}..{} for WPR@{wpr_iova:#x} (ok={wpr_pt_ok})",
            wpr_base_in_pt1 + wpr_pages
        ));
    }

    // ── Step 7: Load BL code → IMEM ──
    w(falcon::CPUCTL, falcon::CPUCTL_IINVAL);
    std::thread::sleep(std::time::Duration::from_millis(1));

    let hwcfg = r(falcon::HWCFG);
    let code_limit = falcon::imem_size_bytes(hwcfg);
    let boot_size =
        ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;

    falcon_imem_upload_nouveau(bar0, base, imem_addr, &parsed.bl_code, start_tag);

    // ── Step 8: BL data → EMEM, ACR data → DMEM, BL descriptor → DMEM ──
    let bl_payload = fw.acr_bl_parsed.payload(&fw.acr_bl_raw);
    let bl_data_off = parsed.bl_desc.bl_data_off as usize;
    let bl_data_size = parsed.bl_desc.bl_data_size as usize;
    let bl_data_end = (bl_data_off + bl_data_size).min(bl_payload.len());
    if bl_data_off < bl_payload.len() && bl_data_size > 0 {
        sec2_emem_write(bar0, 0, &bl_payload[bl_data_off..bl_data_end]);
    }

    let data_section = &payload[data_off..];
    falcon_dmem_upload(bar0, base, 0, data_section);
    notes.push(format!(
        "DMEM: data_off={data_off:#x} data_section={}B",
        data_section.len()
    ));

    // BL descriptor with ctx_dma=VIRT — uses falcon MMU with our patched page tables
    let bl_desc_bytes = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    // ctx_dma defaults to VIRT (1) from build_bl_dmem_desc — no patch needed
    falcon_dmem_upload(bar0, base, 0, &bl_desc_bytes);

    notes.push(format!(
        "BL: IMEM@{imem_addr:#x} boot={boot_addr:#x} code_dma={code_dma_base:#x} ctx_dma=VIRT"
    ));

    // ── Step 9: Boot SEC2 ──
    w(falcon::EXCI, 0);
    w(falcon::MAILBOX0, 0);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, boot_addr);
    notes.push(format!("BOOTVEC={boot_addr:#x}, issuing STARTCPU"));
    falcon_start_cpu(bar0, base);

    // ── Step 9b: DMEM repair ──
    // The BL descriptor (84B) overwrites data_section[0..84] in DMEM@0.
    // The BL reads it in microseconds, then should DMA-load the real data section.
    // If BL data-DMA fails, the first 84 bytes stay corrupted → ACR loops forever.
    // Fix: after BL has read the descriptor, PIO-write the correct bytes back.
    std::thread::sleep(std::time::Duration::from_millis(10));
    let repair_len = bl_desc_bytes.len().min(data_section.len());
    {
        use super::sec2_hal::sec2_dmem_read;
        let pre = sec2_dmem_read(bar0, 0, 8);
        falcon_dmem_upload(bar0, base, 0, &data_section[..repair_len]);
        let post = sec2_dmem_read(bar0, 0, 8);
        notes.push(format!(
            "DMEM repair: {}B @ DMEM@0 pre=[{:#010x},{:#010x}] post=[{:#010x},{:#010x}]",
            repair_len,
            pre.first().copied().unwrap_or(0),
            pre.get(1).copied().unwrap_or(0),
            post.first().copied().unwrap_or(0),
            post.get(1).copied().unwrap_or(0),
        ));
    }

    // ── Step 10: Poll for ACR completion ──
    let timeout = std::time::Duration::from_secs(5);
    let start_time = std::time::Instant::now();
    let mut all_pcs: Vec<u32> = Vec::new();

    for _ in 0..500 {
        let pc = r(falcon::PC);
        if all_pcs.last() != Some(&pc) {
            all_pcs.push(pc);
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
        if start_time.elapsed().as_millis() > 50 { break; }
    }

    let mut settled_count = 0u32;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let pc = r(falcon::PC);
        let stopped = cpuctl & falcon::CPUCTL_STOPPED != 0;
        let fw_halted = cpuctl & falcon::CPUCTL_HALTED != 0;

        if all_pcs.last() != Some(&pc) {
            all_pcs.push(pc);
            settled_count = 0;
        } else {
            settled_count += 1;
        }

        if mb0 != 0 || stopped || fw_halted {
            notes.push(format!(
                "SEC2: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#06x} ({}ms)",
                start_time.elapsed().as_millis()
            ));
            break;
        }
        if settled_count > 200 || start_time.elapsed() > timeout {
            notes.push(format!(
                "SEC2 timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#06x}",
            ));
            break;
        }
    }

    if !all_pcs.is_empty() {
        let trace: Vec<String> = all_pcs.iter().map(|p| format!("{p:#06x}")).collect();
        notes.push(format!("PC trace: [{}]", trace.join(" ")));
    }

    // ── Step 11: Diagnostics ──
    super::boot_diagnostics::capture_post_boot_diagnostics(bar0, base, &mut notes);

    // Check WPR status in DMA buffer
    if wpr_size >= 48 {
        let buf = wpr_dma.as_slice();
        let fecs_status = u32::from_le_bytes(buf[20..24].try_into().unwrap_or([0; 4]));
        let gpccs_status = u32::from_le_bytes(buf[44..48].try_into().unwrap_or([0; 4]));
        notes.push(format!(
            "WPR DMA: FECS status={fecs_status:#x} GPCCS status={gpccs_status:#x}"
        ));
    }

    let sctl = r(falcon::SCTL);
    let hs = sctl & 0x02 != 0;
    notes.push(format!("*** SCTL={sctl:#010x} HS={hs} ***"));

    let fecs_cpuctl = bar0.read_u32(falcon::FECS_BASE + falcon::CPUCTL).unwrap_or(0xDEAD);
    let fecs_mb0 = bar0.read_u32(falcon::FECS_BASE + falcon::MAILBOX0).unwrap_or(0xDEAD);
    let gpccs_cpuctl = bar0.read_u32(falcon::GPCCS_BASE + falcon::CPUCTL).unwrap_or(0xDEAD);
    notes.push(format!(
        "Sub-falcons: FECS cpuctl={fecs_cpuctl:#010x} mb0={fecs_mb0:#010x} \
         GPCCS cpuctl={gpccs_cpuctl:#010x}"
    ));

    super::sec2_queue::probe_and_bootstrap(bar0, &mut notes);

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);

    drop(acr_dma);
    drop(wpr_dma);

    post.into_result(
        "PIO ACR + sysmem WPR (Strategy 7c)",
        sec2_before,
        sec2_after,
        notes,
    )
}
