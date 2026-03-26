// SPDX-License-Identifier: AGPL-3.0-only

//! System-memory ACR boot (IOMMU DMA).

use crate::vfio::channel::registers::{falcon, misc};
use crate::vfio::device::{DmaBackend, MappedBar};
use crate::vfio::dma::DmaBuffer;

use crate::vfio::memory::{MemoryRegion, PraminRegion};

use super::boot_result::{AcrBootResult, make_fail_result};
use super::firmware::{AcrFirmwareSet, ParsedAcrFirmware};
use super::instance_block::{self, SEC2_FLCN_BIND_INST};
use super::sec2_hal::{
    Sec2Probe, falcon_dmem_upload, falcon_imem_upload_nouveau, falcon_start_cpu, find_sec2_pmc_bit,
    pmc_enable_sec2, sec2_dmem_read, sec2_emem_write, sec2_prepare_physical_first,
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
/// System-memory ACR boot.
///
/// When `skip_blob_dma` is `true` (default for boot solver), blob_size is zeroed
/// so the ACR firmware skips its internal blob DMA. This achieves HS mode but
/// causes the firmware to exit immediately. Set to `false` to let the firmware
/// attempt full initialization including the CMDQ idle loop.
pub fn attempt_sysmem_acr_boot(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    container: DmaBackend,
) -> AcrBootResult {
    attempt_sysmem_acr_boot_inner(bar0, fw, container, true)
}

/// System-memory ACR boot with blob DMA enabled — firmware attempts full init.
pub fn attempt_sysmem_acr_boot_full(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    container: DmaBackend,
) -> AcrBootResult {
    attempt_sysmem_acr_boot_inner(bar0, fw, container, false)
}

/// Hybrid ACR boot: sysmem for ACR payload + page tables, VRAM for WPR.
///
/// The ACR firmware's internal DMA uses physical addressing to read the WPR
/// from VRAM. Our sysmem-only approach fails because the firmware bypasses
/// the falcon's page table for WPR access. This hybrid writes the WPR to
/// actual VRAM via PRAMIN while keeping everything else in system memory.
pub fn attempt_hybrid_sysmem_vram_boot(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    container: DmaBackend,
) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);

    // Verify PRAMIN/VRAM is accessible
    let sentinel_ok = match PraminRegion::new(bar0, 0x5_0000, 8) {
        Ok(mut rgn) => {
            let s = 0xACB0_CAFE_u32;
            let _ = rgn.write_u32(0, s);
            let rb = rgn.read_u32(0).unwrap_or(0);
            let ok = rb == s;
            notes.push(format!("VRAM sentinel: {s:#x}→{rb:#x} ok={ok}"));
            ok
        }
        Err(e) => {
            notes.push(format!("PRAMIN unavailable: {e}"));
            false
        }
    };
    if !sentinel_ok {
        return make_fail_result(
            "Hybrid ACR: VRAM inaccessible",
            sec2_before,
            bar0,
            notes,
        );
    }

    // Build WPR with VRAM base address (firmware reads from VRAM physical)
    let wpr_vram_base: u64 = 0x7_0000;
    let wpr_data = build_wpr(fw, wpr_vram_base);
    let wpr_end = wpr_vram_base + wpr_data.len() as u64;
    notes.push(format!(
        "WPR: {}B for VRAM@{wpr_vram_base:#x}..{wpr_end:#x}",
        wpr_data.len()
    ));

    // Write WPR to VRAM via PRAMIN
    let write_vram = |vram_addr: u32, data: &[u8], notes: &mut Vec<String>| -> bool {
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
                            notes.push(format!("VRAM write fail @{chunk_vram:#x}+{word_off:#x}"));
                            return false;
                        }
                    }
                    off += chunk_size;
                }
                Err(e) => {
                    notes.push(format!("PRAMIN fail @{chunk_vram:#x}: {e}"));
                    return false;
                }
            }
        }
        true
    };

    if !write_vram(wpr_vram_base as u32, &wpr_data, &mut notes) {
        return make_fail_result("Hybrid ACR: WPR→VRAM write failed", sec2_before, bar0, notes);
    }
    // Also write shadow copy (ACR verifies WPR against shadow)
    let shadow_vram: u64 = 0x6_0000;
    if !write_vram(shadow_vram as u32, &wpr_data, &mut notes) {
        return make_fail_result("Hybrid ACR: shadow→VRAM write failed", sec2_before, bar0, notes);
    }
    notes.push(format!("WPR + shadow written to VRAM via PRAMIN"));

    // Verify WPR readback
    if let Ok(rgn) = PraminRegion::new(bar0, wpr_vram_base as u32, 8) {
        let w0 = rgn.read_u32(0).unwrap_or(0xDEAD);
        let w1 = rgn.read_u32(4).unwrap_or(0xDEAD);
        notes.push(format!("VRAM WPR verify: [{wpr_vram_base:#x}]={w0:#010x} +4={w1:#010x}"));
    }

    // Now delegate to the inner boot, but with blob_size preserved (full init).
    // The ACR descriptor will point to VRAM addresses for the WPR.
    // We patch wpr_start/end and shadow to VRAM offsets.
    let parsed = match ParsedAcrFirmware::parse(fw) {
        Ok(p) => p,
        Err(e) => {
            notes.push(format!("Firmware parse failed: {e}"));
            return make_fail_result("Hybrid ACR: parse failed", sec2_before, bar0, notes);
        }
    };

    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    // Allocate sysmem DMA buffers for everything EXCEPT the WPR
    let _low_catch = DmaBuffer::new(
        container.clone(),
        sysmem_iova::LOW_CATCH_SIZE,
        sysmem_iova::LOW_CATCH,
    )
    .ok();
    let mut inst_dma = DmaBuffer::new(container.clone(), 4096, sysmem_iova::INST)
        .map_err(|e| notes.push(format!("inst alloc: {e}")))
        .ok();
    let mut pd3_dma = DmaBuffer::new(container.clone(), 4096, sysmem_iova::PD3).ok();
    let mut pd2_dma = DmaBuffer::new(container.clone(), 4096, sysmem_iova::PD2).ok();
    let mut pd1_dma = DmaBuffer::new(container.clone(), 4096, sysmem_iova::PD1).ok();
    let mut pd0_dma = DmaBuffer::new(container.clone(), 4096, sysmem_iova::PD0).ok();
    let mut pt0_dma = DmaBuffer::new(container.clone(), 4096, sysmem_iova::PT0).ok();

    let acr_payload_size = parsed.acr_payload.len().div_ceil(4096) * 4096;
    let mut acr_dma =
        DmaBuffer::new(container.clone(), acr_payload_size.max(4096), sysmem_iova::ACR).ok();

    if inst_dma.is_none()
        || pd3_dma.is_none()
        || pd2_dma.is_none()
        || pd1_dma.is_none()
        || pd0_dma.is_none()
        || pt0_dma.is_none()
        || acr_dma.is_none()
    {
        notes.push("DMA alloc failed for one or more buffers".to_string());
        return make_fail_result("Hybrid ACR: DMA alloc failed", sec2_before, bar0, notes);
    }

    let inst_dma = inst_dma.as_mut().unwrap();
    let pd3_dma = pd3_dma.as_mut().unwrap();
    let pd2_dma = pd2_dma.as_mut().unwrap();
    let pd1_dma = pd1_dma.as_mut().unwrap();
    let pd0_dma = pd0_dma.as_mut().unwrap();
    let pt0_dma = pt0_dma.as_mut().unwrap();
    let acr_dma = acr_dma.as_mut().unwrap();

    // Patch ACR descriptor to point to VRAM addresses for WPR
    let mut payload_patched = parsed.acr_payload.to_vec();
    let data_off = parsed.load_header.data_dma_base as usize;
    patch_acr_desc(
        &mut payload_patched,
        data_off,
        wpr_vram_base,
        wpr_end,
        shadow_vram,
    );
    notes.push(format!(
        "ACR desc: wpr=[{wpr_vram_base:#x}..{wpr_end:#x}] shadow={shadow_vram:#x} (VRAM phys)"
    ));
    acr_dma.as_mut_slice()[..payload_patched.len()].copy_from_slice(&payload_patched);

    // Page tables: identity-map VA 0..2MiB → IOVA (SYS_MEM_COH)
    let sysmem_pde = |iova: u64| -> u64 {
        const FLAGS: u64 = (2 << 1) | (1 << 3);
        (iova >> 4) | FLAGS
    };
    let sysmem_pd0_pde = |iova: u64| -> u64 { sysmem_pde(iova) | (1 << 4) };
    let sysmem_pte = |phys: u64| -> u64 {
        const FLAGS: u64 = 1 | (2 << 1) | (1 << 3);
        (phys >> 4) | FLAGS
    };
    let w32_le = |buf: &mut [u8], off: usize, val: u32| {
        buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
    };

    // GV100 MMU v2: directory pointers in upper 8 bytes of 16-byte entry
    pd3_dma.as_mut_slice()[8..16]
        .copy_from_slice(&sysmem_pde(sysmem_iova::PD2).to_le_bytes());
    pd2_dma.as_mut_slice()[8..16]
        .copy_from_slice(&sysmem_pde(sysmem_iova::PD1).to_le_bytes());
    pd1_dma.as_mut_slice()[8..16]
        .copy_from_slice(&sysmem_pde(sysmem_iova::PD0).to_le_bytes());
    pd0_dma.as_mut_slice()[8..16]
        .copy_from_slice(&sysmem_pd0_pde(sysmem_iova::PT0).to_le_bytes());

    // VRAM aperture PTE: VALID + VRAM(0) + VOL
    let vram_pte = |phys: u64| -> u64 {
        const FLAGS: u64 = 1 | (0 << 1) | (1 << 3); // VALID + VRAM aperture + VOL
        (phys >> 4) | FLAGS
    };

    let pt = pt0_dma.as_mut_slice();
    let shadow_page_start = (shadow_vram as usize) / 4096; // page 24 (0x60000)
    let wpr_page_end = ((wpr_end as usize) + 4095) / 4096; // page ~125 (0x7D000)
    let mut vram_pages = 0;
    for i in 0..512usize {
        let phys = (i as u64) * 4096;
        // Use VRAM aperture for WPR + shadow pages; SYS_MEM for everything else
        let pte = if i >= shadow_page_start && i < wpr_page_end {
            vram_pages += 1;
            vram_pte(phys)
        } else {
            sysmem_pte(phys)
        };
        let off = i * 8;
        pt[off..off + 8].copy_from_slice(&pte.to_le_bytes());
    }
    notes.push(format!(
        "PT: VA 0..2MiB mixed — {vram_pages} VRAM pages [{shadow_page_start}..{wpr_page_end}), rest SYS_MEM"
    ));

    // Instance block
    {
        let inst = inst_dma.as_mut_slice();
        let pd3_iova = sysmem_iova::PD3;
        const APER_COH: u32 = 2;
        let pdb_lo: u32 =
            ((pd3_iova >> 12) as u32) << 12 | (1 << 11) | (1 << 10) | (1 << 2) | APER_COH;
        let pdb_hi: u32 = (pd3_iova >> 32) as u32;
        w32_le(inst, 0x200, pdb_lo);
        w32_le(inst, 0x204, pdb_hi);
        w32_le(inst, 0x208, 0xFFFF_FFFF);
        w32_le(inst, 0x20C, 0x0001_FFFF);
        w32_le(inst, 0x290, 1);
        w32_le(inst, 0x2A0, pdb_lo);
        w32_le(inst, 0x2A4, pdb_hi);
        notes.push(format!("Inst block: PDB={pdb_lo:#010x} at IOVA {:#x}", sysmem_iova::INST));
    }

    // SEC2 reset + enable
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
        notes.push(format!("PMC enable: {e}"));
    }
    let scrub_start = std::time::Instant::now();
    loop {
        let scrub = r(falcon::DMACTL);
        if scrub & 0x06 == 0 {
            break;
        }
        if scrub_start.elapsed() > std::time::Duration::from_millis(100) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }
    let boot0 = bar0.read_u32(misc::BOOT0).unwrap_or(0);
    w(0x084, boot0);

    // Bind instance block
    w(falcon::ITFEN, (r(falcon::ITFEN) & !0x01) | 0x01);
    let inst_bind_val = instance_block::encode_bind_inst(sysmem_iova::INST, 2);
    let (bind_ok, bind_notes) =
        instance_block::falcon_bind_context(&|off| r(off), &|off, val| w(off, val), inst_bind_val);
    for n in &bind_notes {
        notes.push(n.clone());
    }
    notes.push(format!("bind: {}", if bind_ok { "OK" } else { "FAIL" }));

    // Load BL code → IMEM
    let hwcfg = r(falcon::HWCFG);
    let code_limit = falcon::imem_size_bytes(hwcfg);
    let boot_size =
        ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;
    falcon_imem_upload_nouveau(bar0, base, imem_addr, &parsed.bl_code, start_tag);

    // Pre-load ACR data to DMEM
    let code_dma_base = sysmem_iova::ACR;
    let data_dma_base = sysmem_iova::ACR + data_off as u64;
    let data_section = &payload_patched[data_off..];
    falcon_dmem_upload(bar0, base, 0, data_section);
    let bl_desc = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    falcon_dmem_upload(bar0, base, 0, &bl_desc);
    notes.push(format!("BL: code={code_dma_base:#x} data={data_dma_base:#x}"));

    // Exp 105: TLB invalidation (0x100CEC is TLB hi-addr, not WPR2).
    // Removed WPR2 writes that corrupted the TLB invalidation register.
    {
        let pdb_inv = ((sysmem_iova::INST >> 12) << 4) as u32;
        let _ = bar0.write_u32(0x100CB8, pdb_inv);
        let _ = bar0.write_u32(0x100CEC, 0);
        let _ = bar0.write_u32(0x100CBC, 0x8000_0005);
        for _ in 0..200 {
            if bar0.read_u32(0x100C80).unwrap_or(0) & 0x0000_8000 != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
        notes.push(format!("TLB invalidate: PDB={pdb_inv:#x}"));
    }

    // Boot SEC2
    w(falcon::MAILBOX0, 0xcafe_beef_u32);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, boot_addr);
    notes.push(format!("BOOTVEC={boot_addr:#x}, STARTCPU"));
    falcon_start_cpu(bar0, base);

    // Poll with PC sampling
    let timeout = std::time::Duration::from_secs(5);
    let poll_start = std::time::Instant::now();
    let mut pc_trace = Vec::new();
    let mut last_pc = 0u32;
    let mut settled = 0u32;

    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let pc = r(falcon::PC);
        if pc != last_pc {
            pc_trace.push(format!("{pc:#06x}@{}ms", poll_start.elapsed().as_millis()));
            last_pc = pc;
            settled = 0;
        } else {
            settled += 1;
        }
        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;
        let hreset = cpuctl & falcon::CPUCTL_HRESET != 0;
        if mb0 != 0 || halted || hreset || settled > 200
            || poll_start.elapsed() > timeout
        {
            let sctl = r(falcon::SCTL);
            let exci = r(falcon::EXCI);
            notes.push(format!(
                "Exit: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#06x} sctl={sctl:#010x} EXCI={exci:#010x} ({}ms)",
                poll_start.elapsed().as_millis()
            ));
            if !pc_trace.is_empty() {
                notes.push(format!("PC: [{}]", pc_trace.join(", ")));
            }
            break;
        }
    }

    // Check FECS/GPCCS state
    let fecs_cpuctl = bar0.read_u32(falcon::FECS_BASE + falcon::CPUCTL).unwrap_or(0xDEAD);
    let fecs_pc = bar0.read_u32(falcon::FECS_BASE + falcon::PC).unwrap_or(0xDEAD);
    let fecs_exci = bar0.read_u32(falcon::FECS_BASE + falcon::EXCI).unwrap_or(0xDEAD);
    let gpccs_cpuctl = bar0.read_u32(falcon::GPCCS_BASE + falcon::CPUCTL).unwrap_or(0xDEAD);
    let gpccs_pc = bar0.read_u32(falcon::GPCCS_BASE + falcon::PC).unwrap_or(0xDEAD);
    notes.push(format!(
        "FECS: cpuctl={fecs_cpuctl:#010x} pc={fecs_pc:#06x} exci={fecs_exci:#010x}"
    ));
    notes.push(format!(
        "GPCCS: cpuctl={gpccs_cpuctl:#010x} pc={gpccs_pc:#06x}"
    ));

    // Check WPR status in VRAM
    if let Ok(rgn) = PraminRegion::new(bar0, wpr_vram_base as u32, 48) {
        let fecs_status = rgn.read_u32(20).unwrap_or(0xDEAD);
        let gpccs_status = rgn.read_u32(44).unwrap_or(0xDEAD);
        notes.push(format!(
            "VRAM WPR status: FECS={fecs_status} GPCCS={gpccs_status} (0xFF=done)"
        ));
    }

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);
    let success = post.success();
    notes.push(format!("success={success}"));

    post.into_result("Hybrid sysmem+VRAM ACR boot", sec2_before, sec2_after, notes)
}

