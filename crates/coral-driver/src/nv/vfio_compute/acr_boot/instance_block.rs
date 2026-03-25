// SPDX-License-Identifier: AGPL-3.0-only

//! VRAM instance block and falcon MMU page table helpers for ACR DMA.

use crate::vfio::device::MappedBar;
use crate::vfio::memory::{MemoryRegion, PraminRegion};

/// SEC2 falcon instance block binding register.
/// Nouveau `gm200_flcn_bind_inst` writes to falcon_base + 0x054.
/// Previous value 0x668 was incorrect — confirmed via upstream
/// `nvkm/falcon/gm200.c` (hotSpring Exp 083).
pub(crate) const SEC2_FLCN_BIND_INST: usize = 0x054;

/// DMAIDX register — must be cleared to 0 (VIRT) before writing bind_inst.
/// Nouveau: `nvkm_falcon_mask(falcon, 0x604, 0x07, 0x00)`.
pub(crate) const FALCON_DMAIDX: usize = 0x604;

/// UNK090 — bit 16 must be set after writing bind_inst to trigger binding.
/// Nouveau: `nvkm_falcon_mask(falcon, 0x090, 0x00010000, 0x00010000)`.
pub(crate) const FALCON_UNK090: usize = 0x090;

/// ENG_CONTROL — bit 3 must be set after UNK090 to complete bind trigger.
/// Nouveau: `nvkm_falcon_mask(falcon, 0x0a4, 0x00000008, 0x00000008)`.
pub(crate) const FALCON_ENG_CONTROL: usize = 0x0a4;

/// CHANNEL_TRIGGER — bit 1 (LOAD) must be set after bind_stat==5 to activate.
/// Nouveau: `nvkm_falcon_mask(falcon, 0x058, 2, 2)`.
pub(crate) const FALCON_CHANNEL_TRIGGER: usize = 0x058;

/// INTR_ACK — write bit 3 to acknowledge bind completion interrupt.
/// Nouveau: `nvkm_falcon_mask(falcon, 0x004, 0x8, 0x8)`.
pub(crate) const FALCON_INTR_ACK: usize = 0x004;

/// bind_stat register — bits [14:12] polled until == 5.
pub(crate) const FALCON_BIND_STAT: usize = 0x0dc;

/// Construct the bind_inst register value.
/// Format from nouveau `gm200_flcn_bind_inst`:
///   `(1 << 30) | (target << 28) | (addr >> 12)`
/// Target: 0=VRAM, 2=SYS_MEM_COHERENT, 3=SYS_MEM_NCOH.
pub(crate) fn encode_bind_inst(addr: u64, target: u32) -> u32 {
    (1u32 << 30) | (target << 28) | ((addr >> 12) as u32)
}

