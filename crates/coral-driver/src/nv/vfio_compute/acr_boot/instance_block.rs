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
/// Construct the bind_inst register value.
/// Format from nouveau `gm200_flcn_bind_inst`:
///   `(1 << 30) | (target << 28) | (addr >> 12)`
/// Target: 0=VRAM, 2=SYS_MEM_COHERENT, 3=SYS_MEM_NCOH.
pub fn encode_bind_inst(addr: u64, target: u32) -> u32 {
    (1u32 << 30) | (target << 28) | ((addr >> 12) as u32)
}

/// Execute the full nouveau-style falcon bind sequence (Exp 084 discovery).
///
/// Nouveau `gm200_flcn_bind_inst` + `gm200_flcn_fw_load` does ALL of these:
/// 1. Clear DMAIDX → VIRT
/// 2. Write CHANNEL_NEXT (0x054) with bind value
/// 3. Set UNK090 bit 16 (trigger)
/// 4. Set ENG_CONTROL bit 3 (trigger)
/// 5. Poll bind_stat `bits[14:12]` == 5
/// 6. Ack interrupt (0x004 bit 3)
/// 7. Set CHANNEL_TRIGGER LOAD (0x058 bit 1)
/// 8. Poll bind_stat `bits[14:12]` == 0
///
/// Returns (bind_ok, notes) where bind_ok is true if bind_stat reached 5.
/// Execute the full nouveau-style falcon bind sequence.
pub fn falcon_bind_context(
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

// ── Falcon v1 binding (GP102+/GV100 SEC2) ─────────────────────────────

/// Execute the falcon v1 + SEC2-specific bind sequence matching nouveau exactly.
///
/// Matches `nvkm_falcon_v1_bind_context` (nvkm/subdev/falcon/v1.c):
/// 1. Clear ITFEN (defensive — clear stale physical mode bits)
/// 2. Write instance block to falcon v1 register (0x480):
///    `target << 28 | (addr >> 12)` (NO bit 30, unlike gm200)
/// 3. Write ITFEN = 0x30 (bits 5:4 — triggers the context bind DMA walk)
///    NOTE: 0x10 is the UNBIND value; 0x30 is the BIND trigger!
/// 4. Poll IRQSTAT (0x008) bit 3 (bind completion interrupt)
/// 5. Ack interrupt: write 0x08 to IRQSCLR (0x004)
/// 6. Trigger channel load: write 0x02 to 0x058
/// 7. Poll bind_stat (0x0dc) bits [14:12] → 0
pub fn falcon_v1_bind_context(
    r: &dyn Fn(usize) -> u32,
    w: &dyn Fn(usize, u32),
    inst_addr: u64,
    target: u32,
) -> (bool, Vec<String>) {
    use crate::vfio::channel::registers::falcon;
    let mut notes = Vec::new();

    fn t(msg: &str) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/var/lib/coralreef/traces/ember_sec2_trace.log")
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(f, "[{ts}] {msg}");
            let _ = f.sync_all();
        }
    }

    // Step 1: Clear ITFEN (stale bit 2 from physical mode conflicts with v1 DMA)
    let itfen_before = r(falcon::ITFEN);
    w(falcon::ITFEN, 0);
    let itfen_cleared = r(falcon::ITFEN);
    t(&format!("BIND_V1: ITFEN clear {itfen_before:#010x} → {itfen_cleared:#010x}"));
    notes.push(format!(
        "v1 ITFEN: {itfen_before:#010x} → {itfen_cleared:#010x} (cleared)"
    ));

    // Step 2: Write instance block to 0x480
    let bind_val = (target << 28) | ((inst_addr >> 12) as u32);
    w(falcon::FALCON_V1_INST, bind_val);
    t(&format!("BIND_V1: inst wrote {bind_val:#010x} to 0x480 (addr={inst_addr:#x} target={target})"));
    notes.push(format!(
        "v1 inst: wrote {bind_val:#010x} to 0x480 (addr={inst_addr:#x} target={target})"
    ));

    // Step 3: ITFEN = 0x30 triggers the bind (nouveau: nvkm_falcon_wr32(0x048, 0x30))
    w(falcon::ITFEN, 0x30);
    let itfen_after = r(falcon::ITFEN);
    t(&format!("BIND_V1: ITFEN wrote 0x30, readback={itfen_after:#010x}"));
    notes.push(format!("v1 ITFEN→{itfen_after:#010x} (0x30 = bind trigger)"));

    // Step 4: Poll IRQSTAT bit 3 (bind completion interrupt)
    let start = std::time::Instant::now();
    let mut bind_irq = false;
    let mut last_irq = 0u32;
    loop {
        last_irq = r(falcon::IRQSTAT);
        if last_irq & 0x08 != 0 {
            bind_irq = true;
            break;
        }
        if start.elapsed() > std::time::Duration::from_millis(10) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(10));
    }

    if !bind_irq {
        let stat = r(FALCON_BIND_STAT);
        t(&format!("BIND_V1: IRQ TIMEOUT irq={last_irq:#010x} bind_stat={stat:#010x} itfen={:#010x}", r(falcon::ITFEN)));
        notes.push(format!(
            "v1 bind IRQ TIMEOUT (irq={last_irq:#010x} bind_stat={stat:#010x})"
        ));
        return (false, notes);
    }
    t(&format!("BIND_V1: IRQ OK {:?} irq={last_irq:#010x}", start.elapsed()));
    notes.push(format!(
        "v1 bind IRQ OK in {:?} (irq={last_irq:#010x})",
        start.elapsed()
    ));

    // Step 5: Ack interrupt (nouveau: nvkm_falcon_wr32(0x004, 0x08))
    w(falcon::IRQSCLR, 0x08);

    // Step 6: Trigger channel load (nouveau: nvkm_falcon_wr32(0x058, 0x02))
    w(FALCON_CHANNEL_TRIGGER, 0x02);

    // Step 7: Poll bind_stat [14:12] → 0
    let start2 = std::time::Instant::now();
    let mut final_ok = false;
    loop {
        let stat = r(FALCON_BIND_STAT);
        if stat & 0x7000 == 0 {
            final_ok = true;
            t(&format!("BIND_V1: bind_stat→0 OK {:?} ({stat:#010x})", start2.elapsed()));
            notes.push(format!(
                "v1 bind_stat→0 OK in {:?} ({stat:#010x})",
                start2.elapsed()
            ));
            break;
        }
        if start2.elapsed() > std::time::Duration::from_millis(10) {
            t(&format!("BIND_V1: bind_stat→0 TIMEOUT ({stat:#010x})"));
            notes.push(format!("v1 bind_stat→0 TIMEOUT ({stat:#010x})"));
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(10));
    }

    (final_ok, notes)
}

