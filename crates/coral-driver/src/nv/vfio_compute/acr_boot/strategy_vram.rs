// SPDX-License-Identifier: AGPL-3.0-only

//! VRAM-based ACR boot strategies.
//!
//! Two strategies:
//! - `attempt_vram_acr_boot`: Legacy physical DMA path (FBIF override, no bind).
//! - `attempt_vram_native_acr_boot`: **Exp 111** — VRAM page tables + virtual DMA
//!   via instance block bind. Addresses the HS+MMU paradox from Exp 110.

use crate::vfio::channel::registers::{falcon, misc};
use crate::vfio::device::MappedBar;
use crate::vfio::memory::{MemoryRegion, PraminRegion};

use super::boot_result::{AcrBootResult, make_fail_result};
use super::firmware::{AcrFirmwareSet, ParsedAcrFirmware};
use super::instance_block::{
    FALCON_INST_VRAM, FALCON_PD0_VRAM, FALCON_PD1_VRAM, FALCON_PD2_VRAM, FALCON_PD3_VRAM,
    FALCON_PT0_VRAM, build_vram_falcon_inst_block, encode_bind_inst, encode_vram_pde,
    encode_vram_pte, falcon_bind_context,
};
use super::sec2_hal::{
    Sec2Probe, falcon_dmem_upload, falcon_imem_upload_nouveau, falcon_start_cpu,
    find_sec2_pmc_bit, pmc_enable_sec2, sec2_dmem_read, sec2_emem_write,
    sec2_prepare_physical_first,
};
use super::wpr::{build_bl_dmem_desc, build_wpr, falcon_id, patch_acr_desc};

