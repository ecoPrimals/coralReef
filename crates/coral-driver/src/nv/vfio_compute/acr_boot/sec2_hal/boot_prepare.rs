// SPDX-License-Identifier: AGPL-3.0-or-later

//! Direct-boot and physical-first SEC2 preparation sequences.

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

use super::super::instance_block::{self, FALCON_INST_VRAM};
use super::falcon_cpu;
use super::pmc;

/// Prepare SEC2 falcon for direct ACR boot (bypass bootloader DMA).
///
/// After a primer (`attempt_nouveau_boot`), SEC2 is halted in the ROM's
/// exception handler. SCTL=0x3000 (LS mode) is fuse-enforced and does NOT
/// block PIO — IMEM/DMEM uploads work with the correct IMEMC format
/// (BIT(24) for write, BIT(25) for read). A PMC-level reset clears the
/// falcon execution state (CPUCTL, EXCI, firmware) but does not clear SCTL.
///
/// Sequence (matching nouveau's gm200_flcn_enable + gm200_flcn_fw_load):
/// 1. PMC disable/enable — full hardware reset, clears execution state
/// 2. ENGCTL local reset — extra cleanup (nouveau does both)
/// 3. Wait for memory scrub completion (DMACTL `bits[2:1]`)
/// 4. Wait for ROM to halt (cpuctl bit 4)
/// 5. Enable ITFEN ACCESS_EN for DMA
/// 6. Full nouveau-style instance block bind
/// 7. Configure DMA registers
///
/// After this call, IMEM/DMEM are clean and the caller should upload ACR
/// firmware via PIO, set BOOTVEC, and call `falcon_start_cpu`.
///
/// VRAM contents (page tables at 0x10000, WPR at 0x70000) survive the
/// PMC reset because it only affects the SEC2 engine, not the frame buffer.
pub fn sec2_prepare_direct_boot(bar0: &MappedBar) -> (bool, Vec<String>) {
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };
    let mut notes = Vec::new();

    // 0. Diagnostic: capture pre-reset state
    let pre_cpuctl = r(falcon::CPUCTL);
    let pre_sctl = r(falcon::SCTL);
    let pre_exci = r(falcon::EXCI);
    notes.push(format!(
        "Pre-reset: cpuctl={pre_cpuctl:#010x} sctl={pre_sctl:#010x} exci={pre_exci:#010x}"
    ));

    // 1. ENGCTL local reset FIRST (while PMC is still enabled).
    //    Nouveau's gm200_flcn_enable does ENGCTL toggle before PMC enable.
    //    Doing PMC reset first starts the ROM, then ENGCTL kills it mid-scrub.
    w(falcon::ENGCTL, 0x01);
    std::thread::sleep(std::time::Duration::from_micros(10));
    w(falcon::ENGCTL, 0x00);
    notes.push("ENGCTL local reset pulse (before PMC)".to_string());

    // 2. PMC-level reset: power-cycles SEC2 engine → ROM starts fresh.
    //    SCTL (security mode) is fuse-enforced on GV100 and survives PMC reset.
    match pmc::pmc_reset_sec2(bar0) {
        Ok(()) => notes.push("PMC SEC2 reset OK".to_string()),
        Err(e) => notes.push(format!("PMC SEC2 reset FAILED: {e}")),
    }
    std::thread::sleep(std::time::Duration::from_micros(50));

    // 3. Wait for memory scrub (DMACTL bits [2:1] = 0).
    let scrub_start = std::time::Instant::now();
    loop {
        let scrub = r(falcon::DMACTL);
        if scrub & 0x06 == 0 {
            notes.push(format!("Scrub done in {:?}", scrub_start.elapsed()));
            break;
        }
        if scrub_start.elapsed() > std::time::Duration::from_millis(500) {
            notes.push(format!("Scrub timeout DMACTL={scrub:#010x}"));
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    // 4. Wait for ROM to halt — nouveau checks CPUCTL bit 4 (0x10) for halt.
    //    On GV100, bit 4 is HRESET/HALTED dual-purpose. ROM sets it to 1 after scrub.
    let halt_start = std::time::Instant::now();
    let mut rom_halted = false;
    loop {
        let cpuctl = r(falcon::CPUCTL);
        if cpuctl & falcon::CPUCTL_HRESET != 0 {
            notes.push(format!(
                "ROM halted in {:?} cpuctl={cpuctl:#010x}",
                halt_start.elapsed()
            ));
            rom_halted = true;
            break;
        }
        if halt_start.elapsed() > std::time::Duration::from_millis(500) {
            notes.push(format!("HALT timeout (500ms) cpuctl={cpuctl:#010x}"));
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    // Fallback: if ROM didn't halt, manually scrub IMEM via PIO to clear stale code.
    if !rom_halted {
        super::falcon_cpu::falcon_pio_scrub_imem(bar0, base);
        notes.push("Manual IMEM scrub (ROM did not halt)".to_string());
    }

    // Post-PMC diagnostic
    let post_cpuctl = r(falcon::CPUCTL);
    let post_sctl = r(falcon::SCTL);
    let post_exci = r(falcon::EXCI);
    notes.push(format!(
        "Post-reset: cpuctl={post_cpuctl:#010x} sctl={post_sctl:#010x} exci={post_exci:#010x}"
    ));

    // 5. Write BOOT_0 chip ID (per nouveau gm200_flcn_enable)
    let boot0 = bar0.read_u32(0x000).unwrap_or(0);
    w(0x084, boot0);

    let itfen_before = r(falcon::ITFEN);
    w(falcon::ITFEN, itfen_before | 0x01);
    let itfen_after = r(falcon::ITFEN);
    notes.push(format!("ITFEN: {itfen_before:#x} → {itfen_after:#x}"));

    // 7. Configure FBIF BEFORE bind — the bind walker reads page tables
    //    from VRAM via FBIF. If FBIF is in VIRT mode (0), the walker needs
    //    the page tables it's trying to bind (circular dependency, stalls at
    //    state 2). Setting FBIF to PHYS_VID (1) lets the walker access VRAM
    //    directly to read page tables.
    let fbif_before = r(falcon::FBIF_TRANSCFG);
    w(
        falcon::FBIF_TRANSCFG,
        (fbif_before & !0x03) | falcon::FBIF_TARGET_PHYS_VID,
    );
    w(falcon::DMACTL, 0x01);
    let fbif_after = r(falcon::FBIF_TRANSCFG);
    let dmactl_after = r(falcon::DMACTL);
    notes.push(format!(
        "FBIF→PHYS_VID: FBIF_TRANSCFG={fbif_before:#x}→{fbif_after:#x} DMACTL={dmactl_after:#x}"
    ));

    // 8. Full nouveau-style bind sequence (Exp 084 discovery)
    let bind_val = instance_block::encode_bind_inst(FALCON_INST_VRAM as u64, 0);
    let (bind_ok, bind_notes) =
        instance_block::falcon_bind_context(&|off| r(off), &|off, val| w(off, val), bind_val);
    notes.extend(bind_notes);
    notes.push(format!("Bind result: ok={bind_ok} val={bind_val:#010x}"));

    // 9. Diagnostic: check bind_stat and EXCI
    let bind_stat = r(instance_block::FALCON_BIND_STAT);
    let exci = r(falcon::EXCI);
    notes.push(format!(
        "Post-prepare: bind_stat={bind_stat:#010x} bits[14:12]={} EXCI={exci:#010x}",
        (bind_stat >> 12) & 0x7
    ));

    (bind_ok, notes)
}

/// Prepare SEC2 for physical-DMA-only boot — no instance block, no virtual addressing.
///
/// This mirrors nouveau's approach more faithfully than [`sec2_prepare_direct_boot`]:
/// nouveau boots the SEC2 bootloader with **physical DMA** and only sets up virtual
/// addressing later when the ACR firmware needs it. Our prior approach tried to build
/// a full instance block + page table chain before SEC2 was running, creating a
/// circular dependency (the MMU bind walker needs FBIF in physical mode to read the
/// page tables it's trying to bind).
///
/// Sequence:
/// 1. PMC-level reset (clears secure mode from prior primer)
/// 2. ENGCTL local reset (extra cleanup)
/// 3. Wait for memory scrub completion
/// 4. Wait for ROM halt
/// 5. Write BOOT_0 chip ID
/// 6. Enable ITFEN ACCESS_EN
/// 7. Set physical DMA mode (FBIF |= 0x80, clear DMACTL) — **NO instance block**
///
/// After this, the caller uploads BL to IMEM, BL descriptor to DMEM/EMEM with
/// physical VRAM addresses, sets BOOTVEC, and calls `falcon_start_cpu`.
pub fn sec2_prepare_physical_first(bar0: &MappedBar) -> (bool, Vec<String>) {
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };
    let mut notes = Vec::new();

    let pre_cpuctl = r(falcon::CPUCTL);
    let pre_sctl = r(falcon::SCTL);
    let pre_exci = r(falcon::EXCI);
    notes.push(format!(
        "Pre-reset: cpuctl={pre_cpuctl:#010x} sctl={pre_sctl:#010x} exci={pre_exci:#010x}"
    ));

    // 1. ENGCTL local reset FIRST (before PMC cycle).
    w(falcon::ENGCTL, 0x01);
    std::thread::sleep(std::time::Duration::from_micros(10));
    w(falcon::ENGCTL, 0x00);
    notes.push("ENGCTL local reset pulse (before PMC)".to_string());

    // 2. PMC-level reset: power-cycles SEC2 engine → ROM starts fresh.
    match pmc::pmc_reset_sec2(bar0) {
        Ok(()) => notes.push("PMC SEC2 reset OK".to_string()),
        Err(e) => notes.push(format!("PMC SEC2 reset FAILED: {e}")),
    }
    std::thread::sleep(std::time::Duration::from_micros(50));

    // 3. Wait for memory scrub (DMACTL bits [2:1] = 0)
    let scrub_start = std::time::Instant::now();
    loop {
        let scrub = r(falcon::DMACTL);
        if scrub & 0x06 == 0 {
            notes.push(format!("Scrub done in {:?}", scrub_start.elapsed()));
            break;
        }
        if scrub_start.elapsed() > std::time::Duration::from_millis(500) {
            notes.push(format!("Scrub timeout DMACTL={scrub:#010x}"));
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    // 4. Wait for ROM halt — nouveau checks CPUCTL bit 4 (0x10).
    let halt_start = std::time::Instant::now();
    let mut halted = false;
    loop {
        let cpuctl = r(falcon::CPUCTL);
        if cpuctl & falcon::CPUCTL_HRESET != 0 {
            notes.push(format!(
                "ROM halted in {:?} cpuctl={cpuctl:#010x}",
                halt_start.elapsed()
            ));
            halted = true;
            break;
        }
        if halt_start.elapsed() > std::time::Duration::from_millis(500) {
            notes.push(format!("HALT timeout (500ms) cpuctl={cpuctl:#010x}"));
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    // Fallback: manually scrub IMEM if ROM didn't halt
    if !halted {
        super::falcon_cpu::falcon_pio_scrub_imem(bar0, base);
        notes.push("Manual IMEM scrub (ROM did not halt)".to_string());
    }

    // Post-reset diagnostics
    let post_cpuctl = r(falcon::CPUCTL);
    let post_sctl = r(falcon::SCTL);
    let post_exci = r(falcon::EXCI);
    notes.push(format!(
        "Post-reset: cpuctl={post_cpuctl:#010x} sctl={post_sctl:#010x} exci={post_exci:#010x}"
    ));

    // 5. Write BOOT_0 chip ID
    let boot0 = bar0.read_u32(0x000).unwrap_or(0);
    w(0x084, boot0);

    let itfen_before = r(falcon::ITFEN);
    w(falcon::ITFEN, itfen_before | 0x01);
    let itfen_after = r(falcon::ITFEN);
    notes.push(format!("ITFEN: {itfen_before:#x} → {itfen_after:#x}"));

    // 7. Physical DMA mode — NO instance block bind
    //    FBIF_TRANSCFG[7] = 1 enables physical addressing for DMA
    //    DMACTL = 0 disables MMU-based DMA translation
    falcon_cpu::falcon_prepare_physical_dma(bar0, base);
    let fbif_after = r(falcon::FBIF_TRANSCFG);
    let dmactl_after = r(falcon::DMACTL);
    notes.push(format!(
        "Physical DMA: FBIF_TRANSCFG={fbif_after:#010x} DMACTL={dmactl_after:#010x} (NO instance block)"
    ));

    (halted, notes)
}
