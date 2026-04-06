// SPDX-License-Identifier: AGPL-3.0-or-later

//! WPR/shadow population, ACR descriptor patching, page tables, instance block, and VRAM MMU chain.

use crate::vfio::device::MappedBar;
use crate::vfio::dma::DmaBuffer;
use crate::vfio::memory::{MemoryRegion, PraminRegion};

use super::super::firmware::ParsedAcrFirmware;
use super::super::sysmem_iova;
use super::super::wpr::patch_acr_desc;
use super::boot_config::BootConfig;
use super::sysmem_state::SysmemDmaState;
use super::sysmem_vram::mirror_payload_to_vram;

fn w32_le(buf: &mut [u8], off: usize, val: u32) {
    buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
}

/// Copies WPR into DMA + shadow, mirrors to VRAM, patches the ACR payload, fills MMU tables,
/// writes the SEC2 instance block, optionally mirrors a VRAM-native PD/PT chain, and maps the high catch-all.
pub(super) fn fill_wpr_patch_acr_and_setup_mmu(
    bar0: &MappedBar,
    dma: &mut SysmemDmaState,
    parsed: &ParsedAcrFirmware,
    config: &BootConfig,
    notes: &mut Vec<String>,
    skip_blob_dma: bool,
) -> Vec<u8> {
    let wpr_base_iova = dma.wpr_base_iova;
    let wpr_end_iova = dma.wpr_end_iova;
    let shadow_iova = dma.shadow_iova;
    let acr_payload_size = dma.acr_payload_size;

    {
        let wpr = &dma.wpr_data;
        dma.wpr_dma.as_mut_slice()[..wpr.len()].copy_from_slice(wpr);
        dma.shadow_dma.as_mut_slice()[..wpr.len()].copy_from_slice(wpr);
        notes.push(format!(
            "WPR: {}B at IOVA {wpr_base_iova:#x}..{wpr_end_iova:#x} shadow={shadow_iova:#x}",
            wpr.len()
        ));

        let wpr_mirrored = mirror_payload_to_vram(bar0, wpr_base_iova as u32, wpr);
        let shadow_mirrored = mirror_payload_to_vram(bar0, shadow_iova as u32, wpr);
        notes.push(format!(
            "VRAM mirror: WPR={wpr_mirrored} shadow={shadow_mirrored}"
        ));
    }
    let mut payload_patched = parsed.acr_payload.to_vec();
    let data_off = parsed.load_header.data_dma_base as usize;

    if data_off + 0x268 <= payload_patched.len() {
        let fw_blob_size = u32::from_le_bytes(
            payload_patched[data_off + 0x258..data_off + 0x25C]
                .try_into()
                .unwrap_or([0; 4]),
        );
        let fw_blob_base = u64::from_le_bytes(
            payload_patched[data_off + 0x260..data_off + 0x268]
                .try_into()
                .unwrap_or([0; 8]),
        );
        let fw_wpr_region_id = u32::from_le_bytes(
            payload_patched[data_off + 0x210..data_off + 0x214]
                .try_into()
                .unwrap_or([0; 4]),
        );
        let fw_no_regions = u32::from_le_bytes(
            payload_patched[data_off + 0x21C..data_off + 0x220]
                .try_into()
                .unwrap_or([0; 4]),
        );
        notes.push(format!(
            "FW original ACR desc: blob_size={fw_blob_size:#x} blob_base={fw_blob_base:#x} wpr_region_id={fw_wpr_region_id} no_regions={fw_no_regions}"
        ));
    }

    patch_acr_desc(
        &mut payload_patched,
        data_off,
        wpr_base_iova,
        wpr_end_iova,
        shadow_iova,
    );

    if payload_patched.len() > 0x510 {
        let fw_at_500: Vec<String> = (0x500..0x510)
            .step_by(4)
            .map(|off| {
                let w =
                    u32::from_le_bytes(payload_patched[off..off + 4].try_into().unwrap_or([0; 4]));
                format!("[{off:#05x}]={w:#010x}")
            })
            .collect();
        notes.push(format!("FW code @0x500: {}", fw_at_500.join(" ")));
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
            notes.push(format!(
                "FW code @HS_entry({hs_entry_off:#x}): {}",
                fw_at_hs.join(" ")
            ));
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
    dma.acr_dma.as_mut_slice()[..payload_patched.len()].copy_from_slice(&payload_patched);
    notes.push(format!(
        "ACR desc patched: wpr=[{wpr_base_iova:#x}..{wpr_end_iova:#x}] shadow={shadow_iova:#x}"
    ));

    if !skip_blob_dma {
        let acr_mirrored = mirror_payload_to_vram(bar0, sysmem_iova::ACR as u32, &payload_patched);
        notes.push(format!(
            "VRAM mirror ACR: {acr_mirrored} ({}B at VRAM {:#x})",
            payload_patched.len(),
            sysmem_iova::ACR
        ));

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
            let x0 = u32::from_le_bytes(dma.wpr_data[0..4].try_into().unwrap_or([0; 4]));
            let x4 = u32::from_le_bytes(dma.wpr_data[4..8].try_into().unwrap_or([0; 4]));
            notes.push(format!(
                "VRAM WPR readback: [{:#x}]={w0:#010x}(expect {x0:#010x}) +4={w4:#010x}(expect {x4:#010x}) match={}",
                sysmem_iova::WPR, w0 == x0 && w4 == x4
            ));
        }
    }

    let sysmem_pde = |iova: u64| -> u64 {
        const FLAGS: u64 = (2 << 1) | (1 << 3);
        (iova >> 4) | FLAGS
    };
    let sysmem_pd0_pde = |iova: u64| -> u64 { sysmem_pde(iova) | (1 << 4) };
    let sysmem_pte = |phys: u64| -> u64 {
        const FLAGS: u64 = 1 | (2 << 1) | (1 << 3);
        (phys >> 4) | FLAGS
    };

    let (lo_range, hi_range): (std::ops::Range<usize>, std::ops::Range<usize>) = if config.pde_upper
    {
        (0..8, 8..16)
    } else {
        (8..16, 0..8)
    };

    let pde3 = sysmem_pde(sysmem_iova::PD2);
    dma.pd3_dma.as_mut_slice()[lo_range.clone()].copy_from_slice(&0u64.to_le_bytes());
    dma.pd3_dma.as_mut_slice()[hi_range.clone()].copy_from_slice(&pde3.to_le_bytes());

    let pde2 = sysmem_pde(sysmem_iova::PD1);
    dma.pd2_dma.as_mut_slice()[lo_range.clone()].copy_from_slice(&0u64.to_le_bytes());
    dma.pd2_dma.as_mut_slice()[hi_range.clone()].copy_from_slice(&pde2.to_le_bytes());

    let pde1 = sysmem_pde(sysmem_iova::PD0);
    dma.pd1_dma.as_mut_slice()[lo_range.clone()].copy_from_slice(&0u64.to_le_bytes());
    dma.pd1_dma.as_mut_slice()[hi_range.clone()].copy_from_slice(&pde1.to_le_bytes());

    let pde0 = sysmem_pd0_pde(sysmem_iova::PT0);
    dma.pd0_dma.as_mut_slice()[lo_range].copy_from_slice(&0u64.to_le_bytes());
    dma.pd0_dma.as_mut_slice()[hi_range].copy_from_slice(&pde0.to_le_bytes());

    let vram_pte = |phys: u64| -> u64 {
        const VALID: u64 = 1;
        const VOL: u64 = 1 << 3;
        const FLAGS: u64 = VALID | VOL;
        (phys >> 4) | FLAGS
    };

    let pt = dma.pt0_dma.as_mut_slice();
    let acr_start_page = (sysmem_iova::ACR as usize) / 4096;
    let acr_end_page = (sysmem_iova::ACR as usize + acr_payload_size).div_ceil(4096);
    let shadow_page = (shadow_iova as usize) / 4096;
    let wpr_end_page = (wpr_end_iova as usize).div_ceil(4096);
    let mut vram_pages = 0u32;
    for i in 0..512usize {
        let phys = (i as u64) * 4096;
        let use_vram = config.acr_vram_pte && i >= acr_start_page && i < acr_end_page;
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

    let high_start = (wpr_end_iova as usize).div_ceil(4096) * 4096;
    let two_mib: usize = 2 * 1024 * 1024;
    dma._high_catch = if high_start < two_mib {
        let high_size = two_mib - high_start;
        match DmaBuffer::new(dma.container.clone(), high_size, high_start as u64) {
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

    {
        let inst = dma.inst_dma.as_mut_slice();
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
        notes.push(format!(
            "Instance block: PDB_LO={pdb_lo:#010x} PDB_HI={pdb_hi:#010x} at IOVA {:#x}",
            sysmem_iova::INST
        ));
    }

    if !skip_blob_dma {
        let vram_pde = |addr: u64| -> u64 {
            const VOL: u64 = 1 << 3;
            const FLAGS: u64 = VOL;
            (addr >> 4) | FLAGS
        };
        let vram_pd0_pde = |addr: u64| -> u64 { vram_pde(addr) | (1 << 4) };
        let vram_pte_fn = |phys: u64| -> u64 {
            const VALID: u64 = 1;
            const VOL: u64 = 1 << 3;
            const FLAGS: u64 = VALID | VOL;
            (phys >> 4) | FLAGS
        };

        let mut vr_pd3 = vec![0u8; 4096];
        vr_pd3[8..16].copy_from_slice(&vram_pde(sysmem_iova::PD2).to_le_bytes());
        let pd3_ok = mirror_payload_to_vram(bar0, sysmem_iova::PD3 as u32, &vr_pd3);

        let mut vr_pd2 = vec![0u8; 4096];
        vr_pd2[8..16].copy_from_slice(&vram_pde(sysmem_iova::PD1).to_le_bytes());
        let pd2_ok = mirror_payload_to_vram(bar0, sysmem_iova::PD2 as u32, &vr_pd2);

        let mut vr_pd1 = vec![0u8; 4096];
        vr_pd1[8..16].copy_from_slice(&vram_pde(sysmem_iova::PD0).to_le_bytes());
        let pd1_ok = mirror_payload_to_vram(bar0, sysmem_iova::PD1 as u32, &vr_pd1);

        let mut vr_pd0 = vec![0u8; 4096];
        vr_pd0[8..16].copy_from_slice(&vram_pd0_pde(sysmem_iova::PT0).to_le_bytes());
        let pd0_ok = mirror_payload_to_vram(bar0, sysmem_iova::PD0 as u32, &vr_pd0);

        let mut vr_pt = vec![0u8; 4096];
        for i in 0..512usize {
            let phys = (i as u64) * 4096;
            let off = i * 8;
            vr_pt[off..off + 8].copy_from_slice(&vram_pte_fn(phys).to_le_bytes());
        }
        let pt0_ok = mirror_payload_to_vram(bar0, sysmem_iova::PT0 as u32, &vr_pt);

        let mut vr_inst = vec![0u8; 4096];
        let pd3_addr = sysmem_iova::PD3;
        let vram_pdb_lo: u32 = ((pd3_addr >> 12) as u32) << 12 | (1 << 11) | (1 << 10) | (1 << 2);
        w32_le(&mut vr_inst, 0x200, vram_pdb_lo);
        w32_le(&mut vr_inst, 0x204, 0u32);
        w32_le(&mut vr_inst, 0x208, 0xFFFF_FFFFu32);
        w32_le(&mut vr_inst, 0x20C, 0x0001_FFFFu32);
        w32_le(&mut vr_inst, 0x290, 1u32);
        w32_le(&mut vr_inst, 0x2A0, vram_pdb_lo);
        w32_le(&mut vr_inst, 0x2A4, 0u32);
        let inst_ok = mirror_payload_to_vram(bar0, sysmem_iova::INST as u32, &vr_inst);

        let all_ok = inst_ok && pd3_ok && pd2_ok && pd1_ok && pd0_ok && pt0_ok;
        notes.push(format!(
            "VRAM PT chain: PDB_LO={vram_pdb_lo:#010x} inst={inst_ok} pd3={pd3_ok} pd2={pd2_ok} pd1={pd1_ok} pd0={pd0_ok} pt0={pt0_ok} ALL={all_ok}"
        ));
    }

    payload_patched
}
