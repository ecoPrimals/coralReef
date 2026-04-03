// SPDX-License-Identifier: AGPL-3.0-only
//! GF100 V1 MMU page table encoding for Kepler (GK104/GK110/GK210).
//!
//! Implements the 2-level page table hierarchy (PDB → SPT) used by
//! GF100-family GPUs for GPU virtual address translation.
//!
//! Key differences from Volta (GP100 V2):
//! - **2 levels** (PDB + SPT) instead of 5 (PD3→PD2→PD1→PD0→PT)
//! - **PDE is dual-format**: 8 bytes with large-page PDE (lower dword)
//!   and small-page PDE (upper dword)
//! - **PTE address shift**: `addr >> 12` in bits [31:4] of a 32-bit word
//!   (vs GP100 `addr >> 4` in 64-bit word)
//! - **PTE is 8 bytes**: lower dword = address + flags, upper dword =
//!   storage type + compression (0 for uncompressed system memory)

use super::registers::*;

/// Encode a GF100 V1 small-page PDE (upper dword of the dual PDE).
///
/// Points to a Small Page Table (SPT) containing 4K page PTEs.
/// Layout: `[31:4] = spt_addr >> 12, [3] = VOL, [2:1] = target, [0] = 1 (SPT present)`
fn encode_kepler_spt_pde(spt_iova: u64) -> u32 {
    const SPT_PRESENT: u32 = 1;
    const TARGET_COH: u32 = 2 << 1;
    const VOL: u32 = 1 << 3;

    #[expect(
        clippy::cast_possible_truncation,
        reason = "IOVA >> 12 fits u32 for our allocation range"
    )]
    let addr_field = ((spt_iova >> 12) as u32) << 4;
    addr_field | VOL | TARGET_COH | SPT_PRESENT
}

/// Encode a GF100 V1 small-page PTE for an identity-mapped physical address.
///
/// Lower dword: `[31:4] = phys_addr >> 12, [3] = VOL, [2:1] = target, [0] = VALID`
/// Upper dword: 0 (uncompressed, no storage type)
fn encode_kepler_pte(phys_addr: u64) -> u64 {
    const VALID: u32 = 1;
    const TARGET_COH: u32 = 2 << 1;
    const VOL: u32 = 1 << 3;

    #[expect(
        clippy::cast_possible_truncation,
        reason = "phys_addr >> 12 fits u32 for our identity-mapped range"
    )]
    let lo = ((phys_addr >> 12) as u32) << 4 | VOL | TARGET_COH | VALID;
    u64::from(lo) // upper dword = 0 (no compression)
}

/// Write a little-endian `u32` into a byte slice at the given byte offset.
fn write_u32_le(buf: &mut [u8], offset: usize, value: u32) {
    buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// Populate GF100 V1 page tables: PDB + SPT for 2 MiB identity mapping.
///
/// PDB entry 0 contains a dual PDE: lower dword = large page (unused, 0),
/// upper dword = small page PDE pointing to our SPT.
///
/// SPT maps 512 small pages (4 KiB each, total 2 MiB). Page 0 is left
/// unmapped as a null guard.
pub(super) fn populate_kepler_page_tables(pdb: &mut [u8], spt: &mut [u8]) {
    // PDB entry 0: dual PDE (8 bytes)
    // Lower dword (large page table pointer) = 0 (no LPT)
    pdb[0..4].copy_from_slice(&0u32.to_le_bytes());
    // Upper dword (small page table pointer) = SPT PDE
    let spt_pde = encode_kepler_spt_pde(KEPLER_SPT_IOVA);
    pdb[4..8].copy_from_slice(&spt_pde.to_le_bytes());

    // SPT: identity-map 512 small pages (4 KiB each, total 2 MiB).
    // Page 0 left unmapped as a null guard.
    for i in 1..PT_ENTRIES {
        let phys = (i as u64) * 4096;
        let pte = encode_kepler_pte(phys);
        let off = i * 8;
        spt[off..off + 8].copy_from_slice(&pte.to_le_bytes());
    }
}

/// Populate Kepler instance block (RAMFC + GF100 V1 PDB config).
///
/// Matches nouveau's `gk104_chan_ramfc_write()` adapted for system memory.
/// Key difference from Volta: no VER2_PT bit, no subcontexts, 40-bit VA limit.
#[expect(
    clippy::cast_possible_truncation,
    reason = "IOVA values and ilog2 results always fit u32"
)]
pub(super) fn populate_kepler_instance_block(
    inst: &mut [u8],
    gpfifo_iova: u64,
    gpfifo_entries: u32,
    userd_iova: u64,
    channel_id: u32,
    pdb_iova: u64,
) {
    use super::registers::kepler_ramfc as ramfc;
    use super::registers::kepler_ramin as ramin;

    let limit2 = gpfifo_entries.ilog2();

    // ── RAMFC fields ────────────────────────────────────────────────
    write_u32_le(
        inst,
        ramfc::USERD_LO,
        (userd_iova as u32 & 0xFFFF_FE00) | PBDMA_TARGET_SYS_MEM_COHERENT,
    );
    write_u32_le(inst, ramfc::USERD_HI, (userd_iova >> 32) as u32);
    write_u32_le(inst, ramfc::SIGNATURE, 0x0000_FACE);
    write_u32_le(inst, ramfc::ACQUIRE, 0x7FFF_F902);

    write_u32_le(inst, ramfc::GP_BASE_LO, gpfifo_iova as u32);
    write_u32_le(
        inst,
        ramfc::GP_BASE_HI,
        (gpfifo_iova >> 32) as u32 | (limit2 << 16),
    );
    write_u32_le(inst, ramfc::GP_PUT, 0);
    write_u32_le(inst, ramfc::GP_GET, 0);
    write_u32_le(inst, ramfc::GP_FETCH, 0);

    write_u32_le(inst, ramfc::PB_HEADER, 0x2040_0000);
    write_u32_le(inst, ramfc::SUBDEVICE, 0x3000_0000 | 0xFFF);
    write_u32_le(inst, ramfc::HCE_CTRL, 0x0000_0020);
    write_u32_le(inst, ramfc::CHID, channel_id);
    // GK104 has CONFIG — write default (0). Unlike GV100 where it PRI-faults.
    write_u32_le(inst, ramfc::CONFIG, 0);
    write_u32_le(inst, ramfc::CHANNEL_INFO, 0x0300_0000 | channel_id);

    // ── NV_RAMIN page directory base (GF100 V1 encoding) ────────────
    // No VER2 bit (bit 10), no BIG_PAGE_SIZE (bit 11 = 0 → 128K large pages).
    let pdb_lo: u32 = ((pdb_iova >> 12) as u32) << 12
        | (1 << 2)  // VOL = TRUE
        | TARGET_SYS_MEM_COHERENT;
    write_u32_le(inst, ramin::PAGE_DIR_BASE_LO, pdb_lo);
    write_u32_le(inst, ramin::PAGE_DIR_BASE_HI, (pdb_iova >> 32) as u32);

    // VA space address limit — 1 TB (40-bit, Kepler limit).
    write_u32_le(inst, ramin::ADDR_LIMIT_LO, 0xFFFF_FFFF);
    write_u32_le(inst, ramin::ADDR_LIMIT_HI, 0x0000_00FF);
}

