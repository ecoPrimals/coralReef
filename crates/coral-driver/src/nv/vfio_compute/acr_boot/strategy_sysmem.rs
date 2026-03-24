// SPDX-License-Identifier: AGPL-3.0-only

//! System-memory ACR boot (IOMMU DMA).

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::{DmaBackend, MappedBar};
use crate::vfio::dma::DmaBuffer;

use super::boot_result::{AcrBootResult, make_fail_result};
use super::firmware::{AcrFirmwareSet, ParsedAcrFirmware};
use super::instance_block::SEC2_FLCN_BIND_INST;
use super::sec2_hal::{
    Sec2Probe, falcon_dmem_upload, falcon_imem_upload_nouveau, falcon_start_cpu, find_sec2_pmc_bit,
    pmc_enable_sec2, sec2_dmem_read,
};
use super::sysmem_iova;
use super::wpr::{build_bl_dmem_desc, build_wpr, patch_acr_desc};

/// System-memory ACR boot: all DMA buffers in IOMMU-mapped host memory.
///
/// This matches Nouveau's actual architecture on GV100: the WPR, instance
/// block, and page tables all live in system memory. The falcon's MMU
/// translates GPU VAs → IOVAs, which the system IOMMU resolves to host
/// physical pages.
///
/// Key differences from the VRAM path:
///  - Instance block at system memory IOVA (not VRAM offset)
///  - Page table entries use SYS_MEM_COH aperture
///  - 0x668 binding uses SYS_MEM_COH target (bits `[29:28]` = 2)
///  - WPR and ACR payload in DMA buffers (not PRAMIN)
pub fn attempt_sysmem_acr_boot(
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
            return make_fail_result("SysMem ACR: parse failed", sec2_before, bar0, notes);
        }
    };
    notes.push(format!(
        "ACR payload: {}B non_sec=[{:#x}+{:#x}] data=[{:#x}+{:#x}]",
        parsed.acr_payload.len(),
        parsed.load_header.non_sec_code_off,
        parsed.load_header.non_sec_code_size,
        parsed.load_header.data_dma_base,
        parsed.load_header.data_size,
    ));

    // ── Step 2: Allocate DMA buffers ──
    // Each buffer gets its own IOMMU mapping at a distinct IOVA.
    let mut inst_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::INST) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc inst failed: {e}"));
            return make_fail_result("SysMem ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    let mut pd3_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::PD3) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc PD3 failed: {e}"));
            return make_fail_result("SysMem ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    let mut pd2_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::PD2) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc PD2 failed: {e}"));
            return make_fail_result("SysMem ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    let mut pd1_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::PD1) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc PD1 failed: {e}"));
            return make_fail_result("SysMem ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    let mut pd0_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::PD0) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc PD0 failed: {e}"));
            return make_fail_result("SysMem ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    let mut pt0_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::PT0) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc PT0 failed: {e}"));
            return make_fail_result("SysMem ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };

    let acr_payload_size = parsed.acr_payload.len().div_ceil(4096) * 4096;
    let mut acr_dma = match DmaBuffer::new(
        container.clone(),
        acr_payload_size.max(4096),
        sysmem_iova::ACR,
    ) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc ACR payload failed: {e}"));
            return make_fail_result("SysMem ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };

    // WPR: build first to know the size, then allocate
    let wpr_base_iova = sysmem_iova::WPR;
    let wpr_data = build_wpr(fw, wpr_base_iova);
    let wpr_end_iova = wpr_base_iova + wpr_data.len() as u64;
    let wpr_buf_size = wpr_data.len().div_ceil(4096) * 4096;
    let mut wpr_dma = match DmaBuffer::new(container.clone(), wpr_buf_size.max(4096), wpr_base_iova)
    {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc WPR failed: {e}"));
            return make_fail_result("SysMem ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    notes.push(format!(
        "DMA buffers: inst={:#x} PD3={:#x} ACR={:#x}({acr_payload_size:#x}) WPR={:#x}({wpr_buf_size:#x})",
        sysmem_iova::INST, sysmem_iova::PD3, sysmem_iova::ACR, sysmem_iova::WPR
    ));

    // ── Step 3: Populate WPR + patch ACR descriptor ──
    wpr_dma.as_mut_slice()[..wpr_data.len()].copy_from_slice(&wpr_data);
    notes.push(format!(
        "WPR: {}B at IOVA {wpr_base_iova:#x}..{wpr_end_iova:#x}",
        wpr_data.len()
    ));

    let mut payload_patched = parsed.acr_payload.to_vec();
    let data_off = parsed.load_header.data_dma_base as usize;
    patch_acr_desc(&mut payload_patched, data_off, wpr_base_iova, wpr_end_iova);
    acr_dma.as_mut_slice()[..payload_patched.len()].copy_from_slice(&payload_patched);
    notes.push(format!(
        "ACR desc patched: wpr=[{wpr_base_iova:#x}..{wpr_end_iova:#x}] data_off={data_off:#x}"
    ));

    // ── Step 4: Populate page tables (identity map first 2 MiB) ──
    // GP100 V2 MMU PDE: (addr >> 4) | aperture_flags
    //   aperture bits[2:1]: 2=SYS_MEM_COH, VOL=bit3
    // GP100 V2 MMU PTE: (addr >> 4) | VALID(0) | aperture(2:1) | VOL(3)
    let sysmem_pde = |iova: u64| -> u64 {
        const FLAGS: u64 = (2 << 1) | (1 << 3); // SYS_MEM_COH + VOL
        (iova >> 4) | FLAGS
    };
    let sysmem_pd0_pde = |iova: u64| -> u64 {
        sysmem_pde(iova) | (1 << 4) // SPT_PRESENT
    };
    let sysmem_pte = |phys: u64| -> u64 {
        const FLAGS: u64 = 1 | (2 << 1) | (1 << 3); // VALID + SYS_MEM_COH + VOL
        (phys >> 4) | FLAGS
    };
    let w32_le = |buf: &mut [u8], off: usize, val: u32| {
        buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
    };

    // PD3[0] → PD2
    let pde3 = sysmem_pde(sysmem_iova::PD2);
    pd3_dma.as_mut_slice()[0..8].copy_from_slice(&pde3.to_le_bytes());

    // PD2[0] → PD1
    let pde2 = sysmem_pde(sysmem_iova::PD1);
    pd2_dma.as_mut_slice()[0..8].copy_from_slice(&pde2.to_le_bytes());

    // PD1[0] → PD0
    let pde1 = sysmem_pde(sysmem_iova::PD0);
    pd1_dma.as_mut_slice()[0..8].copy_from_slice(&pde1.to_le_bytes());

    // PD0[0] → PT0 (dual entry: small PDE with SPT_PRESENT)
    let pde0 = sysmem_pd0_pde(sysmem_iova::PT0);
    pd0_dma.as_mut_slice()[0..8].copy_from_slice(&pde0.to_le_bytes());

    // PT0: identity-map pages 1..512 (4 KiB each = 2 MiB total)
    // Page 0 left unmapped as null guard.
    let pt = pt0_dma.as_mut_slice();
    for i in 1..512usize {
        let phys = (i as u64) * 4096;
        let pte = sysmem_pte(phys);
        let off = i * 8;
        pt[off..off + 8].copy_from_slice(&pte.to_le_bytes());
    }

    notes.push("Page tables: identity-mapped VA 0..2MiB → IOVA (SYS_MEM_COH)".to_string());

    // ── Step 5: Populate SEC2 instance block ──
    // Minimal: just PDB at offset 0x200 + subcontext 0 PDB at 0x2A0.
    // Aperture = SYS_MEM_COH (2), USE_VER2_PT_FORMAT, BIG_PAGE_SIZE = 64K.
    {
        let inst = inst_dma.as_mut_slice();
        let pd3_iova = sysmem_iova::PD3;
        const APER_COH: u32 = 2; // SYS_MEM_COHERENT in bits[1:0]
        let pdb_lo: u32 = ((pd3_iova >> 12) as u32) << 12
            | (1 << 11)  // BIG_PAGE_SIZE = 64 KiB
            | (1 << 10)  // USE_VER2_PT_FORMAT
            | (1 << 2)   // valid
            | APER_COH;
        let pdb_hi: u32 = (pd3_iova >> 32) as u32;

        w32_le(inst, 0x200, pdb_lo);
        w32_le(inst, 0x204, pdb_hi);

        // VA limit: 128 TB
        w32_le(inst, 0x208, 0xFFFF_FFFF);
        w32_le(inst, 0x20C, 0x0001_FFFF);

        // Subcontext 0 PDB (same as main)
        w32_le(inst, 0x290, 1); // SC_PDB_VALID
        w32_le(inst, 0x2A0, pdb_lo);
        w32_le(inst, 0x2A4, pdb_hi);

        notes.push(format!(
            "Instance block: PDB_LO={pdb_lo:#010x} PDB_HI={pdb_hi:#010x} at IOVA {:#x}",
            sysmem_iova::INST
        ));
    }

    // ── Step 6: Full Nouveau-style SEC2 reset (gm200_flcn_disable + gm200_flcn_enable) ──
    // Phase A: gm200_flcn_disable
    w(0x048, r(0x048) & !0x03); // clear ITFEN bits[1:0]
    w(0x014, 0xFFFF_FFFF); // clear all interrupts (IRQMCLR)
    // PMC disable SEC2 engine
    {
        let pmc_enable: usize = 0x200;
        let sec2_bit = find_sec2_pmc_bit(bar0).unwrap_or(22);
        let sec2_mask = 1u32 << sec2_bit;
        let val = bar0.read_u32(pmc_enable).unwrap_or(0);
        if val & sec2_mask != 0 {
            let _ = bar0.write_u32(pmc_enable, val & !sec2_mask);
            let _ = bar0.read_u32(pmc_enable); // flush
            std::thread::sleep(std::time::Duration::from_micros(20));
        }
    }
    // Falcon-local reset (reset_eng)
    w(0x3C0, 0x01);
    std::thread::sleep(std::time::Duration::from_micros(10));
    w(0x3C0, 0x00);

    // Phase B: gm200_flcn_enable
    if let Err(e) = pmc_enable_sec2(bar0) {
        notes.push(format!("PMC enable failed: {e}"));
    }
    // Wait for mem scrubbing (gm200_flcn_reset_wait_mem_scrubbing)
    let _ = bar0.read_u32(base + falcon::MAILBOX0); // dummy read
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

    let boot0 = bar0.read_u32(0x000).unwrap_or(0);
    w(0x084, boot0);

    let cpuctl_post = r(falcon::CPUCTL);
    let sctl_post = r(falcon::SCTL);
    notes.push(format!(
        "Post-reset: cpuctl={cpuctl_post:#010x} sctl={sctl_post:#010x}"
    ));

    // ── Step 7: Bind instance block (exact Nouveau gm200_flcn_fw_load sequence) ──
    // Nouveau does NOT touch 0x624 or DMACTL in the instance-block path.
    // Instead it: enable ITFEN → bind_inst → poll bind_stat → clear IRQ → set IRQ mask → poll idle.

    // 7a. Enable interrupt/transfer: mask(0x048, 0x01, 0x01)
    let itfen = r(0x048);
    w(0x048, (itfen & !0x01) | 0x01);
    notes.push(format!("ITFEN: {itfen:#010x} → {:#010x}", r(0x048)));

    // 7b. gp102_sec2_flcn_bind_inst: SYS_MEM_COH target
    const SYS_MEM_COH_TARGET: u32 = 2;
    let inst_bind_val = ((sysmem_iova::INST >> 12) as u32) | (SYS_MEM_COH_TARGET << 28);
    w(SEC2_FLCN_BIND_INST, inst_bind_val);
    notes.push(format!("0x668: wrote={inst_bind_val:#010x} (SYS_MEM_COH)"));

    // 7c. Poll bind_stat (0x0dc bits[14:12]) until == 5 (bind complete)
    let bind_start = std::time::Instant::now();
    let mut bind_ok = false;
    loop {
        let stat = (r(0x0dc) & 0x7000) >> 12;
        if stat == 5 {
            bind_ok = true;
            break;
        }
        if bind_start.elapsed() > std::time::Duration::from_millis(10) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(10));
    }
    notes.push(format!(
        "bind_stat→5: {} (0x0dc={:#010x})",
        if bind_ok { "OK" } else { "TIMEOUT" },
        r(0x0dc)
    ));

    // 7d. Clear DMA interrupt + set IRQ mask: mask(0x004, 0x08, 0x08), mask(0x058, 0x02, 0x02)
    let irqs = r(0x004);
    w(0x004, (irqs & !0x08) | 0x08);
    let irqm = r(0x058);
    w(0x058, (irqm & !0x02) | 0x02);

    // 7e. Poll bind_stat until == 0 (idle)
    let idle_start = std::time::Instant::now();
    let mut idle_ok = false;
    loop {
        let stat = (r(0x0dc) & 0x7000) >> 12;
        if stat == 0 {
            idle_ok = true;
            break;
        }
        if idle_start.elapsed() > std::time::Duration::from_millis(10) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(10));
    }
    notes.push(format!(
        "bind_stat→0: {} (0x0dc={:#010x})",
        if idle_ok { "OK" } else { "TIMEOUT" },
        r(0x0dc)
    ));

    // ── Step 8: Load BL code → IMEM ──
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

    // ── Step 10: Pre-load ACR data to DMEM + BL descriptor overlay ──
    // The BL's DMA xcld may succeed now (system memory path), but we also
    // pre-load the data section as insurance.
    let code_dma_base = sysmem_iova::ACR;
    let data_dma_base = sysmem_iova::ACR + data_off as u64;

    let data_section = &payload_patched[data_off..];
    falcon_dmem_upload(bar0, base, 0, data_section);
    notes.push(format!(
        "ACR data pre-loaded: {}B → DMEM@0",
        data_section.len()
    ));

    let bl_desc = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    falcon_dmem_upload(bar0, base, 0, &bl_desc);
    notes.push(format!(
        "BL desc: code={code_dma_base:#x} data={data_dma_base:#x}"
    ));

    // ── Step 10: Boot SEC2 ──
    // Nouveau writes 0xcafebeef to MAILBOX0 before start; expects 0x10 on success.
    w(falcon::MAILBOX0, 0xcafe_beef_u32);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, boot_addr);
    notes.push(format!(
        "BOOTVEC={boot_addr:#x} mb0=0xcafebeef, issuing STARTCPU"
    ));
    falcon_start_cpu(bar0, base);

    // ── Step 12: Poll with PC sampling ──
    let timeout = std::time::Duration::from_secs(5);
    let start_time = std::time::Instant::now();
    let mut pc_samples = Vec::new();
    let mut last_pc = 0u32;

    let mut all_pcs: Vec<u32> = Vec::new();
    for _ in 0..500 {
        let pc = r(0x030);
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
        let pc = r(0x030);

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

    // ── Step 13: Diagnostics ──
    let exci = r(falcon::EXCI);
    let tracepc = [r(0x030), r(0x034), r(0x038), r(0x03C)];
    notes.push(format!(
        "Diag: EXCI={exci:#010x} TRACEPC=[{:#06x}, {:#06x}, {:#06x}, {:#06x}]",
        tracepc[0], tracepc[1], tracepc[2], tracepc[3]
    ));

    let dma_624 = r(0x624);
    let dma_10c = r(falcon::DMACTL);
    let dma_668 = r(SEC2_FLCN_BIND_INST);
    let dmatrfbase = r(0x110);
    let dmatrfmoffs = r(0x114);
    let dmatrfcmd = r(0x118);
    let dmatrffboffs = r(0x11C);
    notes.push(format!(
        "DMA: 0x624={dma_624:#010x} DMACTL={dma_10c:#010x} 0x668={dma_668:#010x}"
    ));
    notes.push(format!(
        "DMA xfer: base={dmatrfbase:#010x} moffs={dmatrfmoffs:#010x} cmd={dmatrfcmd:#010x} fboffs={dmatrffboffs:#010x}"
    ));

    // DMEM diagnostic: ACR descriptor region
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

    // Check WPR header status in DMA buffer — did ACR modify it?
    {
        let wpr_slice = wpr_dma.as_mut_slice();
        let fecs_status =
            u32::from_le_bytes([wpr_slice[20], wpr_slice[21], wpr_slice[22], wpr_slice[23]]);
        let gpccs_status =
            u32::from_le_bytes([wpr_slice[44], wpr_slice[45], wpr_slice[46], wpr_slice[47]]);
        notes.push(format!(
            "WPR headers: FECS status={fecs_status} GPCCS status={gpccs_status} (0=none, 1=copy, 0xFF=done)"
        ));
    }

    // ── Capture final state ──
    let sec2_after = Sec2Probe::capture(bar0);
    let fecs_cpuctl_after = bar0
        .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    let fecs_mailbox0_after = bar0
        .read_u32(falcon::FECS_BASE + falcon::MAILBOX0)
        .unwrap_or(0);
    let gpccs_cpuctl_after = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    notes.push(format!(
        "Final: FECS cpuctl={fecs_cpuctl_after:#010x} GPCCS cpuctl={gpccs_cpuctl_after:#010x}"
    ));

    let success = fecs_cpuctl_after & falcon::CPUCTL_HRESET == 0 && fecs_mailbox0_after != 0;

    AcrBootResult {
        strategy: "System-memory ACR boot (IOMMU DMA)",
        sec2_before,
        sec2_after,
        fecs_cpuctl_after,
        fecs_mailbox0_after,
        gpccs_cpuctl_after,
        success,
        notes,
    }
}
