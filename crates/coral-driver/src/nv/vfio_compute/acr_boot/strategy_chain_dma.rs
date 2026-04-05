// SPDX-License-Identifier: AGPL-3.0-only
//! DMA-backed ACR bootloader chain strategy.

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::{DmaBackend, MappedBar};
use crate::vfio::dma::DmaBuffer;

use super::boot_result::{AcrBootResult, make_fail_result};
use super::firmware::{AcrFirmwareSet, ParsedAcrFirmware};
use super::sec2_hal::{
    Sec2Probe, falcon_dmem_upload, falcon_engine_reset, falcon_imem_upload_nouveau,
    falcon_start_cpu,
};
use super::wpr::{ACR_IOVA_BASE, build_bl_dmem_desc};

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