fn attempt_sysmem_acr_boot_inner(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    container: DmaBackend,
    skip_blob_dma: bool,
) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    notes.push(format!("SEC2 state: {:?}", sec2_before.state));

    // ── Step 0: VRAM accessibility + GPU memory controller probe ──
    {
        let vram_ok = match PraminRegion::new(bar0, 0x5_0000, 16) {
            Ok(mut rgn) => {
                let s1 = 0xACB0_1234_u32;
                let s2 = 0xFEED_FACE_u32;
                let _ = rgn.write_u32(0, s1);
                let _ = rgn.write_u32(4, s2);
                let rb1 = rgn.read_u32(0).unwrap_or(0);
                let rb2 = rgn.read_u32(4).unwrap_or(0);
                let ok = rb1 == s1 && rb2 == s2;
                notes.push(format!(
                    "VRAM test: {s1:#x}→{rb1:#x} {s2:#x}→{rb2:#x} ok={ok}"
                ));
                ok
            }
            Err(e) => {
                notes.push(format!("PRAMIN unavailable: {e}"));
                false
            }
        };
        if !vram_ok {
            notes.push("VRAM INACCESSIBLE — all mirrors are invalid".to_string());
        }
        let mc_boot = bar0.read_u32(0x100000).unwrap_or(0xDEAD);
        let mc_cfg = bar0.read_u32(0x100004).unwrap_or(0xDEAD);
        let fbhub0 = bar0.read_u32(0x100800).unwrap_or(0xDEAD);
        let fbhub4 = bar0.read_u32(0x100804).unwrap_or(0xDEAD);
        let fbhub8 = bar0.read_u32(0x100808).unwrap_or(0xDEAD);
        let pmc_enable = bar0.read_u32(0x000200).unwrap_or(0xDEAD);
        notes.push(format!(
            "MC: PFB[0]={mc_boot:#010x} [4]={mc_cfg:#010x} FBHUB={fbhub0:#010x}/{fbhub4:#010x}/{fbhub8:#010x} PMC_EN={pmc_enable:#010x}"
        ));
    }

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

    // Low catch-all: provides IOMMU backing for VA 0x0..0x40000 so ACR
    // internal DMA (observed at 0x26000-0x28000) doesn't IOMMU-fault.
    let _low_catch = match DmaBuffer::new(
        container.clone(),
        sysmem_iova::LOW_CATCH_SIZE,
        sysmem_iova::LOW_CATCH,
    ) {
        Ok(b) => {
            notes.push(format!(
                "Low catch-all: {}KiB at IOVA {:#x}",
                sysmem_iova::LOW_CATCH_SIZE / 1024,
                sysmem_iova::LOW_CATCH
            ));
            b
        }
        Err(e) => {
            notes.push(format!("Low catch-all alloc failed (non-fatal): {e}"));
            // Non-fatal: continue without it. Some IOMMUs reject IOVA 0.
            // Fall through — we'll allocate a smaller buffer at 0x1000 instead.
            match DmaBuffer::new(
                container.clone(),
                sysmem_iova::LOW_CATCH_SIZE - 0x1000,
                0x1000,
            ) {
                Ok(b) => {
                    notes.push("Low catch-all fallback: mapped 0x1000..0x40000".to_string());
                    b
                }
                Err(e2) => {
                    notes.push(format!("Low catch-all fallback also failed: {e2}"));
                    return make_fail_result(
                        "SysMem ACR: low catch DMA alloc failed",
                        sec2_before,
                        bar0,
                        notes,
                    );
                }
            }
        }
    };

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

    // Shadow WPR: separate copy for ACR verification
    let shadow_iova = sysmem_iova::SHADOW;
    let mut shadow_dma =
        match DmaBuffer::new(container.clone(), wpr_buf_size.max(4096), shadow_iova) {
            Ok(b) => b,
            Err(e) => {
                notes.push(format!("DMA alloc shadow failed: {e}"));
                return make_fail_result(
                    "SysMem ACR: DMA alloc failed",
                    sec2_before,
                    bar0,
                    notes,
                );
            }
        };

    notes.push(format!(
        "DMA buffers: inst={:#x} PD3={:#x} ACR={:#x}({acr_payload_size:#x}) shadow={shadow_iova:#x} WPR={:#x}({wpr_buf_size:#x})",
        sysmem_iova::INST, sysmem_iova::PD3, sysmem_iova::ACR, sysmem_iova::WPR
    ));

    // Fill IOVA gaps between named buffers so firmware DMA never hits unmapped holes.
    let acr_end = sysmem_iova::ACR + acr_payload_size as u64;
    let _mid_gap1 = if acr_end < sysmem_iova::SHADOW {
        let gap = (sysmem_iova::SHADOW - acr_end) as usize;
        DmaBuffer::new(container.clone(), gap, acr_end).ok()
    } else {
        None
    };
    let shadow_end = shadow_iova + wpr_buf_size as u64;
    let _mid_gap2 = if shadow_end < sysmem_iova::WPR {
        let gap = (sysmem_iova::WPR - shadow_end) as usize;
        DmaBuffer::new(container.clone(), gap, shadow_end).ok()
    } else {
        None
    };

    // ── Step 3: Populate WPR + shadow + patch ACR descriptor ──
    wpr_dma.as_mut_slice()[..wpr_data.len()].copy_from_slice(&wpr_data);
    shadow_dma.as_mut_slice()[..wpr_data.len()].copy_from_slice(&wpr_data);
    notes.push(format!(
        "WPR: {}B at IOVA {wpr_base_iova:#x}..{wpr_end_iova:#x} shadow={shadow_iova:#x}",
        wpr_data.len()
    ));

    // Mirror WPR + shadow to VRAM via PRAMIN.
    // In HS mode, the ACR firmware's internal DMA reads from VRAM physical
    // addresses (bypassing the falcon page table). Without this mirror, the
    // WPR at VRAM 0x70000 is empty and the firmware DMA-traps.
    let vram_mirror = |vram_addr: u32, data: &[u8]| -> bool {
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
        true
    };
    let wpr_mirrored = vram_mirror(wpr_base_iova as u32, &wpr_data);
    let shadow_mirrored = vram_mirror(shadow_iova as u32, &wpr_data);
    notes.push(format!(
        "VRAM mirror: WPR={wpr_mirrored} shadow={shadow_mirrored}"
    ));

    let mut payload_patched = parsed.acr_payload.to_vec();
    let data_off = parsed.load_header.data_dma_base as usize;
    patch_acr_desc(
        &mut payload_patched,
        data_off,
        wpr_base_iova,
        wpr_end_iova,
        shadow_iova,
    );
    // Log the original blob_base/blob_size before any patching
    if data_off + 0x268 <= payload_patched.len() {
        let orig_blob_size = u32::from_le_bytes(
            payload_patched[data_off + 0x258..data_off + 0x25C]
                .try_into()
                .unwrap_or([0; 4]),
        );
        let orig_blob_base = u64::from_le_bytes(
            payload_patched[data_off + 0x260..data_off + 0x268]
                .try_into()
                .unwrap_or([0; 8]),
        );
        notes.push(format!(
            "Original ACR desc: blob_size={orig_blob_size:#x} blob_base={orig_blob_base:#x}"
        ));
    }

    // Dump raw firmware bytes at the crash PC offset for offline analysis
    if payload_patched.len() > 0x510 {
        let fw_at_500: Vec<String> = (0x500..0x510)
            .step_by(4)
            .map(|off| {
                let w = u32::from_le_bytes(
                    payload_patched[off..off + 4].try_into().unwrap_or([0; 4]),
                );
                format!("[{off:#05x}]={w:#010x}")
            })
            .collect();
        notes.push(format!("FW code @0x500: {}", fw_at_500.join(" ")));
        // Also check around the HS entry point — might be near 0x100 (non_sec_code_size)
        let hs_entry_off = parsed.load_header.non_sec_code_size as usize;
        if hs_entry_off + 16 <= payload_patched.len() {
            let fw_at_hs: Vec<String> = (hs_entry_off..hs_entry_off + 16)
                .step_by(4)
                .map(|off| {
                    let w = u32::from_le_bytes(
                        payload_patched[off..off + 4].try_into().unwrap_or([0; 4]),
                    );
                    format!("[{off:#05x}]={w:#010x}")
                })
                .collect();
            notes.push(format!("FW code @HS_entry({hs_entry_off:#x}): {}", fw_at_hs.join(" ")));
        }
    }

    if skip_blob_dma {
        if data_off + 0x268 <= payload_patched.len() {
            payload_patched[data_off + 0x258..data_off + 0x25C]
                .copy_from_slice(&0u32.to_le_bytes());
            payload_patched[data_off + 0x260..data_off + 0x268]
                .copy_from_slice(&0u64.to_le_bytes());
            notes.push("blob_size=0: skip ACR blob DMA (WPR pre-populated)".to_string());
        }
    } else {
        notes.push("blob_size preserved: firmware will attempt full WPR→falcon DMA".to_string());
    }
    acr_dma.as_mut_slice()[..payload_patched.len()].copy_from_slice(&payload_patched);
    notes.push(format!(
        "ACR desc patched: wpr=[{wpr_base_iova:#x}..{wpr_end_iova:#x}] shadow={shadow_iova:#x}"
    ));

    // Mirror ACR payload to VRAM so HS-mode DMA can read it.
    if !skip_blob_dma {
        let acr_mirrored = vram_mirror(sysmem_iova::ACR as u32, &payload_patched);
        notes.push(format!(
            "VRAM mirror ACR: {acr_mirrored} ({}B at VRAM {:#x})",
            payload_patched.len(),
            sysmem_iova::ACR
        ));

        // Verify VRAM readback of mirrored data
        if let Ok(rgn) = PraminRegion::new(bar0, sysmem_iova::ACR as u32, 16) {
            let v0 = rgn.read_u32(0).unwrap_or(0xDEAD);
            let v4 = rgn.read_u32(4).unwrap_or(0xDEAD);
            let e0 = u32::from_le_bytes(payload_patched[0..4].try_into().unwrap_or([0; 4]));
            let e4 = u32::from_le_bytes(payload_patched[4..8].try_into().unwrap_or([0; 4]));
            notes.push(format!(
                "VRAM ACR readback: [{:#x}]={v0:#010x}(expect {e0:#010x}) +4={v4:#010x}(expect {e4:#010x}) match={}",
                sysmem_iova::ACR, v0 == e0 && v4 == e4
            ));
        }
        if let Ok(rgn) = PraminRegion::new(bar0, sysmem_iova::WPR as u32, 16) {
            let w0 = rgn.read_u32(0).unwrap_or(0xDEAD);
            let w4 = rgn.read_u32(4).unwrap_or(0xDEAD);
            let x0 = u32::from_le_bytes(wpr_data[0..4].try_into().unwrap_or([0; 4]));
            let x4 = u32::from_le_bytes(wpr_data[4..8].try_into().unwrap_or([0; 4]));
            notes.push(format!(
                "VRAM WPR readback: [{:#x}]={w0:#010x}(expect {x0:#010x}) +4={w4:#010x}(expect {x4:#010x}) match={}",
                sysmem_iova::WPR, w0 == x0 && w4 == x4
            ));
        }
    }

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

    // GV100 MMU v2 uses 16-byte PDE entries at ALL directory levels.
    // The directory pointer goes in the UPPER 8 bytes (offset 8..16);
    // the lower 8 bytes (offset 0..8) are zeroed.
    // This matches build_vram_falcon_inst_block and nouveau's format.

    // PD3[0] → PD2 (upper 8 bytes)
    let pde3 = sysmem_pde(sysmem_iova::PD2);
    pd3_dma.as_mut_slice()[0..8].copy_from_slice(&0u64.to_le_bytes());
    pd3_dma.as_mut_slice()[8..16].copy_from_slice(&pde3.to_le_bytes());

    // PD2[0] → PD1 (upper 8 bytes)
    let pde2 = sysmem_pde(sysmem_iova::PD1);
    pd2_dma.as_mut_slice()[0..8].copy_from_slice(&0u64.to_le_bytes());
    pd2_dma.as_mut_slice()[8..16].copy_from_slice(&pde2.to_le_bytes());

    // PD1[0] → PD0 (upper 8 bytes)
    let pde1 = sysmem_pde(sysmem_iova::PD0);
    pd1_dma.as_mut_slice()[0..8].copy_from_slice(&0u64.to_le_bytes());
    pd1_dma.as_mut_slice()[8..16].copy_from_slice(&pde1.to_le_bytes());

    // PD0[0] → PT0 (dual entry: small PT in upper 8 bytes, big PT = 0 in lower)
    let pde0 = sysmem_pd0_pde(sysmem_iova::PT0);
    pd0_dma.as_mut_slice()[0..8].copy_from_slice(&0u64.to_le_bytes());
    pd0_dma.as_mut_slice()[8..16].copy_from_slice(&pde0.to_le_bytes());

    // VRAM aperture PTE for WPR/shadow pages in full-init mode
    let vram_pte = |phys: u64| -> u64 {
        const FLAGS: u64 = 1 | (0 << 1) | (1 << 3); // VALID + VRAM(0) + VOL
        (phys >> 4) | FLAGS
    };

    // PT0: identity-map all 512 pages (4 KiB each = 2 MiB total).
    // In full-init mode, ALL pages with VRAM mirrors use VRAM aperture.
    // This matches nouveau's approach where ACR firmware lives in INST/VRAM
    // and HS-mode DMA accesses VRAM, not system memory.
    let pt = pt0_dma.as_mut_slice();
    let acr_start_page = (sysmem_iova::ACR as usize) / 4096;
    let acr_end_page = (sysmem_iova::ACR as usize + acr_payload_size + 4095) / 4096;
    let shadow_page = (shadow_iova as usize) / 4096;
    let wpr_end_page = (wpr_end_iova as usize + 4095) / 4096;
    let mut vram_pages = 0u32;
    for i in 0..512usize {
        let phys = (i as u64) * 4096;
        // Exp 104: Force ALL sysmem PTEs to test if VRAM PTEs caused HS
        // authentication failure. The VRAM mirrors are still populated —
        // this only changes the DMA path from VRAM to sysmem.
        let use_vram = false;
        let _ = (acr_start_page, acr_end_page, shadow_page, wpr_end_page, skip_blob_dma);
        let pte = if use_vram {
            vram_pages += 1;
            vram_pte(phys)
        } else {
            sysmem_pte(phys)
        };
        let off = i * 8;
        pt[off..off + 8].copy_from_slice(&pte.to_le_bytes());
    }

    if vram_pages > 0 {
        notes.push(format!(
            "PT: {vram_pages} VRAM pages (ACR[{acr_start_page}..{acr_end_page}) + WPR/shadow[{shadow_page}..{wpr_end_page})), rest SYS_MEM"
        ));
    } else {
        notes.push("Page tables: identity-mapped VA 0..2MiB → IOVA (SYS_MEM_COH)".to_string());
    }

    // High catch-all: covers from WPR end to 2 MiB boundary.
    // Prevents IOMMU faults on any VA the firmware accesses above the WPR.
    let high_start = (wpr_end_iova as usize).div_ceil(4096) * 4096;
    let two_mib: usize = 2 * 1024 * 1024;
    let _high_catch = if high_start < two_mib {
        let high_size = two_mib - high_start;
        match DmaBuffer::new(container.clone(), high_size, high_start as u64) {
            Ok(b) => {
                notes.push(format!(
                    "High catch-all: {}KiB at IOVA {high_start:#x}..{two_mib:#x}",
                    high_size / 1024
                ));
                Some(b)
            }
            Err(e) => {
                notes.push(format!("High catch-all alloc failed (non-fatal): {e}"));
                None
            }
        }
    } else {
        None
    };

    // ── Step 5: Populate SEC2 instance block ──
    // Sysmem PDB + sysmem page table chain — required for HS authentication.
    {
        let inst = inst_dma.as_mut_slice();
        let pd3_iova = sysmem_iova::PD3;
        const APER_COH: u32 = 2;
        let pdb_lo: u32 = ((pd3_iova >> 12) as u32) << 12
            | (1 << 11)
            | (1 << 10)
            | (1 << 2)
            | APER_COH;
        let pdb_hi: u32 = (pd3_iova >> 32) as u32;
        w32_le(inst, 0x200, pdb_lo);
        w32_le(inst, 0x204, pdb_hi);
        w32_le(inst, 0x208, 0xFFFF_FFFF);
        w32_le(inst, 0x20C, 0x0001_FFFF);
        w32_le(inst, 0x290, 1);
        w32_le(inst, 0x2A0, pdb_lo);
        w32_le(inst, 0x2A4, pdb_hi);
        notes.push(format!(
            "Instance block: PDB_LO={pdb_lo:#010x} PDB_HI={pdb_hi:#010x} at IOVA {:#x}",
            sysmem_iova::INST
        ));
    }

    // ── Step 5b: VRAM-native page tables for HS-mode MMU walker ──
    // The falcon's MMU page walker can't reach sysmem in HS mode.
    // Build a complete VRAM-aperture page table chain at the same addresses
    // and bind to VRAM so both LS and HS modes use the VRAM page tables.
    if !skip_blob_dma {
        let vram_pde = |addr: u64| -> u64 {
            const FLAGS: u64 = (0 << 1) | (1 << 3); // VRAM aperture + VOL
            (addr >> 4) | FLAGS
        };
        let vram_pd0_pde = |addr: u64| -> u64 {
            vram_pde(addr) | (1 << 4) // SPT_PRESENT
        };
        let vram_pte_fn = |phys: u64| -> u64 {
            const FLAGS: u64 = 1 | (0 << 1) | (1 << 3); // VALID + VRAM + VOL
            (phys >> 4) | FLAGS
        };

        let mut vr_pd3 = vec![0u8; 4096];
        vr_pd3[8..16].copy_from_slice(&vram_pde(sysmem_iova::PD2).to_le_bytes());
        let pd3_ok = vram_mirror(sysmem_iova::PD3 as u32, &vr_pd3);

        let mut vr_pd2 = vec![0u8; 4096];
        vr_pd2[8..16].copy_from_slice(&vram_pde(sysmem_iova::PD1).to_le_bytes());
        let pd2_ok = vram_mirror(sysmem_iova::PD2 as u32, &vr_pd2);

        let mut vr_pd1 = vec![0u8; 4096];
        vr_pd1[8..16].copy_from_slice(&vram_pde(sysmem_iova::PD0).to_le_bytes());
        let pd1_ok = vram_mirror(sysmem_iova::PD1 as u32, &vr_pd1);

        let mut vr_pd0 = vec![0u8; 4096];
        vr_pd0[8..16].copy_from_slice(&vram_pd0_pde(sysmem_iova::PT0).to_le_bytes());
        let pd0_ok = vram_mirror(sysmem_iova::PD0 as u32, &vr_pd0);

        let mut vr_pt = vec![0u8; 4096];
        for i in 0..512usize {
            let phys = (i as u64) * 4096;
            let off = i * 8;
            vr_pt[off..off + 8].copy_from_slice(&vram_pte_fn(phys).to_le_bytes());
        }
        let pt0_ok = vram_mirror(sysmem_iova::PT0 as u32, &vr_pt);

        let mut vr_inst = vec![0u8; 4096];
        let pd3_addr = sysmem_iova::PD3;
        let vram_pdb_lo: u32 = ((pd3_addr >> 12) as u32) << 12
            | (1 << 11)  // BIG_PAGE_SIZE
            | (1 << 10)  // USE_VER2_PT_FORMAT
            ; // aperture bits[1:0] = 0 = VRAM
        w32_le(&mut vr_inst, 0x200, vram_pdb_lo);
        w32_le(&mut vr_inst, 0x204, 0u32);
        w32_le(&mut vr_inst, 0x208, 0xFFFF_FFFFu32);
        w32_le(&mut vr_inst, 0x20C, 0x0001_FFFFu32);
        w32_le(&mut vr_inst, 0x290, 1u32);
        w32_le(&mut vr_inst, 0x2A0, vram_pdb_lo);
        w32_le(&mut vr_inst, 0x2A4, 0u32);
        let inst_ok = vram_mirror(sysmem_iova::INST as u32, &vr_inst);

        let all_ok = inst_ok && pd3_ok && pd2_ok && pd1_ok && pd0_ok && pt0_ok;
        notes.push(format!(
            "VRAM PT chain: PDB_LO={vram_pdb_lo:#010x} inst={inst_ok} pd3={pd3_ok} pd2={pd2_ok} pd1={pd1_ok} pd0={pd0_ok} pt0={pt0_ok} ALL={all_ok}"
        ));
    }

    // ── Step 6: Full Nouveau-style SEC2 reset (gm200_flcn_disable + gm200_flcn_enable) ──
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
    // Wait for mem scrubbing (gm200_flcn_reset_wait_mem_scrubbing)
    let _ = bar0.read_u32(base + falcon::MAILBOX0); // clear stale value before scrub wait
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

    // ── Step 7: Bind instance block (exact Nouveau gm200_flcn_fw_load sequence) ──
    // Nouveau does NOT touch 0x624 or DMACTL in the instance-block path.
    // Instead it: enable ITFEN → bind_inst → poll bind_stat → clear IRQ → set IRQ mask → poll idle.

    let itfen = r(falcon::ITFEN);
    w(falcon::ITFEN, (itfen & !0x01) | 0x01);
    notes.push(format!("ITFEN: {itfen:#010x} → {:#010x}", r(falcon::ITFEN)));

    // 7b. Full nouveau-style bind: DMAIDX → bind_inst → UNK090 → ENG_CONTROL → poll
    // Exp 103: Bind to sysmem (target=2) always — matches nouveau's approach.
    // The LS-mode BL needs sysmem page tables for code loading, and binding
    // to sysmem is the proven path for achieving HS mode.
    let bind_target = 2u32;
    let inst_bind_val = instance_block::encode_bind_inst(sysmem_iova::INST, bind_target);
    notes.push(format!(
        "Binding: target={} addr={:#x}",
        if bind_target == 0 { "VRAM" } else { "SYS_MEM" },
        sysmem_iova::INST
    ));
    let (bind_ok_ctx, bind_notes) =
        instance_block::falcon_bind_context(&|off| r(off), &|off, val| w(off, val), inst_bind_val);
    for n in &bind_notes {
        notes.push(n.clone());
    }

    let bind_ok = bind_ok_ctx;
    notes.push(format!(
        "bind_stat→0: {} (0x0dc={:#010x})",
        if bind_ok {
            "OK (via falcon_bind_context)"
        } else {
            "TIMEOUT"
        },
        r(0x0dc)
    ));

    // ── Step 7c: TLB invalidation (Exp 105 — omics alignment finding) ──
    // Nouveau performs ~54 TLB invalidations during boot. We performed zero.
    // The mmiotrace shows a 3-register sequence:
    //   0x100CB8 = PDB address (addr >> 12 << 4)
    //   0x100CEC = high 32 bits (always 0 for < 4GB)
    //   0x100CBC = trigger (0x80000005 = PAGE_ALL | HUB_ONLY | bit31)
    // Register 0x100CEC is the TLB invalidation high-address field,
    // NOT WPR2_ADDR_LO as previously assumed.
    {
        let mmu_ctrl = bar0.read_u32(0x100C80).unwrap_or(0);
        let flush_slot_avail = mmu_ctrl & 0x00FF_0000 != 0;

        let pdb_addr = sysmem_iova::INST;
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
            "TLB invalidate: PDB={pdb_inv:#010x} slot_avail={flush_slot_avail} ack={flush_ack}"
        ));
    }

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
    // Exp 103: Use default ctx_dma from firmware (VIRT=1) to match nouveau.
    let ctx_dma_val = u32::from_le_bytes(bl_desc[32..36].try_into().unwrap_or([1, 0, 0, 0]));
    falcon_dmem_upload(bar0, base, 0, &bl_desc);
    notes.push(format!(
        "BL desc: code={code_dma_base:#x} data={data_dma_base:#x} data_size={:#x} ctx_dma={ctx_dma_val}",
        parsed.load_header.data_size
    ));

    // ── Step 9b: WPR2 diagnostic reads (Exp 105 — read-only) ──
    // CRITICAL FIX: 0x100CEC is the MMU TLB invalidation high-address field,
    // NOT WPR2_ADDR_LO. Writing WPR values here corrupts TLB invalidation.
    // Nouveau writes 0 to 0x100CEC during TLB flush — never non-zero.
    // WPR2 boundaries are set by BIOS/PMU firmware, not by the host driver.
    // We read-only to observe state; the ACR descriptor in DMEM carries
    // the WPR boundaries to the firmware.
    {
        let tlb_hi = bar0.read_u32(0x100CEC).unwrap_or(0xDEAD);
        let reg_cf0 = bar0.read_u32(0x100CF0).unwrap_or(0xDEAD);

        let _ = bar0.write_u32(0x100CD4, 2);
        let indexed_start = bar0.read_u32(0x100CD4).unwrap_or(0xDEAD);
        let _ = bar0.write_u32(0x100CD4, 3);
        let indexed_end = bar0.read_u32(0x100CD4).unwrap_or(0xDEAD);

        notes.push(format!(
            "MMU TLB hi={tlb_hi:#010x} 0xCF0={reg_cf0:#010x} (read-only, no longer writing WPR here)"
        ));
        notes.push(format!(
            "WPR2 indexed: start_raw={indexed_start:#010x} end_raw={indexed_end:#010x}"
        ));
    }

    // ── Pre-boot DMEM verification ──
    // Confirm our ACR descriptor survived into DMEM before STARTCPU.
    {
        let pre_dmem = sec2_dmem_read(bar0, 0x200, 0x70);
        let pre_nz: Vec<String> = pre_dmem
            .iter()
            .enumerate()
            .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD && w != 0xDEAD_5EC2)
            .take(6)
            .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", 0x200 + i * 4))
            .collect();
        let pre_bl = sec2_dmem_read(bar0, 0, 0x54);
        let bl_ctx_dma = pre_bl.get(8).copied().unwrap_or(0xDEAD);  // byte offset 0x20
        let bl_data_sz = pre_bl.get(18).copied().unwrap_or(0xDEAD); // byte offset 0x48
        notes.push(format!(
            "Pre-boot DMEM verify: BL[0x20]ctx_dma={bl_ctx_dma:#x} BL[0x48]data_size={bl_data_sz:#x}"
        ));
        notes.push(format!(
            "Pre-boot DMEM[0x200..0x270]: {}",
            if pre_nz.is_empty() {
                "ALL ZERO/DEAD".to_string()
            } else {
                pre_nz.join(" ")
            }
        ));
    }

    // ── Step 10: Boot SEC2 ──
    // Nouveau uses 0xDEADA5A5 as the pre-boot MAILBOX0 sentinel.
    // The ACR code writes 0 to MAILBOX0 on success.
    w(falcon::MAILBOX0, 0xdead_a5a5_u32);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, boot_addr);
    notes.push(format!(
        "BOOTVEC={boot_addr:#x} mb0=0xdeada5a5, issuing STARTCPU"
    ));

    // Pre-boot IMEM snapshot at key addresses (BL + ACR entry region)
    // Falcon v5 (GP100+) uses byte addressing: IMEMC = (1<<25) | byte_addr
    {
        // Check around 0x0500 (crash PC) — should be empty since BL loads ACR code later
        let _ = bar0.write_u32(base + 0x180, (1u32 << 25) | 0x0500);
        let mut w5 = Vec::new();
        for _ in 0..4 {
            w5.push(bar0.read_u32(base + 0x184).unwrap_or(0xDEAD));
        }
        let hex5: Vec<String> = w5.iter().enumerate()
            .map(|(i, &v)| format!("[{:#05x}]={v:#010x}", 0x500 + i * 4))
            .collect();
        notes.push(format!("Pre-boot IMEM[0x500..0x510]: {}", hex5.join(" ")));

        // Also check the BL area (near top of IMEM where we loaded BL)
        let bl_start = imem_addr;
        let _ = bar0.write_u32(base + 0x180, (1u32 << 25) | bl_start);
        let mut wbl = Vec::new();
        for _ in 0..4 {
            wbl.push(bar0.read_u32(base + 0x184).unwrap_or(0xDEAD));
        }
        let hexbl: Vec<String> = wbl.iter().enumerate()
            .map(|(i, &v)| format!("[{:#05x}]={v:#010x}", bl_start as usize + i * 4))
            .collect();
        notes.push(format!("Pre-boot IMEM BL[{bl_start:#x}..]: {}", hexbl.join(" ")));
    }

    // No FBIF override — let hardware use its natural default.
    {
        let fbif_pre = r(falcon::FBIF_TRANSCFG);
        let itfen_pre = r(falcon::ITFEN);
        let dmactl_pre = r(falcon::DMACTL);
        let irqstat_pre = r(0x008);
        notes.push(format!(
            "Pre-boot DMA: FBIF={fbif_pre:#x} ITFEN={itfen_pre:#x} DMACTL={dmactl_pre:#x} IRQSTAT={irqstat_pre:#x}"
        ));
    }
    falcon_start_cpu(bar0, base);

    // ── Step 12: Poll with PC sampling ──
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

    // ── Step 13: Diagnostics ──
    let exci = r(falcon::EXCI);
    let sctl_post = r(falcon::SCTL);
    let hs_mode = sctl_post & 0x02 != 0;
    notes.push(format!(
        "Diag: EXCI={exci:#010x} SCTL={sctl_post:#010x} HS={hs_mode}"
    ));

    // Falcon interrupt + exception register dump
    {
        let irqstat = r(0x008);
        let irqmode = r(0x00C);
        let irqmask = r(0x010);
        let debug1 = r(0x090);
        let exci_raw = r(0x148);
        notes.push(format!(
            "Falcon IRQ: IRQSTAT={irqstat:#010x} MODE={irqmode:#010x} MASK={irqmask:#010x} DEBUG1={debug1:#010x}"
        ));
        notes.push(format!(
            "EXCI raw={exci_raw:#010x}: cause[31:24]={:#04x} tracepc_cnt[23:16]={} pc_lo[15:0]={:#06x}",
            (exci_raw >> 24) & 0xFF,
            (exci_raw >> 16) & 0xFF,
            exci_raw & 0xFFFF
        ));
    }

    // TRACEPC dump
    let tidx = r(0x148);
    let nr_traces = ((tidx & 0x00FF_0000) >> 16).min(32);
    if nr_traces > 0 {
        let mut traces = Vec::new();
        for i in 0..nr_traces {
            w(0x148, i);
            let tpc = r(0x14C);
            traces.push(format!("{tpc:#06x}"));
        }
        notes.push(format!("TRACEPC[0..{nr_traces}]: {}", traces.join(" ")));
    }

    let fbif_post = r(falcon::FBIF_TRANSCFG);
    let itfen_post = r(falcon::ITFEN);
    let dma_10c = r(falcon::DMACTL);
    let dma_bind = r(SEC2_FLCN_BIND_INST);
    let dmatrfbase = r(0x110);
    let dmatrfmoffs = r(0x114);
    let dmatrfcmd = r(0x118);
    let dmatrffboffs = r(0x11C);
    let dmaidx_604 = r(0x604);
    notes.push(format!(
        "DMA state: FBIF={fbif_post:#x} ITFEN={itfen_post:#x} DMACTL={dma_10c:#x} DMAIDX_604={dmaidx_604:#x}"
    ));
    notes.push(format!(
        "DMA bind: bind_inst={dma_bind:#010x}"
    ));
    notes.push(format!(
        "DMA xfer: base={dmatrfbase:#010x} moffs={dmatrfmoffs:#010x} cmd={dmatrfcmd:#010x} fboffs={dmatrffboffs:#010x}"
    ));

    // FBHUB + GPU MMU fault diagnostics
    {
        let mmu_ctrl = bar0.read_u32(0x100C80).unwrap_or(0xDEAD);
        let mmu_fault_status = bar0.read_u32(0x100E10).unwrap_or(0xDEAD);
        let mmu_fault_lo = bar0.read_u32(0x100E14).unwrap_or(0xDEAD);
        let mmu_fault_hi = bar0.read_u32(0x100E18).unwrap_or(0xDEAD);
        let mmu_fault_alt = bar0.read_u32(0x100A2C).unwrap_or(0xDEAD);
        let mem_ctrl = bar0.read_u32(0x100804).unwrap_or(0xDEAD);
        let mem_ack = bar0.read_u32(0x100808).unwrap_or(0xDEAD);
        notes.push(format!(
            "FBHUB: MMU_CTRL={mmu_ctrl:#010x} FAULT_STATUS={mmu_fault_status:#010x} FAULT_ADDR={mmu_fault_hi:#010x}_{mmu_fault_lo:#010x}"
        ));
        notes.push(format!(
            "FBHUB: ALT_FAULT={mmu_fault_alt:#010x} MEM_CTRL={mem_ctrl:#010x} MEM_ACK={mem_ack:#010x}"
        ));
        if mmu_fault_status != 0 && mmu_fault_status != 0xDEAD {
            let fault_type = (mmu_fault_status >> 0) & 0xF;
            let client = (mmu_fault_status >> 8) & 0x7F;
            let engine = (mmu_fault_status >> 16) & 0x3F;
            notes.push(format!(
                "FBHUB fault decoded: type={fault_type} client={client:#x} engine={engine:#x} valid={}", 
                mmu_fault_status & 0x80000000 != 0
            ));
        }
    }

    // IMEM dump around crash PC (0x0500) — helps identify the failing instruction.
    // Falcon v5 uses byte addressing: IMEMC = (1<<25) | byte_addr
    {
        let _ = bar0.write_u32(base + 0x180, (1u32 << 25) | 0x04C0); // FALCON_IMEMC(0)
        let mut imem_words = Vec::new();
        for _ in 0..32 {
            imem_words.push(bar0.read_u32(base + 0x184).unwrap_or(0xDEAD)); // FALCON_IMEMD(0)
        }
        let hex: Vec<String> = imem_words.iter().enumerate().map(|(i, &w)| {
            format!("[{:#05x}]={w:#010x}", 0x4C0 + i * 4)
        }).collect();
        notes.push(format!("IMEM[0x4C0..0x540]: {}", hex.join(" ")));
    }

    // DMEM diagnostic: BL descriptor region (0x00..0x54)
    let dmem_bl = sec2_dmem_read(bar0, 0x00, 0x54);
    let bl_vals: Vec<String> = dmem_bl
        .iter()
        .enumerate()
        .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
        .take(8)
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", i * 4))
        .collect();
    notes.push(format!(
        "DMEM[0x00..0x54] (BL desc): {}",
        if bl_vals.is_empty() { "ALL ZERO/DEAD".to_string() } else { bl_vals.join(" ") }
    ));

    // DMEM diagnostic: ACR descriptor region (0x200..0x270)
    let dmem_acr = sec2_dmem_read(bar0, 0x200, 0x70);
    let acr_vals: Vec<String> = dmem_acr
        .iter()
        .enumerate()
        .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD && w != 0xDEAD_5EC2)
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", 0x200 + i * 4))
        .collect();
    notes.push(format!(
        "DMEM[0x200..0x270] (ACR desc): {}",
        if acr_vals.is_empty() {
            "ALL ZERO/DEAD/5EC2".to_string()
        } else {
            acr_vals.join(" ")
        }
    ));
    // Raw hex dump of first 8 words for pattern analysis
    let raw8: Vec<String> = dmem_acr.iter().take(8).map(|w| format!("{w:#010x}")).collect();
    notes.push(format!("DMEM[0x200] raw: {}", raw8.join(" ")));

    // Check WPR header status in both DMA buffers — did ACR modify them?
    {
        let wpr_slice = wpr_dma.as_mut_slice();
        let fecs_status =
            u32::from_le_bytes([wpr_slice[20], wpr_slice[21], wpr_slice[22], wpr_slice[23]]);
        let gpccs_status =
            u32::from_le_bytes([wpr_slice[44], wpr_slice[45], wpr_slice[46], wpr_slice[47]]);
        let shadow_slice = shadow_dma.as_mut_slice();
        let sf = u32::from_le_bytes([
            shadow_slice[20],
            shadow_slice[21],
            shadow_slice[22],
            shadow_slice[23],
        ]);
        let sg = u32::from_le_bytes([
            shadow_slice[44],
            shadow_slice[45],
            shadow_slice[46],
            shadow_slice[47],
        ]);
        notes.push(format!(
            "WPR FECS={fecs_status} GPCCS={gpccs_status} | Shadow FECS={sf} GPCCS={sg} (1=copy, 0xFF=done)"
        ));
    }

    // MMU TLB state after boot (0x100CEC = TLB invalidation hi-addr, not WPR2)
    {
        let tlb_hi = bar0.read_u32(0x100CEC).unwrap_or(0);
        let reg_cf0 = bar0.read_u32(0x100CF0).unwrap_or(0);
        notes.push(format!("MMU post-boot: TLB_hi={tlb_hi:#010x} 0xCF0={reg_cf0:#010x}"));
    }

    // Nouveau checks: MAILBOX0 == 0 means ACR load succeeded.
    let mb0_final = r(falcon::MAILBOX0);
    let mb1_final = r(falcon::MAILBOX1);
    let acr_success = mb0_final == 0;
    notes.push(format!(
        "ACR result: mb0={mb0_final:#010x} mb1={mb1_final:#010x} success={acr_success}"
    ));

    // ── SEC2 Conversation: try queue discovery + BOOTSTRAP_FALCON ──
    super::sec2_queue::probe_and_bootstrap(bar0, &mut notes);

    // ── Capture final state ──
    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);
    notes.push(format!(
        "Final: FECS cpuctl={:#010x} pc={:#06x} exci={:#010x} GPCCS cpuctl={:#010x} pc={:#06x} exci={:#010x}",
        post.fecs_cpuctl, post.fecs_pc, post.fecs_exci,
        post.gpccs_cpuctl, post.gpccs_pc, post.gpccs_exci
    ));

    post.into_result(
        "System-memory ACR boot (IOMMU DMA)",
        sec2_before,
        sec2_after,
        notes,
    )
}

