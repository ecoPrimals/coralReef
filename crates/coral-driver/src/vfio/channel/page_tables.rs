// SPDX-License-Identifier: AGPL-3.0-only
//! V2 MMU page table encoding and population for Volta+ PFIFO channels.
//!
//! Implements the 5-level page table hierarchy (PD3→PD2→PD1→PD0→PT) used by
//! GV100 for GPU virtual address translation. Encoding matches nouveau's
//! `gp100_vmm_pd0_pde()` and `gp100_vmm_pgt_mem()`.

use super::registers::*;

/// Write a PDE entry at `index` in a page directory buffer.
///
/// V2 PDE layout: `(phys_addr >> 4) | flags` — the GPU decodes the
/// physical address as `(PDE & ~0x7) << 4`.
pub(super) fn write_pde(pd_slice: &mut [u8], index: usize, target_iova: u64) {
    let pde = encode_pde(target_iova);
    let off = index * 8;
    pd_slice[off..off + 8].copy_from_slice(&pde.to_le_bytes());
}

/// Encode a V2 PDE pointing to a page table at `iova` in system memory.
///
/// GP100 PDE bit layout (from nouveau `gp100_vmm_pde`):
///   `[2:1]=aperture, [3]=VOL, addr = (PDE & ~0xF) << 4`
///   Aperture: 0=invalid, 1=VRAM, 2=SYS_MEM_COH, 3=SYS_MEM_NCOH
pub(super) fn encode_pde(iova: u64) -> u64 {
    const FLAGS: u64 = (2 << 1) | (1 << 3); // aperture=COH in bits[2:1], VOL=bit3
    (iova >> 4) | FLAGS
}

/// Encode a V2 small-page PTE for an identity-mapped physical address.
///
/// GP100 PTE bit layout (from nouveau `gp100_vmm_valid` + `gf100_vmm_aper`):
///   `[0]=VALID, [2:1]=aperture, [3]=VOL, addr = (PTE & ~0xF) << 4`
///   Aperture: 0=VRAM, 2=SYS_MEM_COH, 3=SYS_MEM_NCOH
pub(super) fn encode_pte(phys_addr: u64) -> u64 {
    const FLAGS: u64 = 1 | (2 << 1) | (1 << 3); // VALID + COH(aper=2) + VOL
    (phys_addr >> 4) | FLAGS
}

/// Write a little-endian `u32` into a byte slice at the given byte offset.
pub(super) fn write_u32_le(buf: &mut [u8], offset: usize, value: u32) {
    buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// Populate V2 MMU page tables with identity mapping for the first 2 MiB.
///
/// The 5-level hierarchy (PD3→PD2→PD1→PD0→PT) maps GPU virtual addresses
/// directly to their IOVA equivalents, so GPU VA 0x1000 → physical 0x1000
/// (which the IOMMU then translates to the actual host physical address).
pub(super) fn populate_page_tables(
    pd3: &mut [u8],
    pd2: &mut [u8],
    pd1: &mut [u8],
    pd0: &mut [u8],
    pt0: &mut [u8],
) {
    write_pde(pd3, 0, PD2_IOVA);
    write_pde(pd2, 0, PD1_IOVA);
    write_pde(pd1, 0, PD0_IOVA);

    // PD0 entry 0: dual PDE format — 16 bytes per entry.
    // Bytes [0:7]  = small page PDE (SPT, pt[0]) → PT0
    // Bytes [8:15] = large page PDE (LPT, pt[1]) — unused, leave as 0
    // Layout matches nouveau's VMM_WO128(pd, ..., data[0]=SPT, data[1]=LPT)
    let small_pde = encode_pde(PT0_IOVA);
    pd0[0..8].copy_from_slice(&small_pde.to_le_bytes());

    // PT0: identity-map 512 small pages (4 KiB each, total 2 MiB).
    // Page 0 left unmapped as a null guard.
    for i in 1..PT_ENTRIES {
        let phys = (i as u64) * 4096;
        let pte = encode_pte(phys);
        let off = i * 8;
        pt0[off..off + 8].copy_from_slice(&pte.to_le_bytes());
    }
}

/// Populate V2 MMU page tables with custom IOVAs for the page table chain.
///
/// Same identity mapping as `populate_page_tables`, but using caller-provided
/// IOVAs for the page directory/table buffers.
#[expect(
    clippy::too_many_arguments,
    reason = "page table chain requires 4 buffers + 4 IOVAs"
)]
pub(super) fn populate_page_tables_custom(
    pd3: &mut [u8],
    pd2: &mut [u8],
    pd1: &mut [u8],
    pd0: &mut [u8],
    pt0: &mut [u8],
    pd2_iova: u64,
    pd1_iova: u64,
    pd0_iova: u64,
    pt0_iova: u64,
) {
    write_pde(pd3, 0, pd2_iova);
    write_pde(pd2, 0, pd1_iova);
    write_pde(pd1, 0, pd0_iova);

    let small_pde = encode_pde(pt0_iova);
    pd0[0..8].copy_from_slice(&small_pde.to_le_bytes());

    for i in 1..PT_ENTRIES {
        let phys = (i as u64) * 4096;
        let pte = encode_pte(phys);
        let off = i * 8;
        pt0[off..off + 8].copy_from_slice(&pte.to_le_bytes());
    }
}

