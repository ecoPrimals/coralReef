// SPDX-License-Identifier: AGPL-3.0-or-later

//! Early probe, firmware parse, and DMA buffer allocation for sysmem ACR boot.

use crate::vfio::device::{DmaBackend, MappedBar};
use crate::vfio::dma::DmaBuffer;
use crate::vfio::memory::{MemoryRegion, PraminRegion};

use super::super::boot_result::{AcrBootResult, make_fail_result};
use super::super::firmware::{AcrFirmwareSet, ParsedAcrFirmware};
use super::super::sec2_hal::Sec2Probe;
use super::super::sysmem_iova;
use super::super::wpr::build_wpr;
use super::sysmem_state::SysmemDmaState;

/// VRAM sentinel probe + memory controller register snapshot.
pub(super) fn probe_vram_and_mc(bar0: &MappedBar, notes: &mut Vec<String>) {
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

#[expect(
    clippy::result_large_err,
    reason = "AcrBootResult is the intentional early-exit payload"
)]
pub(super) fn parse_acr_firmware(
    fw: &AcrFirmwareSet,
    notes: &mut Vec<String>,
    sec2_before: &Sec2Probe,
    bar0: &MappedBar,
) -> Result<ParsedAcrFirmware, AcrBootResult> {
    match ParsedAcrFirmware::parse(fw) {
        Ok(p) => Ok(p),
        Err(e) => {
            notes.push(format!("Firmware parse failed: {e}"));
            Err(make_fail_result(
                "SysMem ACR: parse failed",
                sec2_before.clone(),
                bar0,
                std::mem::take(notes),
            ))
        }
    }
}

/// Allocates the low catch-all, page-directory chain, ACR/WPR/shadow buffers, and IOVA gap fillers.
#[expect(
    clippy::result_large_err,
    reason = "AcrBootResult is the intentional early-exit payload"
)]
pub(super) fn allocate_dma(
    container: DmaBackend,
    fw: &AcrFirmwareSet,
    parsed: &ParsedAcrFirmware,
    notes: &mut Vec<String>,
    sec2_before: &Sec2Probe,
    bar0: &MappedBar,
) -> Result<SysmemDmaState, AcrBootResult> {
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
                    return Err(make_fail_result(
                        "SysMem ACR: low catch DMA alloc failed",
                        sec2_before.clone(),
                        bar0,
                        std::mem::take(notes),
                    ));
                }
            }
        }
    };

    let inst_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::INST) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc inst failed: {e}"));
            return Err(make_fail_result(
                "SysMem ACR: DMA alloc failed",
                sec2_before.clone(),
                bar0,
                std::mem::take(notes),
            ));
        }
    };
    let pd3_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::PD3) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc PD3 failed: {e}"));
            return Err(make_fail_result(
                "SysMem ACR: DMA alloc failed",
                sec2_before.clone(),
                bar0,
                std::mem::take(notes),
            ));
        }
    };
    let pd2_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::PD2) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc PD2 failed: {e}"));
            return Err(make_fail_result(
                "SysMem ACR: DMA alloc failed",
                sec2_before.clone(),
                bar0,
                std::mem::take(notes),
            ));
        }
    };
    let pd1_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::PD1) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc PD1 failed: {e}"));
            return Err(make_fail_result(
                "SysMem ACR: DMA alloc failed",
                sec2_before.clone(),
                bar0,
                std::mem::take(notes),
            ));
        }
    };
    let pd0_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::PD0) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc PD0 failed: {e}"));
            return Err(make_fail_result(
                "SysMem ACR: DMA alloc failed",
                sec2_before.clone(),
                bar0,
                std::mem::take(notes),
            ));
        }
    };
    let pt0_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::PT0) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc PT0 failed: {e}"));
            return Err(make_fail_result(
                "SysMem ACR: DMA alloc failed",
                sec2_before.clone(),
                bar0,
                std::mem::take(notes),
            ));
        }
    };

    let acr_payload_size = parsed.acr_payload.len().div_ceil(4096) * 4096;
    let acr_dma = match DmaBuffer::new(
        container.clone(),
        acr_payload_size.max(4096),
        sysmem_iova::ACR,
    ) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc ACR payload failed: {e}"));
            return Err(make_fail_result(
                "SysMem ACR: DMA alloc failed",
                sec2_before.clone(),
                bar0,
                std::mem::take(notes),
            ));
        }
    };

    let wpr_base_iova = sysmem_iova::WPR;
    let wpr_data = build_wpr(fw, wpr_base_iova);
    let wpr_end_iova = wpr_base_iova + wpr_data.len() as u64;
    let wpr_buf_size = wpr_data.len().div_ceil(4096) * 4096;
    let wpr_dma = match DmaBuffer::new(container.clone(), wpr_buf_size.max(4096), wpr_base_iova) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc WPR failed: {e}"));
            return Err(make_fail_result(
                "SysMem ACR: DMA alloc failed",
                sec2_before.clone(),
                bar0,
                std::mem::take(notes),
            ));
        }
    };

    let shadow_iova = sysmem_iova::SHADOW;
    let shadow_dma = match DmaBuffer::new(container.clone(), wpr_buf_size.max(4096), shadow_iova) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc shadow failed: {e}"));
            return Err(make_fail_result(
                "SysMem ACR: DMA alloc failed",
                sec2_before.clone(),
                bar0,
                std::mem::take(notes),
            ));
        }
    };

    notes.push(format!(
        "DMA buffers: inst={:#x} PD3={:#x} ACR={:#x}({acr_payload_size:#x}) shadow={shadow_iova:#x} WPR={:#x}({wpr_buf_size:#x})",
        sysmem_iova::INST, sysmem_iova::PD3, sysmem_iova::ACR, sysmem_iova::WPR
    ));

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

    Ok(SysmemDmaState {
        _low_catch,
        _mid_gap1,
        _mid_gap2,
        inst_dma,
        pd3_dma,
        pd2_dma,
        pd1_dma,
        pd0_dma,
        pt0_dma,
        acr_dma,
        wpr_dma,
        shadow_dma,
        wpr_data,
        acr_payload_size,
        wpr_base_iova,
        wpr_end_iova,
        shadow_iova,
        _high_catch: None,
        container,
    })
}

/// Records parsed firmware layout in `notes` (caller already logged `BootConfig`).
pub(super) fn push_firmware_layout_notes(parsed: &ParsedAcrFirmware, notes: &mut Vec<String>) {
    notes.push(format!(
        "ACR payload: {}B non_sec=[{:#x}+{:#x}] data=[{:#x}+{:#x}]",
        parsed.acr_payload.len(),
        parsed.load_header.non_sec_code_off,
        parsed.load_header.non_sec_code_size,
        parsed.load_header.data_dma_base,
        parsed.load_header.data_size,
    ));
}