/// Full ACR chain boot: DMA-backed SEC2 → ACR → FECS/GPCCS.
///
/// Implements the complete Nouveau-compatible boot chain:
///
/// 1. Parse firmware blobs (bl.bin, ucode_load.bin)
/// 2. Allocate DMA buffer for ACR firmware payload
/// 3. Patch ACR descriptor with WPR addresses
///
/// VRAM-based ACR boot: write the ACR payload into VRAM via PRAMIN, then
/// have the BL DMA-load it from VRAM using physical addressing.
///
/// Insight: the falcon's physical DMA mode (0x624 | 0x80) reads from GPU
/// VRAM, not from system memory. Previous strategies failed because the BL
/// tried to `xcld` from an IOVA (system memory address) that the falcon
/// couldn't reach. By placing the ACR payload in VRAM and pointing the BL
/// descriptor at VRAM addresses, the DMA path stays entirely on-GPU.
pub fn attempt_vram_acr_boot(bar0: &MappedBar, fw: &AcrFirmwareSet) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    notes.push(format!("SEC2 state: {:?}", sec2_before.state));

    // ── Step 1: Parse firmware ──
    let parsed = match ParsedAcrFirmware::parse(fw) {
        Ok(p) => p,
        Err(e) => {
            notes.push(format!("Firmware parse failed: {e}"));
            return make_fail_result("VRAM ACR: parse failed", sec2_before, bar0, notes);
        }
    };

    let acr_payload = &parsed.acr_payload;
    let payload_size = acr_payload.len();
    notes.push(format!(
        "ACR payload: {}B non_sec=[{:#x}+{:#x}] data=[{:#x}+{:#x}]",
        payload_size,
        parsed.load_header.non_sec_code_off,
        parsed.load_header.non_sec_code_size,
        parsed.load_header.data_dma_base,
        parsed.load_header.data_size,
    ));

    // ── Step 2: Write ACR payload to VRAM via PRAMIN ──
    // Use VRAM offset 0x50000 — above diagnostic areas, within a single 64KB window.
    let vram_base: u32 = 0x0005_0000;
    let vram_sentinel_ok;

    // First verify VRAM is accessible with a sentinel test
    match PraminRegion::new(bar0, vram_base, 8) {
        Ok(mut region) => {
            let sentinel = 0xACB0_0700_u32;
            if let Err(e) = region.write_u32(0, sentinel) {
                notes.push(format!("VRAM sentinel write failed: {e}"));
                vram_sentinel_ok = false;
            } else {
                let rb = region.read_u32(0).unwrap_or(0);
                vram_sentinel_ok = rb == sentinel;
                notes.push(format!(
                    "VRAM@{vram_base:#x} sentinel: wrote={sentinel:#010x} read={rb:#010x} ok={vram_sentinel_ok}"
                ));
            }
        }
        Err(e) => {
            notes.push(format!("PRAMIN region create failed: {e}"));
            vram_sentinel_ok = false;
        }
    }

    if !vram_sentinel_ok {
        notes.push("VRAM not accessible — cannot use VRAM ACR path".to_string());
        return make_fail_result("VRAM ACR: VRAM inaccessible", sec2_before, bar0, notes);
    }

    // ── Step 2a: Build WPR with FECS/GPCCS firmware ──
    let wpr_vram_base: u32 = 0x0007_0000; // WPR starts at VRAM 0x70000
    let wpr_data = build_wpr(fw, wpr_vram_base as u64);
    let wpr_end = wpr_vram_base as u64 + wpr_data.len() as u64;
    notes.push(format!(
        "WPR: {}B at VRAM@{wpr_vram_base:#x}..{wpr_end:#x}",
        wpr_data.len()
    ));

    // ── Step 2b: Patch ACR descriptor in payload with WPR bounds ──
    let mut payload_patched = acr_payload.to_vec();
    let data_off = parsed.load_header.data_dma_base as usize;
    patch_acr_desc(
        &mut payload_patched,
        data_off,
        wpr_vram_base as u64,
        wpr_end,
        wpr_vram_base as u64,
    );
    notes.push(format!(
        "ACR desc patched: wpr_start={wpr_vram_base:#x} wpr_end={wpr_end:#x} at data_off={data_off:#x}"
    ));

    // ── Step 2c: Write ACR payload to VRAM ──
    let write_to_vram =
        |bar0: &MappedBar, vram_addr: u32, data: &[u8], notes: &mut Vec<String>| -> bool {
            let mut off = 0usize;
            while off < data.len() {
                let chunk_vram = vram_addr + off as u32;
                let chunk_size = (data.len() - off).min(0xC000);
                match PraminRegion::new(bar0, chunk_vram, chunk_size) {
                    Ok(mut region) => {
                        for word_off in (0..chunk_size).step_by(4) {
                            let src = off + word_off;
                            if src >= data.len() {
                                break;
                            }
                            let end = (src + 4).min(data.len());
                            let mut bytes = [0u8; 4];
                            bytes[..end - src].copy_from_slice(&data[src..end]);
                            let val = u32::from_le_bytes(bytes);
                            if region.write_u32(word_off, val).is_err() {
                                notes.push(format!(
                                    "VRAM write failed at {chunk_vram:#x}+{word_off:#x}"
                                ));
                                return false;
                            }
                        }
                        off += chunk_size;
                    }
                    Err(e) => {
                        notes.push(format!("PRAMIN at VRAM@{chunk_vram:#x}: {e}"));
                        return false;
                    }
                }
            }
            true
        };

    if !write_to_vram(bar0, vram_base, &payload_patched, &mut notes) {
        return make_fail_result("VRAM ACR: payload write failed", sec2_before, bar0, notes);
    }
    notes.push(format!(
        "ACR payload: {}B → VRAM@{vram_base:#x}",
        payload_patched.len()
    ));

    // ── Step 2d: Write WPR to VRAM ──
    if !write_to_vram(bar0, wpr_vram_base, &wpr_data, &mut notes) {
        return make_fail_result("VRAM ACR: WPR write failed", sec2_before, bar0, notes);
    }
    notes.push(format!(
        "WPR: {}B → VRAM@{wpr_vram_base:#x}",
        wpr_data.len()
    ));

    // Verify ACR payload in VRAM
    if let Ok(region) = PraminRegion::new(bar0, vram_base, 16) {
        let v0 = region.read_u32(0).unwrap_or(0);
        let e0 = u32::from_le_bytes([
            payload_patched[0],
            payload_patched[1],
            payload_patched[2],
            payload_patched[3],
        ]);
        notes.push(format!(
            "VRAM ACR verify: [{v0:#010x}] expected [{e0:#010x}] ok={}",
            v0 == e0
        ));
    }

    // ── Step 2e: Build VRAM instance block for falcon DMA (before reset) ──
    let inst_ok = build_vram_falcon_inst_block(bar0);

    // Verify page tables by reading back critical entries
    let rv = |vram_addr: u32, offset: usize| -> u32 {
        PraminRegion::new(bar0, vram_addr, offset + 4)
            .ok()
            .and_then(|r| r.read_u32(offset).ok())
            .unwrap_or(0xDEAD)
    };
    let pdb = rv(FALCON_INST_VRAM, 0x200);
    let pdb_hi = rv(FALCON_INST_VRAM, 0x204);
    let pt112_lo = rv(FALCON_PT0_VRAM, 112 * 8);
    let pt112_hi = rv(FALCON_PT0_VRAM, 112 * 8 + 4);
    notes.push(format!(
        "VRAM inst: built={inst_ok} PDB@0x200={pdb:#010x}:{pdb_hi:#010x} PT[112]={pt112_lo:#010x}:{pt112_hi:#010x}"
    ));

    // ── Step 3: Minimal SEC2 preparation (NO bind, NO double reset) ──
    // Exp 094 proved that bind_inst writes and double ENGCTL cycles before
    // STARTCPU prevent the BL from executing. For physical DMA (ctx_dma=PHYS),
    // no instance block binding is needed — FBIF=0x91 routes DMA directly to VRAM.
    //
    // Match Strategy 1's flow (which successfully executes BL):
    // engine_reset → FBIF → BL_upload → BOOTVEC → STARTCPU
    let (reset_ok, reset_notes) = sec2_prepare_physical_first(bar0);
    for n in &reset_notes {
        notes.push(n.clone());
    }
    notes.push(format!("SEC2 engine reset: ok={reset_ok}"));

    // Set FBIF for physical VRAM access AFTER reset (reset sets FBIF=0x190)
    let fbif_target = falcon::FBIF_TARGET_PHYS_VID | falcon::FBIF_PHYSICAL_OVERRIDE | 0x10;
    w(falcon::FBIF_TRANSCFG, fbif_target);
    let fbif_after = r(falcon::FBIF_TRANSCFG);
    notes.push(format!(
        "FBIF_TRANSCFG: target={fbif_target:#010x} now={fbif_after:#010x}"
    ));

    // ── Step 5: Load BL code → IMEM ──
    let hwcfg = r(falcon::HWCFG);
    let code_limit = falcon::imem_size_bytes(hwcfg);
    let boot_size =
        ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;

    falcon_imem_upload_nouveau(bar0, base, imem_addr, &parsed.bl_code, start_tag);
    notes.push(format!(
        "BL code: {}B → IMEM@{imem_addr:#x} tag={start_tag:#x} boot_addr={boot_addr:#x}",
        parsed.bl_code.len()
    ));

    // Verify IMEM upload at boot_addr (first 4 words)
    w(falcon::IMEMC, (1u32 << 25) | imem_addr); // read mode
    let mut imem_verify = [0u32; 4];
    for word in &mut imem_verify {
        *word = r(falcon::IMEMD);
    }
    let bl_first_word = if parsed.bl_code.len() >= 4 {
        u32::from_le_bytes([
            parsed.bl_code[0],
            parsed.bl_code[1],
            parsed.bl_code[2],
            parsed.bl_code[3],
        ])
    } else {
        0
    };
    let imem_ok = imem_verify[0] == bl_first_word;
    notes.push(format!(
        "IMEM verify @{imem_addr:#x}: [{:#010x} {:#010x} {:#010x} {:#010x}] expected[0]={bl_first_word:#010x} ok={imem_ok}",
        imem_verify[0], imem_verify[1], imem_verify[2], imem_verify[3]
    ));

    // ── Step 5b: Load BL data section → EMEM ──
    // Critical: Strategy 1/1b both upload the BL's own data section to EMEM.
    // SEC2's bootloader on gp102+ reads initialization data from EMEM; without
    // it the BL cannot bootstrap (Exp 094 discovery: empty TRACEPC = BL never
    // initialized because EMEM was empty).
    let bl_payload = fw.acr_bl_parsed.payload(&fw.acr_bl_raw);
    let bl_data_off = parsed.bl_desc.bl_data_off as usize;
    let bl_data_size = parsed.bl_desc.bl_data_size as usize;
    let bl_data_end = (bl_data_off + bl_data_size).min(bl_payload.len());
    if bl_data_off < bl_payload.len() && bl_data_size > 0 {
        let bl_data = &bl_payload[bl_data_off..bl_data_end];
        sec2_emem_write(bar0, 0, bl_data);
        notes.push(format!(
            "BL data: {}B → EMEM@0 (from bl.bin [{bl_data_off:#x}..{bl_data_end:#x}])",
            bl_data.len()
        ));
    } else {
        notes.push(format!(
            "BL data: SKIP (bl_data_off={bl_data_off:#x} bl_data_size={bl_data_size:#x} payload={}B)",
            bl_payload.len()
        ));
    }

    // ── Step 6: Pre-load ACR data section + BL descriptor to DMEM ──
    // The BL's data xcld (from VRAM) may fail. To ensure the ACR finds its
    // flcn_acr_desc_v1 in DMEM, we pre-load the patched data section first,
    // then overlay the BL descriptor on top. The BL descriptor (76 bytes)
    // falls entirely within the reserved_dmem[512] area that the ACR ignores.
    let data_section = &payload_patched[parsed.load_header.data_dma_base as usize..];
    falcon_dmem_upload(bar0, base, 0, data_section);
    notes.push(format!(
        "ACR data pre-loaded: {}B → DMEM@0 (includes patched desc at 0x210+)",
        data_section.len()
    ));

    let code_dma_base = vram_base as u64;
    let data_dma_base = vram_base as u64 + parsed.load_header.data_dma_base as u64;
    let mut bl_desc = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    // Override ctx_dma from VIRT(4) to PHYS(0) for VRAM physical DMA path.
    // DMACTL only enables index 0 in LS mode; the BL must DMA through index 0.
    bl_desc[32..36].copy_from_slice(&0u32.to_le_bytes());
    let dmem_load_off = parsed.bl_desc.bl_desc_dmem_load_off;
    falcon_dmem_upload(bar0, base, dmem_load_off, &bl_desc);
    notes.push(format!(
        "BL desc: {}B → DMEM@{dmem_load_off:#x} (code={code_dma_base:#x} data={data_dma_base:#x} ctx_dma=PHYS)",
        bl_desc.len()
    ));

    // ── Step 7: Boot SEC2 ──
    // Clear any leftover ROM exception before STARTCPU
    let exci_pre = r(falcon::EXCI);
    w(falcon::EXCI, 0);
    notes.push(format!("EXCI cleared: was={exci_pre:#010x}"));

    // Match nouveau's sentinel (BL checks MAILBOX0 for host-ready signal)
    w(falcon::MAILBOX0, 0xcafe_beef_u32);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, boot_addr);

    // Verify BOOTVEC readback
    let bootvec_rb = r(falcon::BOOTVEC);
    let cpuctl_pre = r(falcon::CPUCTL);
    let sctl_pre = r(falcon::SCTL);
    notes.push(format!(
        "Pre-start: BOOTVEC={bootvec_rb:#x}(expected {boot_addr:#x}) cpuctl={cpuctl_pre:#010x} sctl={sctl_pre:#010x} mb0=0xcafebeef"
    ));

    falcon_start_cpu(bar0, base);

    // ── Step 7b: Maintain FBIF during BL execution ──
    // Re-apply FBIF in case BL or ROM clears PHYS_VID bit during startup.
    for _ in 0..100 {
        w(falcon::FBIF_TRANSCFG, fbif_target);
        std::thread::sleep(std::time::Duration::from_micros(10));
    }
    let fbif_post_start = r(falcon::FBIF_TRANSCFG);
    let dmactl_post_start = r(falcon::DMACTL);
    let mb0_post_start = r(falcon::MAILBOX0);
    notes.push(format!(
        "Post-boot: FBIF={fbif_post_start:#010x} DMACTL={dmactl_post_start:#010x} mb0={mb0_post_start:#010x}"
    ));
    // If mb0 changed from 0xcafebeef, BL executed and wrote a status
    if mb0_post_start != 0xcafe_beef && mb0_post_start != 0 {
        notes.push(format!("BL MAILBOX RESPONSE: {mb0_post_start:#010x} (BL executed!)"));
    }

    // ── Step 8: Poll with PC sampling ──
    let timeout = std::time::Duration::from_secs(5);
    let start_time = std::time::Instant::now();
    let mut pc_samples = Vec::new();
    let mut last_pc = 0u32;
    // Phase 1: Aggressive tracing (100μs intervals) to catch execution flow
    let mut all_pcs: Vec<u32> = Vec::new();
    for _ in 0..500 {
        let pc = r(falcon::PC);
        if all_pcs.last() != Some(&pc) {
            all_pcs.push(pc);
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
        if start_time.elapsed().as_millis() > 50 {
            break;
        }
    }

    // Phase 2: Normal settling (5ms intervals)
    let mut settled_count = 0u32;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let mb1 = r(falcon::MAILBOX1);
        let pc = r(falcon::PC);

        if pc != last_pc {
            pc_samples.push(format!(
                "{:#06x}@{}ms",
                pc,
                start_time.elapsed().as_millis()
            ));
            last_pc = pc;
            settled_count = 0;
        } else {
            settled_count += 1;
        }

        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;
        let hreset_back = cpuctl & falcon::CPUCTL_HRESET != 0;
        let mb0_changed = mb0 != 0xcafe_beef && mb0 != 0;

        if mb0_changed || halted || hreset_back {
            notes.push(format!(
                "SEC2 response: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} pc={pc:#010x} ({}ms)",
                start_time.elapsed().as_millis()
            ));
            break;
        }
        if settled_count > 200 {
            notes.push(format!(
                "SEC2 settled at pc={pc:#010x} cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} ({}ms)",
                start_time.elapsed().as_millis()
            ));
            break;
        }
        if start_time.elapsed() > timeout {
            notes.push(format!(
                "SEC2 timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} pc={pc:#010x}"
            ));
            break;
        }
    }
    if !all_pcs.is_empty() {
        let pc_trace: Vec<String> = all_pcs.iter().map(|p| format!("{p:#06x}")).collect();
        notes.push(format!("Fast PC trace (100μs): [{}]", pc_trace.join(" → ")));
    }
    if !pc_samples.is_empty() {
        notes.push(format!("PC progression: [{}]", pc_samples.join(", ")));
    }

    // ── DMEM diagnostic: read ACR-relevant regions ──
    // First 256B: BL descriptor area
    let dmem_lo = sec2_dmem_read(bar0, 0, 256);
    let lo_vals: Vec<String> = dmem_lo
        .iter()
        .enumerate()
        .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", i * 4))
        .collect();
    if !lo_vals.is_empty() {
        notes.push(format!("DMEM[0..0x100]: {}", lo_vals.join(" ")));
    }
    // ACR descriptor region: 0x200-0x270 (signatures + wpr_region_id + regions)
    let dmem_acr = sec2_dmem_read(bar0, 0x200, 0x70);
    let acr_vals: Vec<String> = dmem_acr
        .iter()
        .enumerate()
        .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", 0x200 + i * 4))
        .collect();
    notes.push(format!(
        "DMEM[0x200..0x270]: {}",
        if acr_vals.is_empty() {
            "ALL ZERO".to_string()
        } else {
            acr_vals.join(" ")
        }
    ));

    // DMA config and transfer registers after ACR settles
    let dma_fbif = r(falcon::FBIF_TRANSCFG);
    let dma_10c = r(falcon::DMACTL);
    let dma_bind = r(0x054); // bind_inst
    let dmatrfbase = r(0x110);
    let dmatrfmoffs = r(0x114);
    let dmatrfcmd = r(0x118);
    let dmatrffboffs = r(0x11C);
    notes.push(format!(
        "DMA: FBIF={dma_fbif:#010x} DMACTL={dma_10c:#010x} bind_inst={dma_bind:#010x}"
    ));
    notes.push(format!(
        "DMA xfer: base={dmatrfbase:#010x} moffs={dmatrfmoffs:#010x} cmd={dmatrfcmd:#010x} fboffs={dmatrffboffs:#010x}"
    ));
    // Check EXCI (exception info) for any trapped errors
    let exci = r(falcon::EXCI);
    let tracepc2 = r(0x034); // TRACEPC[1] — previous PC sample
    let tracepc3 = r(0x038); // TRACEPC[2]
    let tracepc4 = r(0x03C); // TRACEPC[3]
    notes.push(format!(
        "Diag: EXCI={exci:#010x} TRACEPC[0..3]=[{:#06x}, {:#06x}, {:#06x}, {:#06x}]",
        r(falcon::PC),
        tracepc2,
        tracepc3,
        tracepc4
    ));

    // Check WPR header status in VRAM — did ACR modify it?
    // wpr_header_v1[0].status is at WPR+20, [1].status at WPR+44
    if let Ok(region) = PraminRegion::new(bar0, wpr_vram_base, 64) {
        let fecs_fid = region.read_u32(0).unwrap_or(0);
        let fecs_status = region.read_u32(20).unwrap_or(0);
        let gpccs_fid = region.read_u32(24).unwrap_or(0);
        let gpccs_status = region.read_u32(44).unwrap_or(0);
        let sentinel = region.read_u32(48).unwrap_or(0);
        let status_name = |s: u32| match s {
            0 => "NONE",
            1 => "COPY",
            2 => "CODE_FAIL",
            3 => "DATA_FAIL",
            4 => "DONE",
            5 => "SKIPPED",
            6 => "READY",
            7 => "REVOKE_FAIL",
            _ => "UNKNOWN",
        };
        notes.push(format!(
            "WPR hdrs: FECS(id={fecs_fid}) status={fecs_status}({}), GPCCS(id={gpccs_fid}) status={gpccs_status}({}), sentinel={sentinel:#x}",
            status_name(fecs_status), status_name(gpccs_status)
        ));
    }

    // Read DMEM@0xB00 (non-zero region seen in all runs)
    let dmem_b00 = sec2_dmem_read(bar0, 0xB00, 0x20);
    let b00_vals: Vec<String> = dmem_b00
        .iter()
        .enumerate()
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", 0xB00 + i * 4))
        .collect();
    notes.push(format!("DMEM[0xB00..0xB20]: {}", b00_vals.join(" ")));

    // Also read DMEM around 0xD00-0xE00 for potential error codes
    let dmem_hi = sec2_dmem_read(bar0, 0xD00, 0x100);
    let hi_vals: Vec<String> = dmem_hi
        .iter()
        .enumerate()
        .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", 0xD00 + i * 4))
        .collect();
    if !hi_vals.is_empty() {
        notes.push(format!("DMEM[0xD00..0xE00]: {}", hi_vals.join(" ")));
    }

    // ── Step 9: Send BOOTSTRAP_FALCON commands ──
    // If ACR is running (not back in ROM), send bootstrap commands via mailbox.
    let sec2_pc = r(falcon::PC);
    let sec2_in_acr = sec2_pc > 0x100 && sec2_pc < 0x3000; // ACR code range

    if sec2_in_acr {
        notes.push(format!("ACR appears active at pc={sec2_pc:#x}"));

        // Scan full 64KB DMEM for non-zero regions (find queue structures)
        let hwcfg = r(falcon::HWCFG);
        let dmem_total = falcon::dmem_size_bytes(hwcfg) as usize;
        let scan_size = dmem_total.min(0x10000);
        let mut nz_ranges = Vec::new();
        let mut in_nz = false;
        let mut nz_start = 0;
        // Read in 4KB chunks to avoid timeout
        for chunk_base in (0..scan_size).step_by(4096) {
            let chunk = sec2_dmem_read(bar0, chunk_base as u32, 4096);
            for (i, &word) in chunk.iter().enumerate() {
                let addr = chunk_base + i * 4;
                if word != 0 && word != 0xDEAD_DEAD {
                    if !in_nz {
                        nz_start = addr;
                        in_nz = true;
                    }
                } else if in_nz {
                    nz_ranges.push(format!("{nz_start:#06x}..{addr:#06x}"));
                    in_nz = false;
                }
            }
        }
        if in_nz {
            nz_ranges.push(format!("{nz_start:#06x}..{scan_size:#06x}"));
        }
        notes.push(format!(
            "DMEM[0..{scan_size:#x}] non-zero: [{}]",
            nz_ranges.join(", ")
        ));

        // Read interesting high DMEM regions (potential queue headers)
        for &region_start in &[0x1000u32, 0x2000, 0x4000, 0x8000] {
            let sample = sec2_dmem_read(bar0, region_start, 32);
            let has_data = sample.iter().any(|&w| w != 0 && w != 0xDEAD_DEAD);
            if has_data {
                let vals: Vec<String> = sample
                    .iter()
                    .enumerate()
                    .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
                    .map(|(i, &w)| format!("[{:#06x}]={w:#010x}", region_start as usize + i * 4))
                    .collect();
                notes.push(format!("DMEM@{region_start:#x}: {}", vals.join(" ")));
            }
        }

        // Try BOOTSTRAP via all interrupt methods
        w(falcon::MAILBOX1, falcon_id::FECS);
        w(falcon::MAILBOX0, 1);
        // Method 1: IRQSSET bit 4 (ext interrupt to falcon)
        w(0x000, 0x10);
        std::thread::sleep(std::time::Duration::from_millis(200));
        // Method 2: IRQSSET bit 0-3 (other interrupt sources)
        w(0x000, 0x01);
        std::thread::sleep(std::time::Duration::from_millis(200));

        let pc_post = r(falcon::PC);
        let fecs_cpuctl_post = bar0
            .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
            .unwrap_or(0);
        notes.push(format!(
            "After IRQ attempts: pc={pc_post:#010x} FECS cpuctl={fecs_cpuctl_post:#010x}"
        ));

        // Check if interrupts changed PC
        if pc_post != sec2_pc {
            notes.push(format!("PC MOVED after IRQ: {sec2_pc:#x} → {pc_post:#x}"));
        }
    } else {
        notes.push(format!(
            "ACR not in code range (pc={sec2_pc:#x}), skipping bootstrap commands"
        ));
    }

    // ── SEC2 Conversation probe ──
    super::sec2_queue::probe_and_bootstrap(bar0, &mut notes);

    // Final FECS/GPCCS state
    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);
    notes.push(format!(
        "Final: FECS cpuctl={:#010x} pc={:#06x} exci={:#010x} GPCCS cpuctl={:#010x} pc={:#06x} exci={:#010x}",
        post.fecs_cpuctl, post.fecs_pc, post.fecs_exci,
        post.gpccs_cpuctl, post.gpccs_pc, post.gpccs_exci
    ));

    post.into_result(
        "VRAM-based ACR boot (PRAMIN DMA)",
        sec2_before,
        sec2_after,
        notes,
    )
}