/// Populate instance block with custom PD3 IOVA (RAMFC + RAMIN page directory base).
#[expect(
    clippy::cast_possible_truncation,
    reason = "IOVA values and ilog2 results always fit u32"
)]
pub(super) fn populate_instance_block_custom(
    inst: &mut [u8],
    gpfifo_iova: u64,
    gpfifo_entries: u32,
    userd_iova: u64,
    channel_id: u32,
    pd3_iova: u64,
) {
    let limit2 = gpfifo_entries.ilog2();

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
    write_u32_le(inst, ramfc::GP_PUT, 1);
    write_u32_le(inst, ramfc::GP_GET, 0);
    write_u32_le(inst, ramfc::GP_FETCH, 0);

    write_u32_le(inst, ramfc::PB_HEADER, 0x2040_0000);
    write_u32_le(inst, ramfc::SUBDEVICE, 0x3000_0000 | 0xFFF);
    write_u32_le(inst, ramfc::HCE_CTRL, 0x0000_0020);
    write_u32_le(inst, ramfc::CHID, channel_id);
    write_u32_le(inst, ramfc::CHANNEL_INFO, 0x0300_0000 | channel_id);

    let pdb_lo: u32 = ((pd3_iova >> 12) as u32) << 12
        | (1 << 11)
        | (1 << 10)
        | (1 << 2)
        | TARGET_SYS_MEM_COHERENT;
    write_u32_le(inst, ramin::PAGE_DIR_BASE_LO, pdb_lo);
    write_u32_le(inst, ramin::PAGE_DIR_BASE_HI, (pd3_iova >> 32) as u32);

    write_u32_le(inst, ramin::ADDR_LIMIT_LO, 0xFFFF_FFFF);
    write_u32_le(inst, ramin::ADDR_LIMIT_HI, 0x0001_FFFF);

    write_u32_le(inst, ramin::ENGINE_WFI_VEID, 0);

    write_u32_le(inst, ramin::SC_PDB_VALID, 1);
    write_u32_le(inst, ramin::SC0_PAGE_DIR_BASE_LO, pdb_lo);
    write_u32_le(inst, ramin::SC0_PAGE_DIR_BASE_HI, (pd3_iova >> 32) as u32);

    write_u32_le(inst, ramin::SC1_PAGE_DIR_BASE_LO, 1);
    write_u32_le(inst, ramin::SC1_PAGE_DIR_BASE_HI, 1);
}

