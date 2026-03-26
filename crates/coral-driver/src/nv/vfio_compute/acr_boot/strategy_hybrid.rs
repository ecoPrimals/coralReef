// SPDX-License-Identifier: AGPL-3.0-only

//! Hybrid ACR boot: VRAM page tables + system memory payloads.

use crate::vfio::channel::registers::{falcon, misc, mmu};
use crate::vfio::device::{DmaBackend, MappedBar};
use crate::vfio::dma::DmaBuffer;
use crate::vfio::memory::{MemoryRegion, PraminRegion};

use super::boot_result::{AcrBootResult, make_fail_result};
use super::firmware::{AcrFirmwareSet, ParsedAcrFirmware};
use super::instance_block::{
    self, FALCON_INST_VRAM, FALCON_PD0_VRAM, FALCON_PD1_VRAM, FALCON_PD2_VRAM, FALCON_PD3_VRAM,
    FALCON_PT0_VRAM, encode_sysmem_pte, encode_vram_pd0_pde, encode_vram_pde, encode_vram_pte,
};
use super::sec2_hal::{
    Sec2Probe, falcon_dmem_upload, falcon_imem_upload_nouveau, falcon_start_cpu, find_sec2_pmc_bit,
    pmc_enable_sec2, sec2_dmem_read,
};
use super::sysmem_iova;
use super::wpr::{build_bl_dmem_desc, build_wpr, patch_acr_desc};