// ── Exp 111: VRAM-Native Virtual DMA Boot ──────────────────────────────

/// Write a byte slice to VRAM via PRAMIN in 48 KiB chunks.
fn write_to_vram(bar0: &MappedBar, vram_addr: u32, data: &[u8], notes: &mut Vec<String>) -> bool {
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
                        notes.push(format!("VRAM write failed at {chunk_vram:#x}+{wo:#x}"));
                        return false;
                    }
                }
                off += chunk_size;
            }
            Err(e) => {
                notes.push(format!("PRAMIN at VRAM@{chunk_vram:#x}: {e}"));
                return false;
            }
        }
    }
    true
}

/// Exp 111: VRAM-native page tables with virtual DMA via instance block bind.
///
/// Addresses the HS+MMU paradox from Exp 110: legacy PDEs give HS (via VRAM
/// physical fallback) but break DMA; correct PDEs give working DMA but route
/// code to sysmem, failing HS auth.
///
/// This strategy places EVERYTHING in VRAM:
/// - Page table chain (PD3→PT0) in VRAM with correct upper-8-byte PDEs
/// - All PTEs identity-map VRAM (not sysmem)
/// - Instance block in VRAM
/// - ACR payload + WPR in VRAM
/// - Bind target = VRAM
/// - BL descriptor uses ctx_dma=VIRT (virtual DMA through page tables)
///
/// Theory: MMU walker follows correct VRAM PDEs → VRAM PTEs → code DMA
/// from VRAM → HS auth succeeds. Post-auth DMA also resolves correctly.
pub fn attempt_vram_native_acr_boot(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    skip_blob_dma: bool,
) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    notes.push(format!(
        "Exp 111 VRAM-native: skip_blob_dma={skip_blob_dma}"
    ));
    notes.push(format!("SEC2 state: {:?}", sec2_before.state));

    // ── Step 0: VRAM accessibility ──
    let vram_ok = match PraminRegion::new(bar0, 0x5_0000, 16) {
        Ok(mut rgn) => {
            let s1 = 0xACB0_111A_u32;
            let _ = rgn.write_u32(0, s1);
            let rb = rgn.read_u32(0).unwrap_or(0);
            let ok = rb == s1;
            notes.push(format!("VRAM probe: {s1:#x}→{rb:#x} ok={ok}"));
            ok
        }
        Err(e) => {
            notes.push(format!("PRAMIN unavailable: {e}"));
            false
        }
    };
    if !vram_ok {
        return make_fail_result("VRAM-native: VRAM inaccessible", sec2_before, bar0, notes);
    }

    // MC/FBHUB diagnostic
    let fbhub0 = bar0.read_u32(0x100800).unwrap_or(0xDEAD);
    let pmc_enable = bar0.read_u32(0x000200).unwrap_or(0xDEAD);
    notes.push(format!(
        "MC: FBHUB[0]={fbhub0:#010x} PMC_EN={pmc_enable:#010x}"
    ));

    // ── Step 1: Parse firmware ──
    let parsed = match ParsedAcrFirmware::parse(fw) {
        Ok(p) => p,
        Err(e) => {
            notes.push(format!("Firmware parse failed: {e}"));
            return make_fail_result("VRAM-native: parse failed", sec2_before, bar0, notes);
        }
    };
    let acr_payload = &parsed.acr_payload;
    let data_off = parsed.load_header.data_dma_base as usize;
    notes.push(format!(
        "ACR payload: {}B non_sec=[{:#x}+{:#x}] data=[{:#x}+{:#x}]",
        acr_payload.len(),
        parsed.load_header.non_sec_code_off,
        parsed.load_header.non_sec_code_size,
        parsed.load_header.data_dma_base,
        parsed.load_header.data_size,
    ));

    // ── Step 2: Write ACR payload to VRAM (0x50000) ──
    let vram_acr: u32 = 0x0005_0000;
    let mut payload_patched = acr_payload.to_vec();

    // ── Step 2a: Build WPR ──
    let vram_wpr: u32 = 0x0007_0000;
    let wpr_data = build_wpr(fw, vram_wpr as u64);
    let wpr_end = vram_wpr as u64 + wpr_data.len() as u64;
    notes.push(format!(
        "WPR: {}B at VRAM@{vram_wpr:#x}..{wpr_end:#x}",
        wpr_data.len()
    ));

    // ── Step 2b: Patch ACR descriptor ──
    // Both WPR and shadow point to VRAM addresses (identity-mapped by PT0)
    let vram_shadow: u64 = 0x0006_0000;
    patch_acr_desc(
        &mut payload_patched,
        data_off,
        vram_wpr as u64,
        wpr_end,
        vram_shadow,
    );

    if skip_blob_dma {
        if data_off + 0x268 <= payload_patched.len() {
            payload_patched[data_off + 0x258..data_off + 0x25C]
                .copy_from_slice(&0u32.to_le_bytes());
            payload_patched[data_off + 0x260..data_off + 0x268]
                .copy_from_slice(&0u64.to_le_bytes());
            notes.push("blob_size=0: skip ACR blob DMA".to_string());
        }
    } else {
        if data_off + 0x268 <= payload_patched.len() {
            let blob_size = u32::from_le_bytes(
                payload_patched[data_off + 0x258..data_off + 0x25C]
                    .try_into()
                    .unwrap_or([0; 4]),
            );
            notes.push(format!("blob_size={blob_size:#x} preserved"));
        }
    }

    // ── Step 2c: Write payload to VRAM ──
    if !write_to_vram(bar0, vram_acr, &payload_patched, &mut notes) {
        return make_fail_result("VRAM-native: payload write failed", sec2_before, bar0, notes);
    }
    notes.push(format!(
        "ACR payload: {}B → VRAM@{vram_acr:#x}",
        payload_patched.len()
    ));

    // ── Step 2d: Write WPR + shadow to VRAM ──
    if !write_to_vram(bar0, vram_wpr, &wpr_data, &mut notes) {
        return make_fail_result("VRAM-native: WPR write failed", sec2_before, bar0, notes);
    }
    if !write_to_vram(bar0, vram_shadow as u32, &wpr_data, &mut notes) {
        return make_fail_result("VRAM-native: shadow write failed", sec2_before, bar0, notes);
    }
    notes.push(format!(
        "WPR: {}B → VRAM@{vram_wpr:#x}, shadow → VRAM@{vram_shadow:#x}",
        wpr_data.len()
    ));

    // Verify
    if let Ok(rgn) = PraminRegion::new(bar0, vram_acr, 8) {
        let v0 = rgn.read_u32(0).unwrap_or(0xDEAD);
        let e0 = u32::from_le_bytes(payload_patched[0..4].try_into().unwrap_or([0; 4]));
        notes.push(format!(
            "VRAM ACR verify: {v0:#010x} expect {e0:#010x} ok={}",
            v0 == e0
        ));
    }

    // ── Step 3: Build VRAM page tables + instance block ──
    // Uses build_vram_falcon_inst_block which:
    //   - Zeros all PT pages (prevents stale walker)
    //   - PD3→PD2→PD1→PD0→PT0 in VRAM with correct upper-8-byte PDEs
    //   - PT0 identity-maps 512 pages (2 MiB) of VRAM
    //   - Instance block PDB at FALCON_INST_VRAM (0x10000)
    let inst_ok = build_vram_falcon_inst_block(bar0);
    notes.push(format!(
        "VRAM page tables: built={inst_ok} inst@{:#x}",
        FALCON_INST_VRAM
    ));

    // Verify key page table entries
    let rv = |vram_addr: u32, offset: usize| -> u32 {
        PraminRegion::new(bar0, vram_addr, offset + 4)
            .ok()
            .and_then(|r| r.read_u32(offset).ok())
            .unwrap_or(0xDEAD)
    };
    let pdb = rv(FALCON_INST_VRAM, 0x200);
    let pd3_hi = rv(0x11000, 8); // PD3 upper 8 bytes (should be PDE)
    let pd3_lo = rv(0x11000, 0); // PD3 lower 8 bytes (should be 0)
    let pt_acr = rv(FALCON_PT0_VRAM, (vram_acr as usize / 4096) * 8); // PTE for ACR page
    notes.push(format!(
        "PT verify: PDB={pdb:#010x} PD3[lo]={pd3_lo:#010x} PD3[hi]={pd3_hi:#010x} PT[ACR@{:#x}]={pt_acr:#010x}",
        vram_acr / 4096
    ));

    // ── Step 4: Nouveau-style SEC2 reset ──
    // Disable → PMC reset → ENGCTL pulse → enable → scrub wait
    w(falcon::ITFEN, r(falcon::ITFEN) & !0x03);
    w(falcon::IRQMCLR, 0xFFFF_FFFF);
    {
        let sec2_bit = find_sec2_pmc_bit(bar0).unwrap_or(22);
        let sec2_mask = 1u32 << sec2_bit;
        let val = bar0.read_u32(misc::PMC_ENABLE).unwrap_or(0);
        if val & sec2_mask != 0 {
            let _ = bar0.write_u32(misc::PMC_ENABLE, val & !sec2_mask);
            let _ = bar0.read_u32(misc::PMC_ENABLE);
            std::thread::sleep(std::time::Duration::from_micros(20));
        }
    }
    w(falcon::ENGCTL, 0x01);
    std::thread::sleep(std::time::Duration::from_micros(10));
    w(falcon::ENGCTL, 0x00);

    if let Err(e) = pmc_enable_sec2(bar0) {
        notes.push(format!("PMC enable failed: {e}"));
    }
    let scrub_start = std::time::Instant::now();
    loop {
        let scrub = r(falcon::DMACTL);
        if scrub & 0x06 == 0 {
            break;
        }
        if scrub_start.elapsed() > std::time::Duration::from_millis(100) {
            notes.push(format!("Scrub timeout: DMACTL={scrub:#010x}"));
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    let boot0 = bar0.read_u32(misc::BOOT0).unwrap_or(0);
    w(0x084, boot0);

    let cpuctl_post = r(falcon::CPUCTL);
    let sctl_post = r(falcon::SCTL);
    notes.push(format!(
        "Post-reset: cpuctl={cpuctl_post:#010x} sctl={sctl_post:#010x}"
    ));

    // ── Step 5: Bind VRAM instance block ──
    // Enable ITFEN for DMA, then bind to VRAM instance block
    let itfen = r(falcon::ITFEN);
    w(falcon::ITFEN, (itfen & !0x01) | 0x01);
    notes.push(format!("ITFEN: {itfen:#010x} → {:#010x}", r(falcon::ITFEN)));

    let bind_val = encode_bind_inst(FALCON_INST_VRAM as u64, 0); // target=VRAM
    notes.push(format!(
        "Binding: target=VRAM addr={:#x} bind_val={bind_val:#010x}",
        FALCON_INST_VRAM
    ));
    let (bind_ok, bind_notes) =
        falcon_bind_context(&|off| r(off), &|off, val| w(off, val), bind_val);
    for n in &bind_notes {
        notes.push(n.clone());
    }
    notes.push(format!(
        "bind_stat: {} (0x0dc={:#010x})",
        if bind_ok { "OK" } else { "TIMEOUT" },
        r(0x0dc)
    ));

    // TLB invalidate against the VRAM PDB
    {
        let pdb_addr = FALCON_INST_VRAM as u64;
        let pdb_inv = ((pdb_addr >> 12) << 4) as u32;
        let _ = bar0.write_u32(0x100CB8, pdb_inv);
        let _ = bar0.write_u32(0x100CEC, 0);
        let _ = bar0.write_u32(0x100CBC, 0x8000_0005);

        let mut flush_ack = false;
        for _ in 0..200 {
            if bar0.read_u32(0x100C80).unwrap_or(0) & 0x0000_8000 != 0 {
                flush_ack = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
        notes.push(format!("TLB invalidate: PDB={pdb_inv:#010x} ack={flush_ack}"));
    }

    // ── Step 6: Load BL code → IMEM ──
    let hwcfg = r(falcon::HWCFG);
    let code_limit = falcon::imem_size_bytes(hwcfg);
    let boot_size =
        ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;

    falcon_imem_upload_nouveau(bar0, base, imem_addr, &parsed.bl_code, start_tag);
    notes.push(format!(
        "BL code: {}B → IMEM@{imem_addr:#x} tag={start_tag:#x} boot={boot_addr:#x}",
        parsed.bl_code.len()
    ));

    // ── Step 6b: BL data → EMEM ──
    let bl_payload = fw.acr_bl_parsed.payload(&fw.acr_bl_raw);
    let bl_data_off = parsed.bl_desc.bl_data_off as usize;
    let bl_data_size = parsed.bl_desc.bl_data_size as usize;
    let bl_data_end = (bl_data_off + bl_data_size).min(bl_payload.len());
    if bl_data_off < bl_payload.len() && bl_data_size > 0 {
        let bl_data = &bl_payload[bl_data_off..bl_data_end];
        sec2_emem_write(bar0, 0, bl_data);
        notes.push(format!("BL data: {}B → EMEM@0", bl_data.len()));
    }

    // ── Step 7: Pre-load ACR data to DMEM + BL descriptor ──
    // code_dma_base and data_dma_base are VRAM virtual addresses
    // (which equal VRAM physical since PT0 is identity-mapped)
    let code_dma_base = vram_acr as u64;
    let data_dma_base = vram_acr as u64 + data_off as u64;

    let data_section = &payload_patched[data_off..];
    falcon_dmem_upload(bar0, base, 0, data_section);
    notes.push(format!(
        "ACR data pre-loaded: {}B → DMEM@0",
        data_section.len()
    ));

    let bl_desc = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    let ctx_dma_val = u32::from_le_bytes(bl_desc[32..36].try_into().unwrap_or([1, 0, 0, 0]));
    falcon_dmem_upload(bar0, base, 0, &bl_desc);
    notes.push(format!(
        "BL desc: code={code_dma_base:#x} data={data_dma_base:#x} ctx_dma={ctx_dma_val} (VIRT)"
    ));

    // ── Step 8: Boot SEC2 ──
    w(falcon::EXCI, 0);
    w(falcon::MAILBOX0, 0xdead_a5a5_u32);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, boot_addr);

    {
        let fbif_pre = r(falcon::FBIF_TRANSCFG);
        let itfen_pre = r(falcon::ITFEN);
        let dmactl_pre = r(falcon::DMACTL);
        notes.push(format!(
            "Pre-boot: BOOTVEC={boot_addr:#x} FBIF={fbif_pre:#x} ITFEN={itfen_pre:#x} DMACTL={dmactl_pre:#x}"
        ));
    }

    falcon_start_cpu(bar0, base);

    // ── Step 9: Poll with PC sampling ──
    let timeout = std::time::Duration::from_secs(5);
    let start_time = std::time::Instant::now();
    let mut all_pcs: Vec<u32> = Vec::new();
    for _ in 0..500 {
        let pc = r(falcon::PC);
        if all_pcs.last() != Some(&pc) {
            all_pcs.push(pc);
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
        if start_time.elapsed().as_millis() > 50 {
            break;
        }
    }

    let mut pc_samples = Vec::new();
    let mut last_pc = 0u32;
    let mut settled_count = 0u32;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let mb1 = r(falcon::MAILBOX1);
        let pc = r(falcon::PC);

        if pc != last_pc {
            pc_samples.push(format!(
                "{:#06x}@{}ms",
                pc,
                start_time.elapsed().as_millis()
            ));
            last_pc = pc;
            settled_count = 0;
        } else {
            settled_count += 1;
        }

        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;
        let hreset_back = cpuctl & falcon::CPUCTL_HRESET != 0;

        if mb0 != 0 || halted || hreset_back {
            notes.push(format!(
                "SEC2 response: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} pc={pc:#010x} ({}ms)",
                start_time.elapsed().as_millis()
            ));
            break;
        }
        if settled_count > 200 {
            notes.push(format!(
                "SEC2 settled: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#010x} ({}ms)",
                start_time.elapsed().as_millis()
            ));
            break;
        }
        if start_time.elapsed() > timeout {
            notes.push(format!(
                "SEC2 timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#010x}"
            ));
            break;
        }
    }

    if !all_pcs.is_empty() {
        let pc_trace: Vec<String> = all_pcs.iter().map(|p| format!("{p:#06x}")).collect();
        notes.push(format!("Fast PC trace: [{}]", pc_trace.join(" → ")));
    }
    if !pc_samples.is_empty() {
        notes.push(format!("PC progression: [{}]", pc_samples.join(", ")));
    }

    // ── Step 10: Diagnostics ──
    super::boot_diagnostics::capture_post_boot_diagnostics(bar0, base, &mut notes);

    // WPR status in VRAM
    if let Ok(region) = PraminRegion::new(bar0, vram_wpr, 64) {
        let fecs_fid = region.read_u32(0).unwrap_or(0);
        let fecs_status = region.read_u32(20).unwrap_or(0);
        let gpccs_fid = region.read_u32(24).unwrap_or(0);
        let gpccs_status = region.read_u32(44).unwrap_or(0);
        let status_name = |s: u32| match s {
            0 => "NONE",
            1 => "COPY",
            4 => "DONE",
            _ => "OTHER",
        };
        notes.push(format!(
            "WPR: FECS(id={fecs_fid}) status={fecs_status}({}) GPCCS(id={gpccs_fid}) status={gpccs_status}({})",
            status_name(fecs_status), status_name(gpccs_status)
        ));
    }

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);
    notes.push(format!(
        "Final: FECS cpuctl={:#010x} pc={:#06x} exci={:#010x} GPCCS cpuctl={:#010x} pc={:#06x} exci={:#010x}",
        post.fecs_cpuctl, post.fecs_pc, post.fecs_exci,
        post.gpccs_cpuctl, post.gpccs_pc, post.gpccs_exci
    ));

    let sctl = r(falcon::SCTL);
    let hs = sctl & 0x0000_0002 != 0;
    notes.push(format!(
        "*** SCTL={sctl:#010x} HS={hs} ***"
    ));

    post.into_result(
        "VRAM-native ACR boot (Exp 111)",
        sec2_before,
        sec2_after,
        notes,
    )
}

// ── Exp 112: Dual-Phase Boot ───────────────────────────────────────────

/// Build VRAM page tables with legacy PDEs (lower 8-byte slot).
/// Same as `build_vram_falcon_inst_block` but PDEs go in the WRONG slot
/// to trigger MMU physical fallback → HS authentication.
fn build_vram_legacy_pde_tables(bar0: &MappedBar) -> bool {
    let wv = |vram_addr: u32, offset: usize, val: u32| -> bool {
        match PraminRegion::new(bar0, vram_addr, offset + 4) {
            Ok(mut region) => region.write_u32(offset, val).is_ok(),
            Err(_) => false,
        }
    };
    let wv64 = |vram_addr: u32, offset: usize, val: u64| -> bool {
        let lo = (val & 0xFFFF_FFFF) as u32;
        let hi = (val >> 32) as u32;
        wv(vram_addr, offset, lo) && wv(vram_addr, offset + 4, hi)
    };

    // Zero all pages first
    for &page in &[
        FALCON_INST_VRAM, FALCON_PD3_VRAM, FALCON_PD2_VRAM,
        FALCON_PD1_VRAM, FALCON_PD0_VRAM, FALCON_PT0_VRAM,
    ] {
        for off in (0..0x1000).step_by(4) {
            if !wv(page, off, 0) {
                return false;
            }
        }
    }

    // Legacy PDE format: pointer in LOWER 8 bytes, upper 8 bytes = 0
    // This triggers MMU physical fallback → HS authentication
    if !wv64(FALCON_PD3_VRAM, 0, encode_vram_pde(FALCON_PD2_VRAM as u64)) { return false; }
    if !wv64(FALCON_PD3_VRAM, 8, 0) { return false; }

    if !wv64(FALCON_PD2_VRAM, 0, encode_vram_pde(FALCON_PD1_VRAM as u64)) { return false; }
    if !wv64(FALCON_PD2_VRAM, 8, 0) { return false; }

    if !wv64(FALCON_PD1_VRAM, 0, encode_vram_pde(FALCON_PD0_VRAM as u64)) { return false; }
    if !wv64(FALCON_PD1_VRAM, 8, 0) { return false; }

    if !wv64(FALCON_PD0_VRAM, 0, encode_vram_pde(FALCON_PT0_VRAM as u64)) { return false; }
    if !wv64(FALCON_PD0_VRAM, 8, 0) { return false; }

    // PT0: identity-map 512 small pages (2 MiB)
    for i in 0u64..512 {
        let phys = i * 4096;
        if !wv64(FALCON_PT0_VRAM, (i as usize) * 8, encode_vram_pte(phys)) {
            return false;
        }
    }

    // Instance block: PDB at RAMIN offset 0x200
    let pdb_lo: u32 = ((FALCON_PD3_VRAM >> 12) << 12)
        | (1 << 11) // BIG_PAGE_SIZE = 64KiB
        | (1 << 10) // USE_VER2_PT_FORMAT
        ; // target bits[1:0] = 0 = VRAM, VOL=0
    if !wv(FALCON_INST_VRAM, 0x200, pdb_lo) { return false; }
    if !wv(FALCON_INST_VRAM, 0x204, 0) { return false; }
    if !wv(FALCON_INST_VRAM, 0x208, 0xFFFF_FFFF) { return false; }
    if !wv(FALCON_INST_VRAM, 0x20C, 0x0001_FFFF) { return false; }

    true
}

/// Hot-swap PDEs from legacy (lower 8-byte) to correct (upper 8-byte) format.
/// Called after HS authentication to enable correct virtual DMA.
fn hotswap_pdes_to_correct(bar0: &MappedBar) -> bool {
    let wv64 = |vram_addr: u32, offset: usize, val: u64| -> bool {
        let lo = (val & 0xFFFF_FFFF) as u32;
        let hi = (val >> 32) as u32;
        match PraminRegion::new(bar0, vram_addr, offset + 8) {
            Ok(mut region) => {
                region.write_u32(offset, lo).is_ok()
                    && region.write_u32(offset + 4, hi).is_ok()
            }
            Err(_) => false,
        }
    };

    // Write correct PDEs (upper 8 bytes) and zero legacy (lower 8 bytes)
    let ok = wv64(FALCON_PD3_VRAM, 0, 0)
        && wv64(FALCON_PD3_VRAM, 8, encode_vram_pde(FALCON_PD2_VRAM as u64))
        && wv64(FALCON_PD2_VRAM, 0, 0)
        && wv64(FALCON_PD2_VRAM, 8, encode_vram_pde(FALCON_PD1_VRAM as u64))
        && wv64(FALCON_PD1_VRAM, 0, 0)
        && wv64(FALCON_PD1_VRAM, 8, encode_vram_pde(FALCON_PD0_VRAM as u64))
        && wv64(FALCON_PD0_VRAM, 0, 0)
        && wv64(FALCON_PD0_VRAM, 8, encode_vram_pde(FALCON_PT0_VRAM as u64));

    // TLB invalidate
    if ok {
        let pdb_addr = FALCON_INST_VRAM as u64;
        let pdb_inv = ((pdb_addr >> 12) << 4) as u32;
        let _ = bar0.write_u32(0x100CB8, pdb_inv);
        let _ = bar0.write_u32(0x100CEC, 0);
        let _ = bar0.write_u32(0x100CBC, 0x8000_0005);
    }

    ok
}

/// Configuration for dual-phase boot experiments.
pub struct DualPhaseConfig {
    /// If true, skip the PDE hot-swap (stay on legacy PDEs throughout).
    pub skip_hotswap: bool,
    /// If true, set blob_size=0 in the ACR descriptor (skip blob DMA).
    pub skip_blob_dma: bool,
    /// Microseconds to wait before hot-swapping PDEs (0 = immediate).
    pub hotswap_delay_us: u64,
    /// If true, attempt to set WPR2 hardware boundaries before boot.
    pub set_wpr2: bool,
}

impl Default for DualPhaseConfig {
    fn default() -> Self {
        Self {
            skip_hotswap: false,
            skip_blob_dma: false,
            hotswap_delay_us: 0,
            set_wpr2: false,
        }
    }
}

impl std::fmt::Display for DualPhaseConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "hotswap={} blob={} delay={}µs wpr2={}",
            if self.skip_hotswap { "OFF" } else { "ON" },
            if self.skip_blob_dma { "skip" } else { "full" },
            self.hotswap_delay_us,
            if self.set_wpr2 { "SET" } else { "off" },
        )
    }
}