/// Populate instance block (RAMFC + RAMIN page directory base).
///
/// Field values match `gv100_chan_ramfc_write()` from nouveau with
/// `priv=true` and `devm=0xFFF`, adapted for system memory aperture.
#[expect(
    clippy::cast_possible_truncation,
    reason = "IOVA values and ilog2 results always fit u32"
)]
pub(super) fn populate_instance_block(
    inst: &mut [u8],
    gpfifo_iova: u64,
    gpfifo_entries: u32,
    userd_iova: u64,
    channel_id: u32,
) {
    let limit2 = gpfifo_entries.ilog2();

    // ── RAMFC fields (offsets 0x000..0x1FF) ────────────────────────
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
    // GP_PUT=1 so the scheduler loads work immediately; GP_GET=0.
    // Without this, the PBDMA sees GP_PUT=0=GP_GET and waits for a doorbell
    // that may never deliver if the USERD DMA path isn't fully working.
    write_u32_le(inst, ramfc::GP_PUT, 1);
    write_u32_le(inst, ramfc::GP_GET, 0);
    write_u32_le(inst, ramfc::GP_FETCH, 0);

    write_u32_le(inst, ramfc::PB_HEADER, 0x2040_0000);
    write_u32_le(inst, ramfc::SUBDEVICE, 0x3000_0000 | 0xFFF);
    write_u32_le(inst, ramfc::HCE_CTRL, 0x0000_0020);
    write_u32_le(inst, ramfc::CHID, channel_id);
    // CONFIG (0xA8) not written — register doesn't exist on GV100 PBDMA
    write_u32_le(inst, ramfc::CHANNEL_INFO, 0x0300_0000 | channel_id);

    // ── NV_RAMIN page directory base (offset 0x200) ────────────────
    let pdb_lo: u32 = ((PD3_IOVA >> 12) as u32) << 12
        | (1 << 11) // BIG_PAGE_SIZE = 64 KiB
        | (1 << 10) // USE_VER2_PT_FORMAT = TRUE
        | (1 << 2)  // VOL = TRUE
        | TARGET_SYS_MEM_COHERENT;
    write_u32_le(inst, ramin::PAGE_DIR_BASE_LO, pdb_lo);
    write_u32_le(inst, ramin::PAGE_DIR_BASE_HI, (PD3_IOVA >> 32) as u32);

    // VA space address limit — 128 TB (matches nouveau gp100_vmm with 47-bit VA).
    // Without this, the MMU rejects all VA translations as VA_LIMIT_VIOLATION.
    write_u32_le(inst, ramin::ADDR_LIMIT_LO, 0xFFFF_FFFF);
    write_u32_le(inst, ramin::ADDR_LIMIT_HI, 0x0001_FFFF);

    write_u32_le(inst, ramin::ENGINE_WFI_VEID, 0);

    // ── Subcontext 0 page directory (mirrors main PDB) ────────────
    write_u32_le(inst, ramin::SC_PDB_VALID, 1);
    write_u32_le(inst, ramin::SC0_PAGE_DIR_BASE_LO, pdb_lo);
    write_u32_le(inst, ramin::SC0_PAGE_DIR_BASE_HI, (PD3_IOVA >> 32) as u32);

    // Subcontext 1: mark as INVALID (nouveau sets 0x00000001 for unused)
    write_u32_le(inst, ramin::SC1_PAGE_DIR_BASE_LO, 1);
    write_u32_le(inst, ramin::SC1_PAGE_DIR_BASE_HI, 1);
}

/// Populate runlist with a TSG header + channel entry (Volta RAMRL format).
#[expect(
    clippy::cast_possible_truncation,
    reason = "IOVA values always fit u32 for our allocation range"
)]
pub(super) fn populate_runlist(
    rl: &mut [u8],
    userd_iova: u64,
    channel_id: u32,
    instance_iova: u64,
    runq: u32,
) {
    // ── TSG (channel group) header — 16 bytes ──────────────────────
    write_u32_le(rl, 0x00, (128 << 24) | (3 << 16) | 1);
    write_u32_le(rl, 0x04, 1);
    write_u32_le(rl, 0x08, 0);
    write_u32_le(rl, 0x0C, 0);

    // ── Channel entry — 16 bytes (gv100_runl_insert_chan) ────────────
    // DW0: [31:8] USERD_ADDR[31:8], [7:6] USERD_TARGET, [1] RUNQ, [0] TYPE=0
    // DW1: [31:0] USERD_ADDR_HI
    // DW2: [31:12] INST_ADDR[31:12], [21:20] INST_TARGET, [11:0] CHID
    // DW3: [31:0] INST_ADDR_HI
    write_u32_le(
        rl,
        0x10,
        (userd_iova as u32 & 0xFFFF_FF00) | (TARGET_SYS_MEM_COHERENT << 6) | (runq << 1),
    );
    write_u32_le(rl, 0x14, (userd_iova >> 32) as u32);
    write_u32_le(
        rl,
        0x18,
        (instance_iova as u32 & 0xFFFF_F000) | (TARGET_SYS_MEM_NONCOHERENT << 20) | channel_id,
    );
    write_u32_le(rl, 0x1C, (instance_iova >> 32) as u32);
}