/// Hybrid ACR boot: VRAM instance block + page tables, system memory data.
///
/// This matches Nouveau's exact architecture on GV100:
///  - Instance block: VRAM (falcon can always reach VRAM)
///  - Page directory chain (PD3→PD2→PD1→PD0): VRAM, VRAM-aperture PDEs
///  - PT0 entries: SYS_MEM_COH aperture → IOMMU-mapped DMA buffers
///  - ACR payload + WPR: system memory DMA buffers
///
/// The falcon's 0x668 binding uses VRAM target (no IOMMU needed for initial
/// lookup). The GPU MMU walks VRAM page tables. Only leaf PTEs cross to
/// system memory via the IOMMU.
pub fn attempt_hybrid_acr_boot(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    container: DmaBackend,
) -> AcrBootResult {
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
            return make_fail_result("Hybrid ACR: parse failed", sec2_before, bar0, notes);
        }
    };
    notes.push(format!(
        "ACR payload: {}B data_off={:#x}",
        parsed.acr_payload.len(),
        parsed.load_header.data_dma_base
    ));

    // ── Step 2: Allocate DMA buffers for ACR payload + WPR ──
    let acr_iova = sysmem_iova::ACR;
    let acr_payload_size = parsed.acr_payload.len().div_ceil(4096) * 4096;
    let mut acr_dma = match DmaBuffer::new(container.clone(), acr_payload_size.max(4096), acr_iova)
    {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc ACR failed: {e}"));
            return make_fail_result("Hybrid ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };

    let wpr_iova = sysmem_iova::WPR;
    let wpr_data = build_wpr(fw, wpr_iova);
    let wpr_end = wpr_iova + wpr_data.len() as u64;
    let wpr_buf_size = wpr_data.len().div_ceil(4096) * 4096;
    let mut wpr_dma = match DmaBuffer::new(container.clone(), wpr_buf_size.max(4096), wpr_iova) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc WPR failed: {e}"));
            return make_fail_result("Hybrid ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    notes.push(format!(
        "DMA: ACR@{acr_iova:#x}({acr_payload_size:#x}) WPR@{wpr_iova:#x}({wpr_buf_size:#x})"
    ));

    // Shadow WPR: separate copy for ACR verification
    let shadow_iova = sysmem_iova::SHADOW;
    let mut shadow_dma =
        match DmaBuffer::new(container.clone(), wpr_buf_size.max(4096), shadow_iova) {
            Ok(b) => b,
            Err(e) => {
                notes.push(format!("DMA alloc shadow failed: {e}"));
                return make_fail_result(
                    "Hybrid ACR: DMA alloc failed",
                    sec2_before,
                    bar0,
                    notes,
                );
            }
        };

    // ── Step 3: Populate WPR + shadow + patch ACR descriptor ──
    wpr_dma.as_mut_slice()[..wpr_data.len()].copy_from_slice(&wpr_data);
    shadow_dma.as_mut_slice()[..wpr_data.len()].copy_from_slice(&wpr_data);

    let mut payload_patched = parsed.acr_payload.to_vec();
    let data_off = parsed.load_header.data_dma_base as usize;
    patch_acr_desc(
        &mut payload_patched,
        data_off,
        wpr_iova,
        wpr_end,
        shadow_iova,
    );
    acr_dma.as_mut_slice()[..payload_patched.len()].copy_from_slice(&payload_patched);
    notes.push(format!(
        "WPR: {}B [{wpr_iova:#x}..{wpr_end:#x}] shadow={shadow_iova:#x}",
        wpr_data.len()
    ));

    // ── Step 4: Build VRAM page tables (hybrid: VRAM PDEs + sysmem PTEs) ──
    // Reuse VRAM addresses for PD chain (falcon can always DMA from VRAM).
    // PT0 entries point to sysmem IOVAs via SYS_MEM_COH aperture.
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

    // PD chain: 16-byte DualPDE entries — pointer in upper 8 bytes (offset 8)
    let pt_ok = wv64(FALCON_PD3_VRAM, 0, 0)
        && wv64(FALCON_PD3_VRAM, 8, encode_vram_pde(FALCON_PD2_VRAM as u64))
        && wv64(FALCON_PD2_VRAM, 0, 0)
        && wv64(FALCON_PD2_VRAM, 8, encode_vram_pde(FALCON_PD1_VRAM as u64))
        && wv64(FALCON_PD1_VRAM, 0, 0)
        && wv64(FALCON_PD1_VRAM, 8, encode_vram_pde(FALCON_PD0_VRAM as u64))
        && wv64(FALCON_PD0_VRAM, 0, 0)
        && wv64(FALCON_PD0_VRAM, 8, encode_vram_pd0_pde(FALCON_PT0_VRAM as u64));

    if !pt_ok {
        notes.push("VRAM page directory chain write failed".to_string());
        return make_fail_result("Hybrid ACR: VRAM PD failed", sec2_before, bar0, notes);
    }

    // PT0: hybrid mapping
    // Pages 0x40..0x6E (IOVAs 0x40000..0x6E000) → SYS_MEM_COH PTEs
    // All other pages → identity-map to VRAM (for BL/ACR internal use)
    let acr_page_start = acr_iova / 4096;
    let wpr_page_end = ((wpr_iova + wpr_buf_size as u64).div_ceil(4096)) as u64;
    let mut pt_fail = false;
    for i in 1u64..512 {
        let pte = if i >= acr_page_start && i < wpr_page_end {
            encode_sysmem_pte(i * 4096)
        } else {
            encode_vram_pte(i * 4096)
        };
        if !wv64(FALCON_PT0_VRAM, (i as usize) * 8, pte) {
            pt_fail = true;
            break;
        }
    }
    if pt_fail {
        notes.push("VRAM PT0 write failed".to_string());
        return make_fail_result("Hybrid ACR: VRAM PT failed", sec2_before, bar0, notes);
    }
    notes.push(format!(
        "VRAM page tables: PD chain in VRAM, PT0 sysmem pages {acr_page_start}..{wpr_page_end}"
    ));

    // Instance block: PDB in VRAM, pointing to PD3 in VRAM
    let pdb_lo: u32 = ((FALCON_PD3_VRAM >> 12) << 12)
        | (1 << 11) // BIG_PAGE_SIZE = 64KiB
        | (1 << 10) // USE_VER2_PT_FORMAT
        ; // bits[1:0] = 0 = VRAM aperture, VOL=0
    if !wv(FALCON_INST_VRAM, 0x200, pdb_lo)
        || !wv(FALCON_INST_VRAM, 0x204, 0)
        || !wv(FALCON_INST_VRAM, 0x208, 0xFFFF_FFFF)
        || !wv(FALCON_INST_VRAM, 0x20C, 0x0001_FFFF)
    {
        notes.push("VRAM instance block write failed".to_string());
        return make_fail_result("Hybrid ACR: inst write failed", sec2_before, bar0, notes);
    }
    notes.push(format!(
        "Instance block: VRAM@{:#x} PDB_LO={pdb_lo:#010x} (VRAM aperture)",
        FALCON_INST_VRAM
    ));

    // ── Step 5: Full Nouveau-style SEC2 reset (gm200_flcn_disable + gm200_flcn_enable) ──
    // Phase A: gm200_flcn_disable
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

    // Phase B: gm200_flcn_enable
    if let Err(e) = pmc_enable_sec2(bar0) {
        notes.push(format!("PMC enable failed: {e}"));
    }
    let _ = bar0.read_u32(base + falcon::MAILBOX0);
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

    // ── Step 6: Bind instance block (exact Nouveau gm200_flcn_fw_load sequence) ──

    let itfen = r(falcon::ITFEN);
    w(falcon::ITFEN, (itfen & !0x01) | 0x01);
    notes.push(format!("ITFEN: {itfen:#010x} → {:#010x}", r(falcon::ITFEN)));

    // 6b. Full nouveau-style bind: DMAIDX → bind_inst → UNK090 → ENG_CONTROL → poll
    let inst_bind_val = instance_block::encode_bind_inst(FALCON_INST_VRAM as u64, 0);
    let (_bind_ok, bind_notes) =
        instance_block::falcon_bind_context(&|off| r(off), &|off, val| w(off, val), inst_bind_val);
    for n in &bind_notes {
        notes.push(n.clone());
    }

    // ── Step 7: Load BL code → IMEM ──
    let hwcfg = r(falcon::HWCFG);
    let code_limit = falcon::imem_size_bytes(hwcfg);
    let boot_size =
        ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;

    falcon_imem_upload_nouveau(bar0, base, imem_addr, &parsed.bl_code, start_tag);
    notes.push(format!(
        "BL code: {}B → IMEM@{imem_addr:#x} boot={boot_addr:#x}",
        parsed.bl_code.len()
    ));

    // ── Step 9: Pre-load ACR data + BL descriptor ──
    let code_dma_base = acr_iova;
    let data_dma_base = acr_iova + data_off as u64;

    let data_section = &payload_patched[data_off..];
    falcon_dmem_upload(bar0, base, 0, data_section);
    let bl_desc = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    falcon_dmem_upload(bar0, base, 0, &bl_desc);
    notes.push(format!(
        "BL desc: code={code_dma_base:#x} data={data_dma_base:#x}"
    ));

    // ── Step 10: Boot SEC2 ──
    w(falcon::MAILBOX0, 0xcafe_beef_u32);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, boot_addr);
    notes.push(format!(
        "BOOTVEC={boot_addr:#x} mb0=0xcafebeef, issuing STARTCPU"
    ));
    falcon_start_cpu(bar0, base);

    // ── Step 11: Poll with PC sampling ──
    let timeout = std::time::Duration::from_secs(5);
    let start_time = std::time::Instant::now();
    let mut pc_samples = Vec::new();
    let mut last_pc = 0u32;

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

    // ── Diagnostics ──
    let exci = r(falcon::EXCI);
    let (trace_count, traces) = super::sec2_hal::sec2_tracepc_dump(bar0);
    notes.push(format!("Diag: EXCI={exci:#010x}"));
    if trace_count > 0 {
        let trace_str: Vec<String> = traces.iter().map(|t| format!("{t:#06x}")).collect();
        notes.push(format!(
            "TRACEPC[0..{trace_count}]: {}",
            trace_str.join(" ")
        ));
    }

    let dmatrfcmd = r(0x118);
    notes.push(format!(
        "DMA: 0x624={:#010x} DMACTL={:#010x} dma_cmd={dmatrfcmd:#010x}",
        r(0x624),
        r(falcon::DMACTL)
    ));

    // GPU MMU fault check — see if falcon DMA triggered a page fault
    let fault_status = bar0.read_u32(mmu::FAULT_STATUS).unwrap_or(0);
    let fault_addr_lo = bar0.read_u32(mmu::FAULT_ADDR_LO).unwrap_or(0);
    let fault_addr_hi = bar0.read_u32(mmu::FAULT_ADDR_HI).unwrap_or(0);
    let fault_inst_lo = bar0.read_u32(mmu::FAULT_INST_LO).unwrap_or(0);
    let fault_inst_hi = bar0.read_u32(mmu::FAULT_INST_HI).unwrap_or(0);
    if fault_status != 0 {
        notes.push(format!(
            "MMU FAULT: status={fault_status:#010x} addr={fault_addr_hi:#010x}_{fault_addr_lo:#010x} inst={fault_inst_hi:#010x}_{fault_inst_lo:#010x}"
        ));
    } else {
        notes.push("MMU fault: none pending".to_string());
    }

    // DMEM dump: first 0x100 bytes (BL desc + ACR state) + 0x200-0x270 (ACR desc)
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

    // Check WPR header status in DMA buffer
    {
        let wpr_slice = wpr_dma.as_mut_slice();
        let fecs_status =
            u32::from_le_bytes([wpr_slice[20], wpr_slice[21], wpr_slice[22], wpr_slice[23]]);
        let gpccs_status =
            u32::from_le_bytes([wpr_slice[44], wpr_slice[45], wpr_slice[46], wpr_slice[47]]);
        notes.push(format!(
            "WPR: FECS status={fecs_status} GPCCS status={gpccs_status} (1=copy, 0xFF=done)"
        ));
    }

    // ── SEC2 Conversation probe ──
    super::sec2_queue::probe_and_bootstrap(bar0, &mut notes);

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);
    notes.push(format!(
        "Final: FECS cpuctl={:#010x} pc={:#06x} exci={:#010x} GPCCS cpuctl={:#010x} pc={:#06x} exci={:#010x}",
        post.fecs_cpuctl, post.fecs_pc, post.fecs_exci,
        post.gpccs_cpuctl, post.gpccs_pc, post.gpccs_exci
    ));

    post.into_result(
        "Hybrid ACR boot (VRAM pages + sysmem data)",
        sec2_before,
        sec2_after,
        notes,
    )
}