/// Exp 112+: Dual-phase boot — legacy PDEs for HS auth, hot-swap for DMA.
///
/// Phase 1: Build VRAM page tables with legacy PDEs (lower 8-byte slot)
///          → MMU physical fallback → HS authentication succeeds
/// Phase 2: Hot-swap PDEs to correct format (upper 8-byte) via PRAMIN
///          → Post-auth DMA uses correct virtual path through VRAM PTs
pub fn attempt_dual_phase_boot(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
) -> AcrBootResult {
    attempt_dual_phase_boot_cfg(bar0, fw, &DualPhaseConfig::default())
}

/// Configurable dual-phase boot (Exp 113 variants).
pub fn attempt_dual_phase_boot_cfg(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    cfg: &DualPhaseConfig,
) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    notes.push(format!("Dual-Phase Boot [{cfg}]"));

    // ── Step 0: VRAM check ──
    let vram_ok = match PraminRegion::new(bar0, 0x5_0000, 8) {
        Ok(mut rgn) => {
            let s = 0xACB0_112A_u32;
            let _ = rgn.write_u32(0, s);
            rgn.read_u32(0).unwrap_or(0) == s
        }
        Err(_) => false,
    };
    if !vram_ok {
        return make_fail_result("Dual-phase: VRAM inaccessible", sec2_before, bar0, notes);
    }
    notes.push("VRAM: accessible".to_string());

    // ── Step 1: Parse firmware ──
    let parsed = match ParsedAcrFirmware::parse(fw) {
        Ok(p) => p,
        Err(e) => {
            notes.push(format!("Firmware parse failed: {e}"));
            return make_fail_result("Dual-phase: parse failed", sec2_before, bar0, notes);
        }
    };
    let data_off = parsed.load_header.data_dma_base as usize;

    // ── Step 2: Write payload + WPR to VRAM ──
    let vram_acr: u32 = 0x0005_0000;
    let vram_wpr: u32 = 0x0007_0000;
    let vram_shadow: u64 = 0x0006_0000;

    let mut payload_patched = parsed.acr_payload.to_vec();
    let wpr_data = build_wpr(fw, vram_wpr as u64);
    let wpr_end = vram_wpr as u64 + wpr_data.len() as u64;

    patch_acr_desc(
        &mut payload_patched, data_off,
        vram_wpr as u64, wpr_end, vram_shadow,
    );
    if cfg.skip_blob_dma {
        if data_off + 0x268 <= payload_patched.len() {
            payload_patched[data_off + 0x258..data_off + 0x25C]
                .copy_from_slice(&0u32.to_le_bytes());
            payload_patched[data_off + 0x260..data_off + 0x268]
                .copy_from_slice(&0u64.to_le_bytes());
            notes.push("blob_size=0 (skip blob DMA)".to_string());
        }
    } else if data_off + 0x268 <= payload_patched.len() {
        let blob_size = u32::from_le_bytes(
            payload_patched[data_off + 0x258..data_off + 0x25C].try_into().unwrap_or([0; 4]),
        );
        notes.push(format!("blob_size={blob_size:#x} (preserved for full init)"));
    }

    if !write_to_vram(bar0, vram_acr, &payload_patched, &mut notes) {
        return make_fail_result("Dual-phase: payload write failed", sec2_before, bar0, notes);
    }
    if !write_to_vram(bar0, vram_wpr, &wpr_data, &mut notes) {
        return make_fail_result("Dual-phase: WPR write failed", sec2_before, bar0, notes);
    }
    if !write_to_vram(bar0, vram_shadow as u32, &wpr_data, &mut notes) {
        return make_fail_result("Dual-phase: shadow write failed", sec2_before, bar0, notes);
    }
    notes.push(format!(
        "VRAM data: ACR@{vram_acr:#x} WPR@{vram_wpr:#x} shadow@{vram_shadow:#x}"
    ));

    // ── Step 3: Build VRAM page tables with LEGACY PDEs ──
    let pt_ok = build_vram_legacy_pde_tables(bar0);
    notes.push(format!("Legacy PDE page tables: built={pt_ok}"));

    // Verify: PDE should be in LOWER 8 bytes, upper should be 0
    let rv = |vram_addr: u32, offset: usize| -> u32 {
        PraminRegion::new(bar0, vram_addr, offset + 4)
            .ok()
            .and_then(|r| r.read_u32(offset).ok())
            .unwrap_or(0xDEAD)
    };
    let pd3_lo = rv(FALCON_PD3_VRAM, 0);
    let pd3_hi = rv(FALCON_PD3_VRAM, 8);
    notes.push(format!(
        "PD3 verify: lower={pd3_lo:#010x} upper={pd3_hi:#010x} (expect: lower=PDE, upper=0)"
    ));

    // ── Step 3b: WPR2 hardware boundaries ──
    {
        let wpr1_beg = bar0.read_u32(0x100CE4).unwrap_or(0xDEAD);
        let wpr1_end = bar0.read_u32(0x100CE8).unwrap_or(0xDEAD);
        let wpr2_beg = bar0.read_u32(0x100CEC).unwrap_or(0xDEAD);
        let wpr2_end = bar0.read_u32(0x100CF0).unwrap_or(0xDEAD);
        notes.push(format!(
            "WPR hw: WPR1=[{wpr1_beg:#010x}..{wpr1_end:#010x}] WPR2=[{wpr2_beg:#010x}..{wpr2_end:#010x}]"
        ));

        // GM200 indexed register for WPR boundaries
        let _ = bar0.write_u32(0x100CD4, 2);
        let idx_lo = bar0.read_u32(0x100CD4).unwrap_or(0xDEAD);
        let _ = bar0.write_u32(0x100CD4, 3);
        let idx_hi = bar0.read_u32(0x100CD4).unwrap_or(0xDEAD);
        notes.push(format!("WPR GM200 indexed: lo={idx_lo:#010x} hi={idx_hi:#010x}"));

        if cfg.set_wpr2 {
            // Attempt to set WPR2 boundaries to cover our WPR region.
            // The ACR BL may validate these before copying authenticated images.
            let wpr_beg_val = vram_wpr as u32;
            let wpr_end_val = wpr_end as u32;

            // Direct PFB WPR registers
            let _ = bar0.write_u32(0x100CE4, wpr_beg_val);
            let _ = bar0.write_u32(0x100CE8, wpr_end_val);
            let rb1_beg = bar0.read_u32(0x100CE4).unwrap_or(0xDEAD);
            let rb1_end = bar0.read_u32(0x100CE8).unwrap_or(0xDEAD);
            notes.push(format!(
                "WPR1 set: {wpr_beg_val:#010x}→{rb1_beg:#010x} {wpr_end_val:#010x}→{rb1_end:#010x}"
            ));

            // 0x100CEC/CF0 may be TLB invalidation registers, NOT WPR2.
            // Try writing WPR bounds anyway and check readback.
            let _ = bar0.write_u32(0x100CEC, wpr_beg_val);
            let _ = bar0.write_u32(0x100CF0, wpr_end_val);
            let rb2_beg = bar0.read_u32(0x100CEC).unwrap_or(0xDEAD);
            let rb2_end = bar0.read_u32(0x100CF0).unwrap_or(0xDEAD);
            notes.push(format!(
                "WPR2 set: {wpr_beg_val:#010x}→{rb2_beg:#010x} {wpr_end_val:#010x}→{rb2_end:#010x}"
            ));

            // GM200 indexed: (addr >> 8) | enable_bit
            let gm200_lo = (wpr_beg_val >> 8) | 0x01;
            let gm200_hi = wpr_end_val >> 8;
            let _ = bar0.write_u32(0x100CD4, gm200_lo);
            std::thread::sleep(std::time::Duration::from_micros(10));
            let rb_lo = bar0.read_u32(0x100CD4).unwrap_or(0xDEAD);
            let _ = bar0.write_u32(0x100CD4, gm200_hi);
            std::thread::sleep(std::time::Duration::from_micros(10));
            let rb_hi = bar0.read_u32(0x100CD4).unwrap_or(0xDEAD);
            notes.push(format!(
                "GM200 indexed set: lo={gm200_lo:#010x}→{rb_lo:#010x} hi={gm200_hi:#010x}→{rb_hi:#010x}"
            ));
        }
    }

    // ── Step 4: SEC2 reset ──
    w(falcon::ITFEN, r(falcon::ITFEN) & !0x03);
    w(falcon::IRQMCLR, 0xFFFF_FFFF);
    {
        let sec2_bit = find_sec2_pmc_bit(bar0).unwrap_or(22);
        let sec2_mask = 1u32 << sec2_bit;
        let val = bar0.read_u32(misc::PMC_ENABLE).unwrap_or(0);
        if val & sec2_mask != 0 {
            let _ = bar0.write_u32(misc::PMC_ENABLE, val & !sec2_mask);
            let _ = bar0.read_u32(misc::PMC_ENABLE);
            std::thread::sleep(std::time::Duration::from_micros(20));
        }
    }
    w(falcon::ENGCTL, 0x01);
    std::thread::sleep(std::time::Duration::from_micros(10));
    w(falcon::ENGCTL, 0x00);
    if let Err(e) = pmc_enable_sec2(bar0) {
        notes.push(format!("PMC enable failed: {e}"));
    }
    let scrub_start = std::time::Instant::now();
    loop {
        let scrub = r(falcon::DMACTL);
        if scrub & 0x06 == 0 { break; }
        if scrub_start.elapsed() > std::time::Duration::from_millis(100) { break; }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }
    let boot0 = bar0.read_u32(misc::BOOT0).unwrap_or(0);
    w(0x084, boot0);
    notes.push(format!(
        "Post-reset: cpuctl={:#010x} sctl={:#010x}",
        r(falcon::CPUCTL), r(falcon::SCTL)
    ));

    // ── Step 5: Bind VRAM instance block ──
    w(falcon::ITFEN, r(falcon::ITFEN) | 0x01);
    let bind_val = encode_bind_inst(FALCON_INST_VRAM as u64, 0);
    let (bind_ok, bind_notes) =
        falcon_bind_context(&|off| r(off), &|off, val| w(off, val), bind_val);
    for n in &bind_notes {
        notes.push(n.clone());
    }
    notes.push(format!("Bind: {} val={bind_val:#010x}", if bind_ok { "OK" } else { "TIMEOUT" }));

    // ── Step 6: Upload BL + data ──
    let hwcfg = r(falcon::HWCFG);
    let code_limit = falcon::imem_size_bytes(hwcfg);
    let boot_size =
        ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;

    falcon_imem_upload_nouveau(bar0, base, imem_addr, &parsed.bl_code, start_tag);

    let bl_payload = fw.acr_bl_parsed.payload(&fw.acr_bl_raw);
    let bl_data_off = parsed.bl_desc.bl_data_off as usize;
    let bl_data_size = parsed.bl_desc.bl_data_size as usize;
    let bl_data_end = (bl_data_off + bl_data_size).min(bl_payload.len());
    if bl_data_off < bl_payload.len() && bl_data_size > 0 {
        sec2_emem_write(bar0, 0, &bl_payload[bl_data_off..bl_data_end]);
    }

    let code_dma_base = vram_acr as u64;
    let data_dma_base = vram_acr as u64 + data_off as u64;
    let data_section = &payload_patched[data_off..];
    falcon_dmem_upload(bar0, base, 0, data_section);

    let bl_desc = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    falcon_dmem_upload(bar0, base, 0, &bl_desc);
    notes.push(format!(
        "BL: IMEM@{imem_addr:#x} boot={boot_addr:#x} code_dma={code_dma_base:#x} ctx_dma=VIRT"
    ));

    // ── Step 7: Start falcon + immediately hot-swap PDEs ──
    w(falcon::EXCI, 0);
    w(falcon::MAILBOX0, 0xdead_a5a5_u32);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, boot_addr);

    notes.push("Phase 1: Starting falcon with legacy PDEs...".to_string());
    falcon_start_cpu(bar0, base);

    if cfg.skip_hotswap {
        notes.push("Phase 2: SKIPPED (legacy PDEs throughout)".to_string());
    } else {
        if cfg.hotswap_delay_us > 0 {
            std::thread::sleep(std::time::Duration::from_micros(cfg.hotswap_delay_us));
            let delay_sctl = r(falcon::SCTL);
            let delay_pc = r(falcon::PC);
            notes.push(format!(
                "Hot-swap delay: {}µs SCTL={delay_sctl:#010x} PC={delay_pc:#06x}",
                cfg.hotswap_delay_us
            ));
        }
        let swap_ok = hotswap_pdes_to_correct(bar0);
        let swap_sctl = r(falcon::SCTL);
        let swap_pc = r(falcon::PC);
        notes.push(format!(
            "Phase 2: PDEs hot-swapped={swap_ok} SCTL={swap_sctl:#010x} PC={swap_pc:#06x}"
        ));
        std::thread::sleep(std::time::Duration::from_micros(10));
        let _ = hotswap_pdes_to_correct(bar0);
    }

    // ── Step 8: Poll with PC sampling ──
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
        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;
        let hreset = cpuctl & falcon::CPUCTL_HRESET != 0;

        if all_pcs.last() != Some(&pc) {
            all_pcs.push(pc);
            settled_count = 0;
        } else {
            settled_count += 1;
        }

        if mb0 != 0 || halted || hreset {
            notes.push(format!(
                "SEC2: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#06x} ({}ms)",
                start_time.elapsed().as_millis()
            ));
            break;
        }
        if settled_count > 200 || start_time.elapsed() > timeout {
            notes.push(format!(
                "SEC2 settled/timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#06x}",
            ));
            break;
        }
    }

    if !all_pcs.is_empty() {
        let trace: Vec<String> = all_pcs.iter().map(|p| format!("{p:#06x}")).collect();
        notes.push(format!("PC trace: [{}]", trace.join(" → ")));
    }

    // ── Step 9: Diagnostics ──
    super::boot_diagnostics::capture_post_boot_diagnostics(bar0, base, &mut notes);

    // Check WPR status
    if let Ok(region) = PraminRegion::new(bar0, vram_wpr, 64) {
        let fecs_status = region.read_u32(20).unwrap_or(0);
        let gpccs_status = region.read_u32(44).unwrap_or(0);
        notes.push(format!("WPR: FECS status={fecs_status} GPCCS status={gpccs_status}"));
    }

    let sctl = r(falcon::SCTL);
    let hs = sctl & 0x02 != 0;
    notes.push(format!("*** SCTL={sctl:#010x} HS={hs} ***"));

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);
    notes.push(format!(
        "Final: FECS cpuctl={:#010x} exci={:#010x} GPCCS cpuctl={:#010x} exci={:#010x}",
        post.fecs_cpuctl, post.fecs_exci, post.gpccs_cpuctl, post.gpccs_exci
    ));

    post.into_result("Dual-phase boot (Exp 112)", sec2_before, sec2_after, notes)
}
