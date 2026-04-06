// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;
use crate::vfio::memory::{MemoryRegion, PraminRegion};

use super::super::boot_result::{AcrBootResult, make_fail_result};
use super::super::firmware::{AcrFirmwareSet, ParsedAcrFirmware};
use super::super::instance_block::{
    FALCON_INST_VRAM, FALCON_PT0_VRAM, build_vram_falcon_inst_block,
};
use super::super::sec2_hal::{
    Sec2Probe, falcon_dmem_upload, falcon_imem_upload_nouveau, falcon_start_cpu, sec2_dmem_read,
    sec2_emem_write, sec2_prepare_physical_first,
};
use super::super::wpr::{build_bl_dmem_desc, build_wpr, falcon_id, patch_acr_desc};
use super::pramin_write::write_to_vram;

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
        notes.push(format!(
            "BL MAILBOX RESPONSE: {mb0_post_start:#010x} (BL executed!)"
        ));
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
    super::super::sec2_queue::probe_and_bootstrap(bar0, &mut notes);

    // Final FECS/GPCCS state
    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::super::boot_result::PostBootCapture::capture(bar0);
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