/// Populate runlist in a pre-allocated buffer (static version for matrix).
#[expect(clippy::cast_possible_truncation)]
pub(super) fn populate_runlist_static(
    rl: &mut [u8],
    userd_iova: u64,
    channel_id: u32,
    userd_target: u32,
    inst_target: u32,
    runq: u32,
) {
    write_u32_le(rl, 0x00, (128 << 24) | (3 << 16) | 1);
    write_u32_le(rl, 0x04, 1);
    write_u32_le(rl, 0x08, 0);
    write_u32_le(rl, 0x0C, 0);
    // DW0: [31:8] USERD_ADDR, [7:6] USERD_TARGET, [1] RUNQ, [0] TYPE=0
    write_u32_le(
        rl,
        0x10,
        (userd_iova as u32 & 0xFFFF_FF00) | (userd_target << 6) | (runq << 1),
    );
    // DW1: USERD_ADDR_HI
    write_u32_le(rl, 0x14, (userd_iova >> 32) as u32);
    // DW2: [31:12] INST_ADDR, [21:20] INST_TARGET, [11:0] CHID
    write_u32_le(
        rl,
        0x18,
        (INSTANCE_IOVA as u32 & 0xFFFF_F000) | (inst_target << 20) | channel_id,
    );
    write_u32_le(rl, 0x1C, (INSTANCE_IOVA >> 32) as u32);
}