/// Populate Kepler runlist with a single channel entry.
///
/// GK104 runlist format: 8 bytes per channel entry.
/// - DW0: channel_id
/// - DW1: 0x0000_0004 (TYPE = CHANNEL)
///
/// Unlike Volta's 16-byte TSG header + 16-byte channel entry, Kepler
/// uses a flat list of channel entries.
pub(super) fn populate_kepler_runlist(rl: &mut [u8], channel_id: u32) {
    write_u32_le(rl, 0x00, channel_id);
    write_u32_le(rl, 0x04, 0x0000_0004);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kepler_spt_pde_encoding() {
        let pde = encode_kepler_spt_pde(0x6000);
        // (0x6000 >> 12) << 4 | VOL(8) | COH(4) | PRESENT(1) = 0x60 | 0xD = 0x6D
        assert_eq!(pde, 0x6D);
        assert_eq!(pde & 1, 1, "SPT present");
        assert_eq!((pde >> 1) & 3, 2, "target = COH");
        assert_eq!((pde >> 3) & 1, 1, "VOL bit");
        let addr = u64::from(pde & 0xFFFF_FFF0) << 8;
        assert_eq!(addr, 0x6000, "decoded SPT address");
    }

    #[test]
    fn kepler_pte_encoding() {
        let pte = encode_kepler_pte(0x1000);
        // lo: (0x1000 >> 12) << 4 | VOL(8) | COH(4) | VALID(1) = 0x10 | 0xD = 0x1D
        assert_eq!(pte, 0x1D);
        assert_eq!(pte & 1, 1, "valid bit");
        assert_eq!((pte >> 1) & 3, 2, "target = COH");
        assert_eq!((pte >> 3) & 1, 1, "VOL bit");
        let addr = u64::from((pte as u32) & 0xFFFF_FFF0) << 8;
        assert_eq!(addr, 0x1000, "decoded physical address");
    }

    #[test]
    fn kepler_pte_upper_dword_zero() {
        let pte = encode_kepler_pte(0x2000);
        assert_eq!(pte >> 32, 0, "upper dword = 0 (no compression)");
    }

    #[test]
    fn kepler_pte_higher_address() {
        let pte = encode_kepler_pte(0x10_0000);
        let lo = pte as u32;
        let addr = u64::from(lo & 0xFFFF_FFF0) << 8;
        assert_eq!(addr, 0x10_0000);
    }

    #[test]
    fn kepler_runlist_entry_format() {
        let mut rl = [0u8; 64];
        populate_kepler_runlist(&mut rl, 5);
        let dw0 = u32::from_le_bytes([rl[0], rl[1], rl[2], rl[3]]);
        let dw1 = u32::from_le_bytes([rl[4], rl[5], rl[6], rl[7]]);
        assert_eq!(dw0, 5, "channel_id = 5");
        assert_eq!(dw1, 4, "TYPE = CHANNEL");
    }
}