// ── VRAM Instance Block for Falcon DMA ────────────────────────────────
// VRAM addresses for the page table chain (below our ACR/WPR region)
/// VRAM address of the falcon instance block.
pub const FALCON_INST_VRAM: u32 = 0x10000;
/// VRAM address of PD3 (page directory level 3).
pub const FALCON_PD3_VRAM: u32 = 0x11000;
/// VRAM address of PD2 (page directory level 2).
pub const FALCON_PD2_VRAM: u32 = 0x12000;
/// VRAM address of PD1 (page directory level 1).
pub const FALCON_PD1_VRAM: u32 = 0x13000;
/// VRAM address of PD0 (page directory level 0).
pub const FALCON_PD0_VRAM: u32 = 0x14000;
/// VRAM address of the falcon page table (PT0).
pub const FALCON_PT0_VRAM: u32 = 0x15000;

/// Encode a non-leaf PDE pointing to a sub-directory in VRAM.
///
/// MMU v2 PDE format (from nova-core `dev_mmu.h`):
///   bit 0:      valid_inverted (0 = valid)
///   `bits[2:1]`:  aperture (1 = VideoMemory)
///   `bits[32:8]`: table_frame_vid = phys_addr >> 12
///
/// Result: `(phys >> 12) << 8 | aperture << 1` = `(phys >> 4) | 0x2`
pub fn encode_vram_pde(vram_addr: u64) -> u64 {
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
pub fn encode_vram_pte(vram_phys: u64) -> u64 {
    const VALID: u64 = 1;
    (vram_phys >> 4) | VALID
}

/// Encode a PTE pointing to system memory (SYS_MEM_COH) for the hybrid VRAM
/// page table approach. VRAM PDEs walk the page table chain in VRAM, but leaf
/// PTEs point to IOMMU-mapped system memory where the ACR/WPR data lives.
pub fn encode_sysmem_pte(iova: u64) -> u64 {
    const FLAGS: u64 = 1 | (2 << 1) | (1 << 3); // VALID + SYS_MEM_COH + VOL
    (iova >> 4) | FLAGS
}

/// Build a minimal VRAM-based instance block with identity-mapped page tables.
/// Returns true if successful. Maps first 2MB of VRAM so falcon DMA can
/// access VRAM addresses 0x0..0x200000 (covers our ACR payload + WPR).
pub fn build_vram_falcon_inst_block(bar0: &MappedBar) -> bool {
    // Crash-resilient trace: fsync after each page so we know exactly
    // where the system dies if it locks up during PRAMIN writes.
    fn pt_trace(msg: &str) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/var/lib/coralreef/traces/ember_sec2_trace.log")
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(f, "[{ts}] PT_BUILD: {msg}");
            let _ = f.sync_all();
        }
    }

    const REGION_BASE: u32 = FALCON_INST_VRAM; // 0x10000
    const REGION_END: u32 = FALCON_PT0_VRAM + 0x1000; // 0x16000
    const REGION_SIZE: usize = (REGION_END - REGION_BASE) as usize; // 0x6000

    pt_trace("creating single PRAMIN region 0x10000..0x16000");
    let mut rgn = match PraminRegion::new(bar0, REGION_BASE, REGION_SIZE) {
        Ok(r) => r,
        Err(e) => {
            pt_trace(&format!("PRAMIN region FAILED: {e}"));
            tracing::error!(%e, "build_vram_falcon_inst_block: PRAMIN region failed");
            return false;
        }
    };
    pt_trace("PRAMIN region created OK");

    let wv = |rgn: &mut PraminRegion, vram_addr: u32, offset: usize, val: u32| -> bool {
        let abs = (vram_addr - REGION_BASE) as usize + offset;
        rgn.write_u32(abs, val).is_ok()
    };
    let wv64 = |rgn: &mut PraminRegion, vram_addr: u32, offset: usize, val: u64| -> bool {
        let lo = (val & 0xFFFF_FFFF) as u32;
        let hi = (val >> 32) as u32;
        wv(rgn, vram_addr, offset, lo) && wv(rgn, vram_addr, offset + 4, hi)
    };

    // Zero each page with per-page tracing, read-back verification, and
    // PRI drain pacing. Each page = 1024 writes. Without pacing, 6144
    // rapid-fire PRAMIN writes saturate the PCIe posted-write buffer and
    // can trigger DPC (Downstream Port Containment) on the root port.
    let pages: [(&str, u32); 6] = [
        ("INST", FALCON_INST_VRAM),
        ("PD3", FALCON_PD3_VRAM),
        ("PD2", FALCON_PD2_VRAM),
        ("PD1", FALCON_PD1_VRAM),
        ("PD0", FALCON_PD0_VRAM),
        ("PT0", FALCON_PT0_VRAM),
    ];

    for (page_idx, (name, page_vram)) in pages.iter().enumerate() {
        pt_trace(&format!("zeroing page {page_idx}/6: {name} @ {page_vram:#x} (1024 writes)"));

        for off in (0..0x1000).step_by(4) {
            if !wv(&mut rgn, *page_vram, off, 0) {
                pt_trace(&format!("ZERO FAILED: {name} offset {off:#x}"));
                tracing::warn!(page_vram, off, "failed to zero page");
                return false;
            }

            // Insert a read-back every 256 writes to force PCIe completion
            // and prevent posted-write buffer overflow. The read is non-posted
            // and acts as a natural flow-control barrier.
            if off > 0 && off % 1024 == 0 {
                let abs = (*page_vram - REGION_BASE) as usize;
                let _ = rgn.read_u32(abs);
            }
        }

        // After each page: PRI drain + readback verification
        let _ = bar0.write_u32(0x0012_004C, 0x2);
        std::thread::sleep(std::time::Duration::from_micros(200));
        let abs = (*page_vram - REGION_BASE) as usize;
        let rb = rgn.read_u32(abs).unwrap_or(0xDEAD_DEAD);
        pt_trace(&format!("page {name} zeroed OK, readback[0]={rb:#010x}"));
    }

    pt_trace("all pages zeroed — writing PDEs");

    // PD3[0] → PD2
    if !wv64(&mut rgn, FALCON_PD3_VRAM, 0, 0) { return false; }
    if !wv64(&mut rgn, FALCON_PD3_VRAM, 8, encode_vram_pde(FALCON_PD2_VRAM as u64)) { return false; }
    // PD2[0] → PD1
    if !wv64(&mut rgn, FALCON_PD2_VRAM, 0, 0) { return false; }
    if !wv64(&mut rgn, FALCON_PD2_VRAM, 8, encode_vram_pde(FALCON_PD1_VRAM as u64)) { return false; }
    // PD1[0] → PD0
    if !wv64(&mut rgn, FALCON_PD1_VRAM, 0, 0) { return false; }
    if !wv64(&mut rgn, FALCON_PD1_VRAM, 8, encode_vram_pde(FALCON_PD0_VRAM as u64)) { return false; }
    // PD0[0] → PT0
    if !wv64(&mut rgn, FALCON_PD0_VRAM, 0, 0) { return false; }
    if !wv64(&mut rgn, FALCON_PD0_VRAM, 8, encode_vram_pde(FALCON_PT0_VRAM as u64)) { return false; }

    pt_trace("PDEs written — writing 512 PTEs");

    // PT0: identity-map 512 small pages (4KiB each = 2MiB total)
    for i in 0u64..512 {
        let phys = i * 4096;
        let pte = encode_vram_pte(phys);
        if !wv64(&mut rgn, FALCON_PT0_VRAM, (i as usize) * 8, pte) {
            pt_trace(&format!("PTE write FAILED at entry {i}"));
            return false;
        }
        // Read-back pacing every 64 PTEs
        if i > 0 && i % 64 == 0 {
            let abs = (FALCON_PT0_VRAM - REGION_BASE) as usize;
            let _ = rgn.read_u32(abs);
        }
    }

    pt_trace("PTEs written — writing instance block header");

    // Instance block: PAGE_DIR_BASE at RAMIN offset 0x200
    let pdb_lo: u32 = ((FALCON_PD3_VRAM >> 12) << 12)
        | (1 << 11) // BIG_PAGE_SIZE = 64KiB
        | (1 << 10) // USE_VER2_PT_FORMAT
        ;
    if !wv(&mut rgn, FALCON_INST_VRAM, 0x200, pdb_lo) { return false; }
    if !wv(&mut rgn, FALCON_INST_VRAM, 0x204, 0) { return false; }
    if !wv(&mut rgn, FALCON_INST_VRAM, 0x208, 0xFFFF_FFFF) { return false; }
    if !wv(&mut rgn, FALCON_INST_VRAM, 0x20C, 0x0001_FFFF) { return false; }

    // Verify key entries
    let rv = |rgn: &PraminRegion, vram_addr: u32, offset: usize| -> u32 {
        let abs = (vram_addr - REGION_BASE) as usize + offset;
        rgn.read_u32(abs).unwrap_or(0xDEAD)
    };
    let pdb_rb = rv(&rgn, FALCON_INST_VRAM, 0x200);
    let pd3_hi_lo = rv(&rgn, FALCON_PD3_VRAM, 8);

    pt_trace(&format!(
        "instance block complete: pdb={pdb_lo:#010x} rb={pdb_rb:#010x} pd3_hi={pd3_hi_lo:#010x}"
    ));

    tracing::info!(
        pdb_lo = format!("{pdb_lo:#010x}"),
        pdb_rb = format!("{pdb_rb:#010x}"),
        "VRAM falcon instance block built (paced, single-window)"
    );
    true
}