/// Populate instance block with static parameters (for diagnostic matrix).
pub(super) fn populate_instance_block_static(
    inst: &mut [u8],
    gpfifo_iova: u64,
    gpfifo_entries: u32,
    userd_iova: u64,
    channel_id: u32,
) {
    populate_instance_block(inst, gpfifo_iova, gpfifo_entries, userd_iova, channel_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pde_encoding_sys_mem_coherent() {
        let pde = encode_pde(0x6000);
        // (0x6000 >> 4) | (2 << 1) | (1 << 3) = 0x600 | 0xC = 0x60C
        assert_eq!(pde, 0x60C);
        assert_eq!((pde >> 1) & 3, 2, "aperture bits[2:1] = COH(2)");
        assert_eq!((pde >> 3) & 1, 1, "VOL bit 3");
        let addr = (pde & !0xF) << 4;
        assert_eq!(addr, 0x6000, "GPU decode: (PDE & ~0xF) << 4");
    }

    #[test]
    fn pte_encoding_identity_map() {
        let pte = encode_pte(0x1000);
        // (0x1000 >> 4) | 1 | (2 << 1) | (1 << 3) = 0x100 | 0xD = 0x10D
        assert_eq!(pte, 0x10D);
        assert_eq!(pte & 1, 1, "valid bit");
        assert_eq!((pte >> 1) & 3, 2, "aperture bits[2:1] = COH(2)");
        assert_eq!((pte >> 3) & 1, 1, "VOL bit 3");
        let addr = (pte & !0xF) << 4;
        assert_eq!(addr, 0x1000, "GPU decode: (PTE & ~0xF) << 4");
    }

    #[test]
    fn pte_encoding_higher_address() {
        let pte = encode_pte(0x10_0000);
        assert_eq!(pte, 0x1_000D);
        let addr = (pte & !0xF) << 4;
        assert_eq!(addr, 0x10_0000);
    }

    #[test]
    fn write_u32_le_roundtrip() {
        let mut buf = [0u8; 8];
        write_u32_le(&mut buf, 4, 0xDEAD_BEEF);
        let val = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        assert_eq!(val, 0xDEAD_BEEF);
    }

    #[test]
    fn ramuserd_offsets_match_nvidia_spec() {
        assert_eq!(ramuserd::GP_GET, 0x88);
        assert_eq!(ramuserd::GP_PUT, 0x8C);
    }

    #[test]
    fn pccsr_register_offsets() {
        assert_eq!(pccsr::inst(0), 0x80_0000);
        assert_eq!(pccsr::channel(0), 0x80_0004);
        assert_eq!(pccsr::inst(1), 0x80_0008);
        assert_eq!(pccsr::channel(1), 0x80_000C);
    }

    #[test]
    fn iova_layout_non_overlapping() {
        let iovas = [
            ("INSTANCE", INSTANCE_IOVA),
            ("RUNLIST", RUNLIST_IOVA),
            ("PD3", PD3_IOVA),
            ("PD2", PD2_IOVA),
            ("PD1", PD1_IOVA),
            ("PD0", PD0_IOVA),
            ("PT0", PT0_IOVA),
        ];
        for i in 0..iovas.len() {
            for j in (i + 1)..iovas.len() {
                assert_ne!(
                    iovas[i].1, iovas[j].1,
                    "{} and {} overlap at {:#x}",
                    iovas[i].0, iovas[j].0, iovas[i].1
                );
            }
        }
    }

    #[test]
    fn iova_layout_after_userd() {
        assert!(INSTANCE_IOVA > 0x2000, "instance after USERD");
        assert!(
            PT0_IOVA + 4096 <= 0x10_0000,
            "page tables before USER_IOVA_BASE"
        );
    }

    #[test]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "IOVA addresses are intentionally truncated to 32-bit hardware register fields"
    )]
    fn pccsr_inst_value_channel_zero() {
        let value = (INSTANCE_IOVA >> 12) as u32 | pccsr::INST_TARGET_SYS_MEM_NCOH;
        assert_eq!(value & 0x0FFF_FFFF, 3, "INST_PTR = 3 (0x3000 >> 12)");
        assert_eq!((value >> 28) & 3, 3, "target = SYS_MEM_NCOH");
        assert_eq!((value >> 31) & 1, 0, "BIND not set — implicit via runlist");
    }

    #[test]
    fn runlist_gv100_register_addresses() {
        assert_eq!(pfifo::runlist_base(0), 0x2270, "RL0 base");
        assert_eq!(pfifo::runlist_submit(0), 0x2274, "RL0 submit");
        assert_eq!(pfifo::runlist_base(1), 0x2280, "RL1 base");
        assert_eq!(pfifo::runlist_submit(1), 0x2284, "RL1 submit");
        assert_eq!(pfifo::runlist_base(2), 0x2290, "RL2 base");
    }

    #[test]
    fn runlist_gv100_value_encoding() {
        let base = pfifo::gv100_runlist_base_value(RUNLIST_IOVA);
        assert_eq!(base, 4, "lower_32(0x4000 >> 12) = 4");
        let submit = pfifo::gv100_runlist_submit_value(RUNLIST_IOVA, 2);
        assert_eq!(submit, 2 << 16, "upper_32(0x4000>>12)=0, count=2<<16");
        assert_eq!((submit >> 16) & 0xFFFF, 2, "entry count");
    }

    #[test]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "IOVA addresses truncated to 32-bit registers"
    )]
    fn runlist_chan_entry_encoding() {
        let userd: u64 = 0x2000;
        let dw0 = userd as u32 | (TARGET_SYS_MEM_COHERENT << 2);
        assert_eq!(dw0, 0x2008, "USERD=0x2000, target=COH(2), runq=0");
        assert_eq!((dw0 >> 2) & 3, 2, "USERD_TARGET = SYS_MEM_COH");
        assert_eq!(dw0 & 1, 0, "TYPE = 0 (channel)");

        let dw0_runq1 = userd as u32 | (TARGET_SYS_MEM_COHERENT << 2) | (1 << 1);
        assert_eq!(dw0_runq1, 0x200A, "USERD=0x2000, target=COH(2), runq=1");
        assert_eq!((dw0_runq1 >> 1) & 1, 1, "RUNQUEUE = 1");

        let inst: u64 = 0x3000;
        let chid: u32 = 0;
        let dw2 = inst as u32 | (TARGET_SYS_MEM_NONCOHERENT << 4) | chid;
        assert_eq!(dw2, 0x3030, "INST=0x3000, target=NCOH(3), chid=0");
        assert_eq!((dw2 >> 4) & 3, 3, "INST_TARGET = SYS_MEM_NCOH");
    }
}