/// Execute the full nouveau-style falcon bind sequence (Exp 084 discovery).
///
/// Nouveau `gm200_flcn_bind_inst` + `gm200_flcn_fw_load` does ALL of these:
/// 1. Clear DMAIDX → VIRT
/// 2. Write CHANNEL_NEXT (0x054) with bind value
/// 3. Set UNK090 bit 16 (trigger)
/// 4. Set ENG_CONTROL bit 3 (trigger)
/// 5. Poll bind_stat bits[14:12] == 5
/// 6. Ack interrupt (0x004 bit 3)
/// 7. Set CHANNEL_TRIGGER LOAD (0x058 bit 1)
/// 8. Poll bind_stat bits[14:12] == 0
///
/// Returns (bind_ok, notes) where bind_ok is true if bind_stat reached 5.
pub(crate) fn falcon_bind_context(
    r: &dyn Fn(usize) -> u32,
    w: &dyn Fn(usize, u32),
    bind_val: u32,
) -> (bool, Vec<String>) {
    let mut notes = Vec::new();

    // Step 1: DMAIDX → VIRT
    let dmaidx_before = r(FALCON_DMAIDX);
    w(FALCON_DMAIDX, dmaidx_before & !0x07);
    notes.push(format!(
        "DMAIDX: {dmaidx_before:#x} → {:#x}",
        r(FALCON_DMAIDX)
    ));

    // Step 2: Write CHANNEL_NEXT (bind_inst)
    w(SEC2_FLCN_BIND_INST, bind_val);
    let rb = r(SEC2_FLCN_BIND_INST);
    notes.push(format!(
        "bind_inst: wrote={bind_val:#010x} readback={rb:#010x}"
    ));

    // Step 3: UNK090 bit 16 — trigger
    let unk090 = r(FALCON_UNK090);
    w(FALCON_UNK090, unk090 | 0x0001_0000);
    notes.push(format!(
        "UNK090: {unk090:#010x} → {:#010x}",
        r(FALCON_UNK090)
    ));

    // Step 4: ENG_CONTROL bit 3 — trigger
    let eng_ctrl = r(FALCON_ENG_CONTROL);
    w(FALCON_ENG_CONTROL, eng_ctrl | 0x0000_0008);
    notes.push(format!(
        "ENG_CTRL: {eng_ctrl:#010x} → {:#010x}",
        r(FALCON_ENG_CONTROL)
    ));

    // Step 5: Poll bind_stat bits[14:12] == 5 (10ms timeout)
    let start = std::time::Instant::now();
    let mut bind_ok = false;
    let mut last_stat_raw;
    loop {
        last_stat_raw = r(FALCON_BIND_STAT);
        let stat = (last_stat_raw >> 12) & 0x7;
        if stat == 5 {
            bind_ok = true;
            break;
        }
        if start.elapsed() > std::time::Duration::from_millis(10) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(10));
    }

    if bind_ok {
        notes.push(format!(
            "bind_stat→5: OK ({:#010x}) in {:?}",
            last_stat_raw,
            start.elapsed()
        ));

        // Step 6: Ack interrupt bit 3
        w(FALCON_INTR_ACK, r(FALCON_INTR_ACK) | 0x08);

        // Step 7: CHANNEL_TRIGGER LOAD (bit 1)
        let trigger = r(FALCON_CHANNEL_TRIGGER);
        w(FALCON_CHANNEL_TRIGGER, trigger | 0x02);
        notes.push(format!(
            "CHANNEL_TRIGGER: {trigger:#010x} → {:#010x}",
            r(FALCON_CHANNEL_TRIGGER)
        ));

        // Step 8: Poll bind_stat → 0
        let start2 = std::time::Instant::now();
        loop {
            let raw = r(FALCON_BIND_STAT);
            let stat = (raw >> 12) & 0x7;
            if stat == 0 {
                notes.push(format!(
                    "bind_stat→0: OK ({raw:#010x}) in {:?}",
                    start2.elapsed()
                ));
                break;
            }
            if start2.elapsed() > std::time::Duration::from_millis(10) {
                notes.push(format!("bind_stat→0: TIMEOUT ({raw:#010x})",));
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(10));
        }
    } else {
        notes.push(format!("bind_stat→5: TIMEOUT ({last_stat_raw:#010x})"));
    }

    (bind_ok, notes)
}

// ── VRAM Instance Block for Falcon DMA ────────────────────────────────
// VRAM addresses for the page table chain (below our ACR/WPR region)
pub(crate) const FALCON_INST_VRAM: u32 = 0x10000;
pub(crate) const FALCON_PD3_VRAM: u32 = 0x11000;
pub(crate) const FALCON_PD2_VRAM: u32 = 0x12000;
pub(crate) const FALCON_PD1_VRAM: u32 = 0x13000;
pub(crate) const FALCON_PD0_VRAM: u32 = 0x14000;
pub(crate) const FALCON_PT0_VRAM: u32 = 0x15000;

/// Encode a non-leaf PDE pointing to a sub-directory in VRAM.
///
/// MMU v2 PDE format (from nova-core `dev_mmu.h`):
///   bit 0:      valid_inverted (0 = valid)
///   bits[2:1]:  aperture (1 = VideoMemory)
///   bits[32:8]: table_frame_vid = phys_addr >> 12
///
/// Result: `(phys >> 12) << 8 | aperture << 1` = `(phys >> 4) | 0x2`
pub(crate) fn encode_vram_pde(vram_addr: u64) -> u64 {
    const APER_VRAM: u64 = 1 << 1; // bits[2:1] = 1 = VRAM
    (vram_addr >> 4) | APER_VRAM
}

/// Encode a PD0 dual PDE for the small page table pointer (same as non-leaf PDE).
///
/// On GV100, the PD0 level uses 16-byte dual PDEs. The small PT pointer goes in
/// the **upper** 8 bytes (offset 8). Callers must write this at offset 8.
pub(crate) fn encode_vram_pd0_pde(vram_addr: u64) -> u64 {
    encode_vram_pde(vram_addr)
}