/// Sysmem physical DMA boot: DMA buffers in host memory, no instance block.
///
/// Combines the sysmem strategy's DMA allocation with the VRAM strategy's
/// simple boot flow (engine reset → IMEM → EMEM → STARTCPU). Avoids the
/// instance block binding that breaks the boot on clean-reset SEC2.
///
/// The falcon's physical DMA is routed to system memory via
/// `FBIF_TRANSCFG = 0x93` (PHYS_SYS target + physical override).
/// DMA addresses are IOVAs — the IOMMU translates them to host physical.
pub fn attempt_sysmem_physical_boot(
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
    let parsed = match super::firmware::ParsedAcrFirmware::parse(fw) {
        Ok(p) => p,
        Err(e) => {
            notes.push(format!("Firmware parse failed: {e}"));
            return make_fail_result("SysMemPhys: parse failed", sec2_before, bar0, notes);
        }
    };
    notes.push(format!(
        "ACR payload: {}B code=[{:#x}+{:#x}] data=[{:#x}+{:#x}]",
        parsed.acr_payload.len(),
        parsed.load_header.non_sec_code_off,
        parsed.load_header.non_sec_code_size,
        parsed.load_header.data_dma_base,
        parsed.load_header.data_size,
    ));

    // ── Step 2: Allocate DMA buffers in host memory ──
    let acr_payload_size = parsed.acr_payload.len().div_ceil(4096) * 4096;
    let mut acr_dma = match DmaBuffer::new(
        container.clone(),
        acr_payload_size.max(4096),
        sysmem_iova::ACR,
    ) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc ACR failed: {e}"));
            return make_fail_result("SysMemPhys: DMA alloc failed", sec2_before, bar0, notes);
        }
    };

    let wpr_base_iova = sysmem_iova::WPR;
    let wpr_data = build_wpr(fw, wpr_base_iova);
    let wpr_end_iova = wpr_base_iova + wpr_data.len() as u64;
    let wpr_buf_size = wpr_data.len().div_ceil(4096) * 4096;
    let mut wpr_dma = match DmaBuffer::new(container.clone(), wpr_buf_size.max(4096), wpr_base_iova)
    {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc WPR failed: {e}"));
            return make_fail_result("SysMemPhys: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    let shadow_iova = sysmem_iova::SHADOW;
    let mut shadow_dma =
        match DmaBuffer::new(container, wpr_buf_size.max(4096), shadow_iova) {
            Ok(b) => b,
            Err(e) => {
                notes.push(format!("DMA alloc shadow failed: {e}"));
                return make_fail_result(
                    "SysMemPhys: DMA alloc failed",
                    sec2_before,
                    bar0,
                    notes,
                );
            }
        };

    notes.push(format!(
        "DMA: ACR={:#x}({acr_payload_size:#x}) shadow={shadow_iova:#x} WPR={:#x}({wpr_buf_size:#x})",
        sysmem_iova::ACR, sysmem_iova::WPR,
    ));

    // ── Step 3: Populate WPR + shadow + patch ACR descriptor ──
    wpr_dma.as_mut_slice()[..wpr_data.len()].copy_from_slice(&wpr_data);
    shadow_dma.as_mut_slice()[..wpr_data.len()].copy_from_slice(&wpr_data);

    let mut payload_patched = parsed.acr_payload.to_vec();
    let data_off = parsed.load_header.data_dma_base as usize;
    patch_acr_desc(
        &mut payload_patched,
        data_off,
        wpr_base_iova,
        wpr_end_iova,
        shadow_iova,
    );
    acr_dma.as_mut_slice()[..payload_patched.len()].copy_from_slice(&payload_patched);

    notes.push(format!(
        "ACR patched: wpr=[{wpr_base_iova:#x}..{wpr_end_iova:#x}] shadow={shadow_iova:#x}",
    ));

    // ── Step 4: Engine reset + FBIF for sysmem physical DMA ──
    let (reset_ok, reset_notes) = sec2_prepare_physical_first(bar0);
    for n in &reset_notes {
        notes.push(n.clone());
    }
    notes.push(format!("Engine reset: ok={reset_ok}"));

    // PHYS_SYS(0x03) | bit4(0x10) | PHYSICAL_OVERRIDE(0x80) = 0x93
    const FBIF_PHYS_SYS: u32 = 0x93;
    w(falcon::FBIF_TRANSCFG, FBIF_PHYS_SYS);
    let fbif_readback = r(falcon::FBIF_TRANSCFG);
    w(falcon::DMACTL, 0x01);
    let dmactl_readback = r(falcon::DMACTL);
    notes.push(format!(
        "FBIF=PHYS_SYS: wrote={FBIF_PHYS_SYS:#x} read={fbif_readback:#x} DMACTL={dmactl_readback:#x}"
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
        "BL: {}B→IMEM@{imem_addr:#x} tag={start_tag:#x} boot={boot_addr:#x}",
        parsed.bl_code.len()
    ));

    // ── Step 5b: Load BL data → EMEM ──
    let bl_payload = fw.acr_bl_parsed.payload(&fw.acr_bl_raw);
    let bl_data_off = parsed.bl_desc.bl_data_off as usize;
    let bl_data_size = parsed.bl_desc.bl_data_size as usize;
    let bl_data_end = (bl_data_off + bl_data_size).min(bl_payload.len());
    if bl_data_off < bl_payload.len() && bl_data_size > 0 {
        let bl_data = &bl_payload[bl_data_off..bl_data_end];
        sec2_emem_write(bar0, 0, bl_data);
        notes.push(format!("BL data: {}B→EMEM@0", bl_data.len()));
    }

    // ── Step 6: Pre-load ACR data + BL descriptor to DMEM ──
    let data_section = &payload_patched[parsed.load_header.data_dma_base as usize..];
    falcon_dmem_upload(bar0, base, 0, data_section);

    let code_dma_base = sysmem_iova::ACR;
    let data_dma_base =
        sysmem_iova::ACR + parsed.load_header.data_dma_base as u64;
    let mut bl_desc = super::wpr::build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    // ctx_dma=PHYS(0) — BL must DMA through physical index 0.
    bl_desc[32..36].copy_from_slice(&0u32.to_le_bytes());
    let dmem_load_off = parsed.bl_desc.bl_desc_dmem_load_off;
    falcon_dmem_upload(bar0, base, dmem_load_off, &bl_desc);

    notes.push(format!(
        "BL desc→DMEM@{dmem_load_off:#x}: code={code_dma_base:#x} data={data_dma_base:#x} ctx_dma=PHYS"
    ));

    // ── Step 7: Clear EXCI + boot SEC2 ──
    let exci_pre = r(falcon::EXCI);
    w(falcon::EXCI, 0);
    w(falcon::MAILBOX0, 0xcafe_beef_u32);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, boot_addr);
    let cpuctl_pre = r(falcon::CPUCTL);
    notes.push(format!(
        "Pre-start: EXCI was={exci_pre:#x} BOOTVEC={boot_addr:#x} cpuctl={cpuctl_pre:#010x}"
    ));
    falcon_start_cpu(bar0, base);

    // Re-apply FBIF during BL execution (BL or ROM may clear it)
    for _ in 0..200 {
        w(falcon::FBIF_TRANSCFG, FBIF_PHYS_SYS);
        std::thread::sleep(std::time::Duration::from_micros(50));
    }

    // ── Step 8: Poll for completion ──
    let timeout = std::time::Duration::from_secs(5);
    let start_time = std::time::Instant::now();
    let mut pc_samples: Vec<String> = Vec::new();
    let mut last_pc = 0u32;

    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let mb1 = r(falcon::MAILBOX1);
        let pc = r(falcon::PC);

        if pc != last_pc {
            pc_samples.push(format!("{pc:#06x}@{}ms", start_time.elapsed().as_millis()));
            last_pc = pc;
        }

        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;
        let hreset = cpuctl & falcon::CPUCTL_HRESET != 0;
        let mb0_changed = mb0 != 0xcafe_beef && mb0 != 0;

        if mb0_changed || halted || hreset {
            notes.push(format!(
                "SEC2 done: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} pc={pc:#06x} ({}ms)",
                start_time.elapsed().as_millis()
            ));
            break;
        }
        if start_time.elapsed() > timeout {
            notes.push(format!(
                "SEC2 timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} pc={pc:#06x}"
            ));
            break;
        }
    }

    if !pc_samples.is_empty() {
        notes.push(format!("PC trace: [{}]", pc_samples.join(", ")));
    }

    // Diagnostics
    let exci = r(falcon::EXCI);
    let fbif_post = r(falcon::FBIF_TRANSCFG);
    let dmactl_post = r(falcon::DMACTL);
    notes.push(format!(
        "Diag: EXCI={exci:#010x} FBIF={fbif_post:#x} DMACTL={dmactl_post:#x}"
    ));

    // ── SEC2 Conversation probe ──
    super::sec2_queue::probe_and_bootstrap(bar0, &mut notes);

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::boot_result::PostBootCapture::capture(bar0);
    notes.push(format!(
        "Final: FECS cpuctl={:#010x} GPCCS cpuctl={:#010x}",
        post.fecs_cpuctl, post.gpccs_cpuctl,
    ));

    post.into_result(
        "System-memory physical DMA boot",
        sec2_before,
        sec2_after,
        notes,
    )
}
