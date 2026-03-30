// SPDX-License-Identifier: AGPL-3.0-only

use crate::vfio::channel::registers::{falcon, misc};
use crate::vfio::device::MappedBar;
use crate::vfio::memory::{MemoryRegion, PraminRegion};

use super::super::boot_result::{AcrBootResult, make_fail_result};
use super::super::firmware::{AcrFirmwareSet, ParsedAcrFirmware};
use super::super::instance_block::{
    FALCON_INST_VRAM, FALCON_PT0_VRAM, build_vram_falcon_inst_block, encode_bind_inst,
    falcon_bind_context,
};
use super::super::sec2_hal::{
    Sec2Probe, falcon_dmem_upload, falcon_imem_upload_nouveau, falcon_start_cpu, find_sec2_pmc_bit,
    pmc_enable_sec2, sec2_emem_write,
};
use super::super::wpr::{build_bl_dmem_desc, build_wpr, patch_acr_desc};
use super::pramin_write::write_to_vram;

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
    } else if data_off + 0x268 <= payload_patched.len() {
        let blob_size = u32::from_le_bytes(
            payload_patched[data_off + 0x258..data_off + 0x25C]
                .try_into()
                .unwrap_or([0; 4]),
        );
        notes.push(format!("blob_size={blob_size:#x} preserved"));
    }

    // ── Step 2c: Write payload to VRAM ──
    if !write_to_vram(bar0, vram_acr, &payload_patched, &mut notes) {
        return make_fail_result(
            "VRAM-native: payload write failed",
            sec2_before,
            bar0,
            notes,
        );
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
        notes.push(format!(
            "TLB invalidate: PDB={pdb_inv:#010x} ack={flush_ack}"
        ));
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

        let halted = cpuctl & falcon::CPUCTL_STOPPED != 0;
        let hreset_back = cpuctl & falcon::CPUCTL_HALTED != 0;

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
    super::super::boot_diagnostics::capture_post_boot_diagnostics(bar0, base, &mut notes);

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
    let post = super::super::boot_result::PostBootCapture::capture(bar0);
    notes.push(format!(
        "Final: FECS cpuctl={:#010x} pc={:#06x} exci={:#010x} GPCCS cpuctl={:#010x} pc={:#06x} exci={:#010x}",
        post.fecs_cpuctl, post.fecs_pc, post.fecs_exci,
        post.gpccs_cpuctl, post.gpccs_pc, post.gpccs_exci
    ));

    let sctl = r(falcon::SCTL);
    let hs = sctl & 0x0000_0002 != 0;
    notes.push(format!("*** SCTL={sctl:#010x} HS={hs} ***"));

    post.into_result(
        "VRAM-native ACR boot (Exp 111)",
        sec2_before,
        sec2_after,
        notes,
    )
}