/// Encode a PTE for a 4K VRAM page.
///
/// MMU v2 PTE format (from nova-core `dev_mmu.h`):
///   bit 0:      valid (1 = valid)
///   bits[2:1]:  aperture (0 = VideoMemory for PTEs)
///   bits[32:8]: frame_number_vid = phys_addr >> 12
///
/// Result: `(phys >> 12) << 8 | 1` = `(phys >> 4) | 1`
pub(crate) fn encode_vram_pte(vram_phys: u64) -> u64 {
    const VALID: u64 = 1;
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

    // Zero ALL pages first — stale VRAM garbage in unused PDE/PTE entries
    // can look valid to the MMU walker and cause it to follow bogus pointers.
    for &page_vram in &[
        FALCON_INST_VRAM,
        FALCON_PD3_VRAM,
        FALCON_PD2_VRAM,
        FALCON_PD1_VRAM,
        FALCON_PD0_VRAM,
        FALCON_PT0_VRAM,
    ] {
        for off in (0..0x1000).step_by(4) {
            if !wv(page_vram, off, 0) {
                tracing::warn!(page_vram, off, "failed to zero page");
                return false;
            }
        }
    }

    // GV100 MMU v2 uses 16-byte (128-bit) PDE entries at all directory levels.
    // Non-leaf PDEs: lower 8 bytes = 0 (V=0 → PDE mode), upper 8 bytes = pointer.
    // PD0 dual PDE: lower 8 bytes = big PT (0 for small-only), upper 8 bytes = small PT.

    // PD3[0] → PD2 (upper half of 16-byte entry)
    if !wv64(FALCON_PD3_VRAM, 0, 0) {
        return false;
    }
    if !wv64(FALCON_PD3_VRAM, 8, encode_vram_pde(FALCON_PD2_VRAM as u64)) {
        return false;
    }
    // PD2[0] → PD1
    if !wv64(FALCON_PD2_VRAM, 0, 0) {
        return false;
    }
    if !wv64(FALCON_PD2_VRAM, 8, encode_vram_pde(FALCON_PD1_VRAM as u64)) {
        return false;
    }
    // PD1[0] → PD0
    if !wv64(FALCON_PD1_VRAM, 0, 0) {
        return false;
    }
    if !wv64(FALCON_PD1_VRAM, 8, encode_vram_pde(FALCON_PD0_VRAM as u64)) {
        return false;
    }
    // PD0[0] → PT0 (dual PDE: lower = big PT [none], upper = small PT)
    if !wv64(FALCON_PD0_VRAM, 0, 0) {
        return false;
    }
    if !wv64(FALCON_PD0_VRAM, 8, encode_vram_pde(FALCON_PT0_VRAM as u64)) {
        return false;
    }

    // PT0: identity-map 512 small pages (4KiB each = 2MiB total).
    // Starts at page 0 to avoid unmapped-VA faults if firmware accesses low VRAM.
    for i in 0u64..512 {
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

    // Verify: read back key entries (PDE pointer is at upper 8 bytes = offset +8)
    let rv = |vram_addr: u32, offset: usize| -> u32 {
        PraminRegion::new(bar0, vram_addr, offset + 4)
            .ok()
            .and_then(|r| r.read_u32(offset).ok())
            .unwrap_or(0xDEAD)
    };
    let pdb_rb = rv(FALCON_INST_VRAM, 0x200);
    let pd3_hi_lo = rv(FALCON_PD3_VRAM, 8);
    let pd3_hi_hi = rv(FALCON_PD3_VRAM, 12);
    let pt0_112_lo = rv(FALCON_PT0_VRAM, 112 * 8);
    let pt0_112_hi = rv(FALCON_PT0_VRAM, 112 * 8 + 4);
    let pt0_1_lo = rv(FALCON_PT0_VRAM, 8);
    let pt0_1_hi = rv(FALCON_PT0_VRAM, 8 + 4);

    tracing::info!(
        pdb_lo = format!("{pdb_lo:#010x}"),
        pdb_rb = format!("{pdb_rb:#010x}"),
        pd3_upper = format!("{pd3_hi_lo:#010x}:{pd3_hi_hi:#010x}"),
        pt112 = format!("{pt0_112_lo:#010x}:{pt0_112_hi:#010x}"),
        pt1 = format!("{pt0_1_lo:#010x}:{pt0_1_hi:#010x}"),
        "VRAM falcon instance block built"
    );
    true
}
