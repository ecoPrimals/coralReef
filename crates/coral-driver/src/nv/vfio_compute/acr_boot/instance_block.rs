// SPDX-License-Identifier: AGPL-3.0-only

//! VRAM instance block and falcon MMU page table helpers for ACR DMA.

use crate::vfio::device::MappedBar;
use crate::vfio::memory::{MemoryRegion, PraminRegion};

/// SEC2 falcon instance block binding register (Nouveau `gp102_sec2_flcn_bind_inst`).
pub(crate) const SEC2_FLCN_BIND_INST: usize = 0x668;

// ── VRAM Instance Block for Falcon DMA ────────────────────────────────
// VRAM addresses for the page table chain (below our ACR/WPR region)
pub(crate) const FALCON_INST_VRAM: u32 = 0x10000;
pub(crate) const FALCON_PD3_VRAM: u32 = 0x11000;
pub(crate) const FALCON_PD2_VRAM: u32 = 0x12000;
pub(crate) const FALCON_PD1_VRAM: u32 = 0x13000;
pub(crate) const FALCON_PD0_VRAM: u32 = 0x14000;
pub(crate) const FALCON_PT0_VRAM: u32 = 0x15000;

pub(crate) fn encode_vram_pde(vram_addr: u64) -> u64 {
    const APER_VRAM: u64 = 1 << 1; // bits[2:1] = 1 = VRAM
    (vram_addr >> 4) | APER_VRAM
}

pub(crate) fn encode_vram_pd0_pde(vram_addr: u64) -> u64 {
    const SPT_PRESENT: u64 = 1 << 4;
    encode_vram_pde(vram_addr) | SPT_PRESENT
}

pub(crate) fn encode_vram_pte(vram_phys: u64) -> u64 {
    const VALID: u64 = 1; // bit[0] = VALID, bits[2:1] = 0 = VRAM aperture
    (vram_phys >> 4) | VALID
}

/// Encode a PTE pointing to system memory (SYS_MEM_COH) for the hybrid VRAM
/// page table approach. VRAM PDEs walk the page table chain in VRAM, but leaf
/// PTEs point to IOMMU-mapped system memory where the ACR/WPR data lives.
pub(crate) fn encode_sysmem_pte(iova: u64) -> u64 {
    const FLAGS: u64 = 1 | (2 << 1) | (1 << 3); // VALID + SYS_MEM_COH + VOL
    (iova >> 4) | FLAGS
}

/// Build a minimal VRAM-based instance block with identity-mapped page tables.
/// Returns true if successful. Maps first 2MB of VRAM so falcon DMA can
/// access VRAM addresses 0x0..0x200000 (covers our ACR payload + WPR).
pub fn build_vram_falcon_inst_block(bar0: &MappedBar) -> bool {
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

    // PD3[0] → PD2
    if !wv64(FALCON_PD3_VRAM, 0, encode_vram_pde(FALCON_PD2_VRAM as u64)) {
        return false;
    }
    // PD2[0] → PD1
    if !wv64(FALCON_PD2_VRAM, 0, encode_vram_pde(FALCON_PD1_VRAM as u64)) {
        return false;
    }
    // PD1[0] → PD0
    if !wv64(FALCON_PD1_VRAM, 0, encode_vram_pde(FALCON_PD0_VRAM as u64)) {
        return false;
    }
    // PD0[0] → PT0 (dual PDE format: small PDE at bytes 0-7)
    if !wv64(
        FALCON_PD0_VRAM,
        0,
        encode_vram_pd0_pde(FALCON_PT0_VRAM as u64),
    ) {
        return false;
    }

    // PT0: identity-map 512 small pages (4KiB each = 2MiB total)
    for i in 1u64..512 {
        let phys = i * 4096;
        let pte = encode_vram_pte(phys);
        if !wv64(FALCON_PT0_VRAM, (i as usize) * 8, pte) {
            return false;
        }
    }

    // Instance block: PAGE_DIR_BASE at RAMIN offset 0x200
    let pdb_lo: u32 = ((FALCON_PD3_VRAM >> 12) << 12)
        | (1 << 11) // BIG_PAGE_SIZE = 64KiB
        | (1 << 10) // USE_VER2_PT_FORMAT
        ; // target bits[1:0] = 0 = VRAM, VOL=0
    if !wv(FALCON_INST_VRAM, 0x200, pdb_lo) {
        return false;
    }
    if !wv(FALCON_INST_VRAM, 0x204, 0) {
        return false;
    }

    // VA limit = 128TB
    if !wv(FALCON_INST_VRAM, 0x208, 0xFFFF_FFFF) {
        return false;
    }
    if !wv(FALCON_INST_VRAM, 0x20C, 0x0001_FFFF) {
        return false;
    }

    // Verify: read back key entries
    let rv = |vram_addr: u32, offset: usize| -> u32 {
        PraminRegion::new(bar0, vram_addr, offset + 4)
            .ok()
            .and_then(|r| r.read_u32(offset).ok())
            .unwrap_or(0xDEAD)
    };
    let pdb_rb = rv(FALCON_INST_VRAM, 0x200);
    let pd3_0 = rv(FALCON_PD3_VRAM, 0);
    let pd3_4 = rv(FALCON_PD3_VRAM, 4);
    let pt0_112_lo = rv(FALCON_PT0_VRAM, 112 * 8);
    let pt0_112_hi = rv(FALCON_PT0_VRAM, 112 * 8 + 4);
    let pt0_1_lo = rv(FALCON_PT0_VRAM, 8);
    let pt0_1_hi = rv(FALCON_PT0_VRAM, 8 + 4);

    tracing::info!(
        pdb_lo = format!("{pdb_lo:#010x}"),
        pdb_rb = format!("{pdb_rb:#010x}"),
        pd3 = format!("{pd3_0:#010x}:{pd3_4:#010x}"),
        pt112 = format!("{pt0_112_lo:#010x}:{pt0_112_hi:#010x}"),
        pt1 = format!("{pt0_1_lo:#010x}:{pt0_1_hi:#010x}"),
        "VRAM falcon instance block built"
    );
    true
}
