// SPDX-License-Identifier: AGPL-3.0-or-later
//! BAR2 page table initialization for cold GPUs (post-FLR / VFIO bind).
//!
//! Writes a V2 5-level page table hierarchy in VRAM via the PRAMIN window
//! and configures both `BAR1_BLOCK` and `BAR2_BLOCK` in virtual mode.
//! Required by the PFIFO scheduler — without a valid BAR2 aperture,
//! channel switches fail with `CHSW_ERROR=0x4` (RDAT_TIMEOUT).

use std::borrow::Cow;

use crate::error::{DriverError, DriverResult};
use crate::vfio::device::MappedBar;

use super::registers::*;

/// Build a minimal BAR2 page table in VRAM and program `NV_PBUS_BAR2_BLOCK`.
///
/// On a cold GPU (post-FLR / VFIO bind), BAR2_BLOCK reads `0x40000000` (invalid).
/// The PFIFO scheduler requires a configured BAR2 aperture — without it, channel
/// switches fail with `CHSW_ERROR=0x4` (RDAT_TIMEOUT).
///
/// This replicates what nouveau does in `gf100_bar_bar2_init()` +
/// `nvkm_vmm_boot()` + `gv100_vmm_join()`:
///
/// 1. Write a V2 5-level page table hierarchy in VRAM via the PRAMIN window
/// 2. Write a GV100 instance block with PDB + subcontext entries
/// 3. Program BAR2_BLOCK in VIRTUAL mode pointing to the instance block
/// 4. Identity-map the first 2 MiB of virtual address space to IOVAs
///
/// The 2 MiB identity map covers all our DMA buffer IOVAs (instance block,
/// runlist, page tables, USERD, GPFIFO, NOP push buffer, fault buffer).
#[expect(
    clippy::cast_possible_truncation,
    reason = "VRAM offsets and BAR2 addresses fit in u32"
)]
pub(super) fn setup_bar2_page_table(bar0: &MappedBar) -> DriverResult<()> {
    let w = |reg: usize, val: u32| {
        bar0.write_u32(reg, val)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("BAR2 init {reg:#x}: {e}"))))
    };
    let r = |reg: usize| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

    let bar2_before = r(misc::PBUS_BAR2_BLOCK);
    tracing::info!(
        bar2_before = format_args!("{bar2_before:#010x}"),
        "BAR2 setup start"
    );

    // Steer PRAMIN window to our VRAM region.
    let old_bar0_win = r(misc::BAR0_WINDOW);
    let bar0_win_val = BAR2_VRAM_BASE >> 16;
    w(misc::BAR0_WINDOW, bar0_win_val)?;
    std::thread::sleep(std::time::Duration::from_millis(1));

    let pm = misc::PRAMIN_BASE;

    // Zero-fill 24 KiB (6 pages) — clears any stale data.
    for off in (0..0x6000).step_by(4) {
        let _ = w(pm + off, 0);
    }

    // ── PD3 → PD2 ──────────────────────────────────────────────────
    // GP100 PDE: (child_vram_addr >> 4) | (aperture << 1)
    // VRAM aperture = 1 in bits[2:1]
    let pd2_abs = BAR2_VRAM_BASE + BAR2_PD2_OFF;
    let pd3_pde = ((pd2_abs >> 4) as u64) | (1_u64 << 1);
    w(pm + BAR2_PD3_OFF as usize, pd3_pde as u32)?;
    w(pm + BAR2_PD3_OFF as usize + 4, (pd3_pde >> 32) as u32)?;

    // ── PD2 → PD1 ──────────────────────────────────────────────────
    let pd1_abs = BAR2_VRAM_BASE + BAR2_PD1_OFF;
    let pd2_pde = ((pd1_abs >> 4) as u64) | (1_u64 << 1);
    w(pm + BAR2_PD2_OFF as usize, pd2_pde as u32)?;
    w(pm + BAR2_PD2_OFF as usize + 4, (pd2_pde >> 32) as u32)?;

    // ── PD1 → PD0 ──────────────────────────────────────────────────
    let pd0_abs = BAR2_VRAM_BASE + BAR2_PD0_OFF;
    let pd1_pde = ((pd0_abs >> 4) as u64) | (1_u64 << 1);
    w(pm + BAR2_PD1_OFF as usize, pd1_pde as u32)?;
    w(pm + BAR2_PD1_OFF as usize + 4, (pd1_pde >> 32) as u32)?;

    // ── PD0[0] → SPT (dual entry: lo=small PT, hi=large PT) ────────
    // PD0 dual PDEs require bit 4 (SPT_PRESENT) — without it the MMU
    // ignores the page table pointer entirely.
    let spt_abs = BAR2_VRAM_BASE + BAR2_SPT_OFF;
    let pd0_small_pde = ((spt_abs >> 4) as u64) | (1_u64 << 1) | (1_u64 << 4);
    // PD0 entry 0, bytes [0:7] = small page PDE
    w(pm + BAR2_PD0_OFF as usize, pd0_small_pde as u32)?;
    w(pm + BAR2_PD0_OFF as usize + 4, (pd0_small_pde >> 32) as u32)?;
    // bytes [8:15] = large page PDE (unused, already zeroed)

    // ── SPT: identity-map 512 × 4 KiB pages (2 MiB) ────────────────
    // GP100 PTE: (phys_addr >> 4) | VALID(bit0) | aper(bits[2:1]) | VOL(bit3)
    // SYS_MEM_COHERENT: aper=2, VOL=1 → flags = 1 | (2<<1) | (1<<3) = 0xD
    const PTE_FLAGS: u64 = 0xD; // VALID + SYS_MEM_COH + VOL
    for i in 1_u32..512 {
        let iova = (i as u64) * 4096;
        let pte = (iova >> 4) | PTE_FLAGS;
        let off = BAR2_SPT_OFF as usize + (i as usize) * 8;
        w(pm + off, pte as u32)?;
        w(pm + off + 4, (pte >> 32) as u32)?;
    }

    // ── Instance block (PDB + GV100 subcontexts) ────────────────────
    let pd3_abs = BAR2_VRAM_BASE + BAR2_PD3_OFF;
    // PDB format (gf100_vmm_join_ + gp100_vmm_join):
    //   pd_addr | VER2(bit10) | BIG_PAGE_64K(bit11) | target[1:0]
    // For VRAM target: bits[1:0] = 0
    let pdb_lo = pd3_abs | (1 << 10) | (1 << 11); // VER2 + 64KiB bigpage
    let pdb_hi = 0_u32;

    w(pm + BAR2_INST_OFF as usize + 0x200, pdb_lo)?;
    w(pm + BAR2_INST_OFF as usize + 0x204, pdb_hi)?;

    // VA limit (BAR2 aperture size - 1). 32 MiB is typical for Volta.
    w(pm + BAR2_INST_OFF as usize + 0x208, 0x01FF_FFFF)?; // 32 MiB - 1
    w(pm + BAR2_INST_OFF as usize + 0x20C, 0)?;

    // GV100 subcontext setup (gv100_vmm_join):
    // inst+0x21C = 0 (ENGINE_WFI_VEID)
    w(pm + BAR2_INST_OFF as usize + 0x21C, 0)?;

    // Subcontext 0: copy main PDB; subcontexts 1-63: invalid (0x00000001)
    let mask: u64 = 1; // only subcontext 0 valid
    w(pm + BAR2_INST_OFF as usize + 0x298, mask as u32)?;
    w(pm + BAR2_INST_OFF as usize + 0x29C, (mask >> 32) as u32)?;

    // SC0 gets the real PDB
    w(pm + BAR2_INST_OFF as usize + 0x2A0, pdb_lo)?;
    w(pm + BAR2_INST_OFF as usize + 0x2A4, pdb_hi)?;
    w(pm + BAR2_INST_OFF as usize + 0x2A8, 0)?;

    // SCs 1-63: mark invalid
    for i in 1_u32..64 {
        let base = BAR2_INST_OFF as usize + 0x2A0 + (i as usize) * 0x10;
        w(pm + base, 0x0000_0001)?;
        w(pm + base + 4, 0x0000_0001)?;
        w(pm + base + 8, 0)?;
    }

    // ── Program BAR1_BLOCK + BAR2_BLOCK ────────────────────────────
    // Both BAR apertures share the same instance block and page tables.
    // Format: MODE_VIRTUAL(bit31) | TARGET_VID_MEM(bits[29:28]=0) | PTR(inst >> 12)
    let inst_abs = BAR2_VRAM_BASE + BAR2_INST_OFF;
    let bar_block_val = 0x8000_0000_u32 | (inst_abs >> 12);
    w(misc::PBUS_BAR1_BLOCK, bar_block_val)?;
    w(misc::PBUS_BAR2_BLOCK, bar_block_val)?;

    // Wait for both BAR binds to complete (BIND_STATUS register).
    // bits [0:1] = BAR1 pending/outstanding, bits [2:3] = BAR2 pending/outstanding.
    for _ in 0..100 {
        let status = r(misc::PBUS_BIND_STATUS);
        if status & 0xF == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    // Flush GPU MMU TLB so the new BAR2 page table takes effect immediately.
    // Without this, the first scheduling attempt may fault because stale TLB
    // entries don't reflect the new page table.
    // Matches nouveau's gf100_vmm_invalidate: wait for flush slot, write PDB
    // address to 0x100CB8 (already done by MMU_PHYS_SECURE), then trigger
    // invalidate via 0x100CBC with PAGE_ALL | HUB_ONLY flags.
    {
        // Wait for flush slot availability (NV_PFB_PRI_MMU_INVALIDATE counter).
        for _ in 0..200 {
            if r(0x100C80) & 0x00FF_0000 != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
        // Write PDB address for BAR2 (VRAM target=0, addr >> 12 << 4).
        let pdb_inv = (pd3_abs >> 12) << 4;
        w(0x100CB8, pdb_inv)?;
        w(0x100CEC, 0)?; // high 32 bits
        // Trigger TLB invalidate: PAGE_ALL(bit0) + HUB_ONLY(bit2) + trigger(bit31).
        w(0x100CBC, 0x8000_0005)?;
        // Wait for flush acknowledgement.
        for _ in 0..200 {
            if r(0x100C80) & 0x0000_8000 != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
    }

    // Restore BAR0_WINDOW.
    w(misc::BAR0_WINDOW, old_bar0_win)?;

    let bar2_after = r(misc::PBUS_BAR2_BLOCK);
    tracing::info!(
        bar2_before = format_args!("{bar2_before:#010x}"),
        bar2_after = format_args!("{bar2_after:#010x}"),
        "BAR2 page table configured in VRAM"
    );

    Ok(())
}
