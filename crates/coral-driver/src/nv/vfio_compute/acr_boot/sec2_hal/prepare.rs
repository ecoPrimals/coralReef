// SPDX-License-Identifier: AGPL-3.0-only
//! High-level SEC2 preparation sequences for different boot strategies.

use crate::vfio::channel::registers::{falcon, misc};
use crate::vfio::device::MappedBar;

use super::pmc::{find_sec2_pmc_bit, pmc_reset_sec2};
use super::{falcon_prepare_physical_dma};
#[allow(unused_imports)]
use crate::nv::vfio_compute::acr_boot::instance_block::{
    self, FALCON_INST_VRAM, SEC2_FLCN_BIND_INST,
};

/// Prepare SEC2 falcon for direct ACR boot (bypass bootloader DMA).
///
/// After a primer (`attempt_nouveau_boot`), SEC2 is halted in the ROM's
/// exception handler. SCTL=0x3000 (LS mode) is fuse-enforced and does NOT
/// block PIO — IMEM/DMEM uploads work with the correct IMEMC format
/// (BIT(24) for write, BIT(25) for read). A PMC-level reset clears the
/// falcon execution state (CPUCTL, EXCI, firmware) but does not clear SCTL.
///
/// Sequence (matching nouveau's gm200_flcn_enable order):
/// 1. ENGCTL local reset (falcon-local engine reset, must come BEFORE PMC)
/// 2. PMC disable/enable — full hardware reset, ROM starts fresh
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

    // 1. ENGCTL local reset FIRST (nouveau: gp102_flcn_reset_eng, step 1).
    //    Must come BEFORE PMC enable — doing PMC first causes the ROM to
    //    auto-start, and the subsequent ENGCTL kills it mid-execution.
    //    Blind writes only — ENGCTL reads are dangerous if engine is transitioning.
    w(falcon::ENGCTL, 0x01);
    std::thread::sleep(std::time::Duration::from_micros(10));
    w(falcon::ENGCTL, 0x00);
    notes.push("ENGCTL blind reset pulse".to_string());

    // 2. PMC-level reset: clears falcon execution state (CPUCTL, EXCI, firmware).
    //    SCTL (security mode) is fuse-enforced on GV100 and survives PMC reset.
    //    After this, the ROM runs automatically through scrub → halt.
    //    pmc_reset_sec2 includes PRI blind ACK after each toggle.
    match pmc_reset_sec2(bar0) {
        Ok(()) => notes.push("PMC SEC2 reset OK (with PRI ACK)".to_string()),
        Err(e) => notes.push(format!("PMC SEC2 reset FAILED: {e}")),
    }

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

    // 4. Wait for ROM to halt (cpuctl bit 4 = HALTED on GV100).
    //    nouveau: nvkm_falcon_v1_wait_for_halt checks `cpuctl & 0x10`.
    let halt_start = std::time::Instant::now();
    loop {
        let cpuctl = r(falcon::CPUCTL);
        if cpuctl & falcon::CPUCTL_HALTED != 0 {
            notes.push(format!(
                "ROM halted in {:?} cpuctl={cpuctl:#010x}",
                halt_start.elapsed()
            ));
            break;
        }
        if halt_start.elapsed() > std::time::Duration::from_millis(3000) {
            notes.push(format!("HALT timeout (3s) cpuctl={cpuctl:#010x}"));
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    // Post-PMC diagnostic: verify secure mode is cleared
    let post_cpuctl = r(falcon::CPUCTL);
    let post_sctl = r(falcon::SCTL);
    let post_exci = r(falcon::EXCI);
    notes.push(format!(
        "Post-PMC-reset: cpuctl={post_cpuctl:#010x} sctl={post_sctl:#010x} exci={post_exci:#010x}"
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
/// Sequence (matching nouveau `gm200_flcn_enable` order):
/// 1. ENGCTL local reset (falcon-local engine reset)
/// 2. PMC-level reset (clears secure mode from prior primer, restarts ROM)
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

    // 1. ENGCTL local reset FIRST (nouveau: gp102_flcn_reset_eng, step 1).
    //    Blind writes only — reads from disabled/transitioning engine can PRI-hang CPU.
    let _ = bar0.write_u32(base + falcon::ENGCTL, 0x01);
    std::thread::sleep(std::time::Duration::from_micros(10));
    let _ = bar0.write_u32(base + falcon::ENGCTL, 0x00);
    notes.push("ENGCTL blind reset pulse".to_string());

    // 2. PMC-level reset (includes PRI blind ACK after each toggle).
    match pmc_reset_sec2(bar0) {
        Ok(()) => notes.push("PMC SEC2 reset OK (with PRI ACK)".to_string()),
        Err(e) => notes.push(format!("PMC SEC2 reset FAILED: {e}")),
    }

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

    // 4. Wait for ROM halt
    let halt_start = std::time::Instant::now();
    let mut halted = false;
    loop {
        let cpuctl = r(falcon::CPUCTL);
        if cpuctl & falcon::CPUCTL_HALTED != 0 {
            notes.push(format!(
                "ROM halted in {:?} cpuctl={cpuctl:#010x}",
                halt_start.elapsed()
            ));
            halted = true;
            break;
        }
        if halt_start.elapsed() > std::time::Duration::from_millis(3000) {
            notes.push(format!("HALT timeout (3s) cpuctl={cpuctl:#010x}"));
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
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
    falcon_prepare_physical_dma(bar0, base);
    let fbif_after = r(falcon::FBIF_TRANSCFG);
    let dmactl_after = r(falcon::DMACTL);
    notes.push(format!(
        "Physical DMA: FBIF_TRANSCFG={fbif_after:#010x} DMACTL={dmactl_after:#010x} (NO instance block)"
    ));

    (halted, notes)
}

/// Issue STARTCPU to a falcon, trying CPUCTL_ALIAS first then CPUCTL.
///
/// Nouveau's `gm200_flcn_start` writes STARTCPU to CPUCTL_ALIAS (0x130),
/// but empirically on GV100 without a bound instance block, CPUCTL_ALIAS
/// has no effect (falcon stays halted). CPUCTL (0x100) does trigger execution.
///
/// This function tries CPUCTL_ALIAS first (matching nouveau), checks if
/// the falcon started, and falls back to CPUCTL if it's still halted.
pub fn falcon_start_cpu(bar0: &MappedBar, base: usize) {
    let cpuctl = bar0.read_u32(base + falcon::CPUCTL).unwrap_or(0);
    let bootvec = bar0.read_u32(base + falcon::BOOTVEC).unwrap_or(0xDEAD);
    tracing::info!(
        "falcon_start_cpu: base={:#x} cpuctl={:#010x} bootvec={:#010x}",
        base,
        cpuctl,
        bootvec,
    );

    // Try CPUCTL_ALIAS first (nouveau's gm200_flcn_start)
    let _ = bar0.write_u32(base + falcon::CPUCTL_ALIAS, falcon::CPUCTL_STARTCPU);
    std::thread::sleep(std::time::Duration::from_millis(5));

    let cpuctl_after = bar0.read_u32(base + falcon::CPUCTL).unwrap_or(0);
    if cpuctl_after & falcon::CPUCTL_HALTED != 0 {
        tracing::warn!(
            "falcon_start_cpu: CPUCTL_ALIAS had no effect (still halted), falling back to CPUCTL"
        );
        let _ = bar0.write_u32(base + falcon::CPUCTL, falcon::CPUCTL_STARTCPU);
    }

    std::thread::sleep(std::time::Duration::from_millis(20));

    let pc_after = bar0.read_u32(base + falcon::PC).unwrap_or(0xDEAD);
    let exci_after = bar0.read_u32(base + falcon::EXCI).unwrap_or(0xDEAD);
    let cpuctl_after = bar0.read_u32(base + falcon::CPUCTL).unwrap_or(0xDEAD);
    if exci_after != 0 || pc_after == 0 {
        tracing::warn!(
            "falcon_start_cpu: POST-START FAULT base={:#x} pc={:#06x} exci={:#010x} cpuctl={:#010x}",
            base,
            pc_after,
            exci_after,
            cpuctl_after
        );
    } else {
        tracing::info!(
            "falcon_start_cpu: OK base={:#x} pc={:#06x} exci={:#010x} cpuctl={:#010x}",
            base,
            pc_after,
            exci_after,
            cpuctl_after
        );
    }
}

/// Prepare SEC2 for ACR boot using the correct falcon v1 register interface.
///
/// Matches nouveau's GV100 SEC2 ACR boot sequence exactly:
///   `nvkm_falcon_reset` → `gp102_sec2_flcn_enable` + `nvkm_falcon_v1_disable`
///   `nvkm_falcon_bind_context` → `nvkm_falcon_v1_bind_context` + SEC2 WAR
///
/// Corrects prior versions that used the gm200 register interface (0x054, 0x604)
/// which is wrong for falcon v1 (GP102+/GV100). The v1 interface uses:
///   - Instance block at register 0x480 (not 0x054)
///   - ITFEN bits [5:4] for DMA control (not DMAIDX at 0x604)
///   - Debug register at 0x408 (not 0x084)
pub fn sec2_prepare_v1(bar0: &MappedBar) -> (bool, Vec<String>) {
    // Fine-grained fsync trace — survives system lockups so we can read
    // the exact last successful BAR0 operation after a crash and reboot.
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

    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };
    let mut notes = Vec::new();

    let _ = std::fs::remove_file("/var/lib/coralreef/traces/ember_sec2_trace.log");
    t("sec2_prepare_v1 ENTER");

    // ══════════════════════════════════════════════════════════════════════
    // CRITICAL: PRAMIN writes MUST happen FIRST — before ANY register ops.
    //
    // On GV100, PRAMIN writes to VRAM via BAR0 0x700000 share the GPU's
    // internal memory arbiter with engine DMA paths. The PMC reset of SEC2
    // (toggling bit 5 in PMC_ENABLE) causes SEC2's engine logic to go
    // through a hardware init cycle. During and after this cycle, the
    // memory arbiter enters a state where host PRAMIN writes cause it to
    // deadlock — freezing the entire PCIe root complex (all CPU cores).
    //
    // The page table data is pure VRAM content that doesn't depend on any
    // register state. VRAM survives PMC resets (only the engine, not FBPA,
    // is affected). So we build the page tables while the GPU is in its
    // pristine nouveau-warmed state, THEN do the register sequence.
    //
    // Proven crash sequence: PMC reset → PRAMIN write → root complex freeze
    // Safe sequence: PRAMIN write → PMC reset → FBIF → bind
    // ══════════════════════════════════════════════════════════════════════

    // ── Phase 0: VRAM liveness check + PRAMIN writes (GPU pristine) ──

    const PRIV_RING_COMMAND: usize = 0x0012_004C;
    const PRIV_RING_CMD_ACK: u32 = 0x2;

    t("PRI_DRAIN: writing ACK to 0x12004C");
    let _ = bar0.write_u32(PRIV_RING_COMMAND, PRIV_RING_CMD_ACK);
    std::thread::sleep(std::time::Duration::from_millis(5));
    t("PRI_DRAIN: reading BOOT0");
    let boot0_check = bar0.read_u32(0x000).unwrap_or(0xDEAD_DEAD);
    t(&format!("PRI_DRAIN: BOOT0={boot0_check:#010x}"));
    if boot0_check == 0xDEAD_DEAD || boot0_check == 0xFFFF_FFFF {
        notes.push(format!(
            "PRI ring drain FAILED: BOOT0={boot0_check:#010x} — ring may be wedged"
        ));
        return (false, notes);
    }
    notes.push("PRI ring drained (ACK + BOOT0 verify OK)".to_string());

    // VRAM liveness probe via PRAMIN single-read (minimal PCIe exposure).
    // Only reads one u32 — no bulk writes that could stall the root complex.
    t("VRAM_CHECK: probing PRAMIN window (single read)");
    {
        let saved_window = bar0.read_u32(0x1700).unwrap_or(0xDEAD_DEAD);
        let _ = bar0.write_u32(0x1700, 0x0001u32); // window → VRAM 0x10000
        let probe = bar0.read_u32(0x0070_0000).unwrap_or(0xDEAD_DEAD);
        let _ = bar0.write_u32(0x1700, saved_window); // restore
        t(&format!("VRAM_CHECK: probe={probe:#010x}"));
        if probe & 0xFFF0_0000 == 0xBAD0_0000 || probe == 0xDEAD_DEAD {
            notes.push(format!(
                "VRAM DEAD: PRAMIN probe returned {probe:#010x} — FBPA uninitialized. \
                 Warm GPU with nouveau first."
            ));
            return (false, notes);
        }
        notes.push(format!("VRAM liveness: OK (probe={probe:#010x})"));
    }

    // Physical DMA mode: skip page table build entirely.
    // On GV100, ITFEN bits [5:4] are hardware-locked, so the falcon v1 bind
    // (which requires those bits) cannot work. Instead we use physical DMA:
    // FBIF → PHYS_VID + ITFEN bit 2 (physical mode, set by ROM). The falcon's
    // DMA engine accesses VRAM at physical addresses without page table walks.
    // This eliminates ~24KB of dangerous PRAMIN bulk writes.
    t("PT_BUILD: SKIPPED (physical DMA mode — no page tables needed)");
    notes.push("PT_BUILD: SKIPPED (using physical DMA mode)".to_string());

    // ── Phase 1: Diagnostics ──

    t("DIAG: reading SEC2 CPUCTL");
    let pre_cpuctl = r(falcon::CPUCTL);
    t(&format!("DIAG: SEC2 CPUCTL={pre_cpuctl:#010x}"));
    t("DIAG: reading SEC2 SCTL");
    let pre_sctl = r(falcon::SCTL);
    t(&format!("DIAG: SEC2 SCTL={pre_sctl:#010x}"));
    notes.push(format!(
        "Pre-reset: cpuctl={pre_cpuctl:#010x} sctl={pre_sctl:#010x}"
    ));

    let pre_itfen = r(falcon::ITFEN);
    t(&format!("DIAG: ITFEN={pre_itfen:#010x}"));
    notes.push(format!("Pre-reset ITFEN: {pre_itfen:#010x}"));

    {
        t("PMC0: reading PMC_ENABLE");
        let pmc_pre = bar0.read_u32(misc::PMC_ENABLE).unwrap_or(0);
        t(&format!("PMC0: PMC_ENABLE={pmc_pre:#010x} (PFIFO/PMU deferred)"));
        notes.push(format!(
            "PMC_ENABLE: {pmc_pre:#010x} (PFIFO+PMU deferred — not needed for SEC2)"
        ));
    }

    // ── Phase 2: nvkm_falcon_reset (disable + enable) ──

    // Step 2a: MC disable
    t("RESET_2a: find SEC2 PMC bit");
    let sec2_bit = find_sec2_pmc_bit(bar0).unwrap_or(5);
    let sec2_mask = 1u32 << sec2_bit;
    t(&format!("RESET_2a: SEC2 PMC bit={sec2_bit}"));
    t("RESET_2a: reading PMC_ENABLE");
    let pmc = bar0.read_u32(misc::PMC_ENABLE).unwrap_or(0);
    t(&format!("RESET_2a: PMC_ENABLE={pmc:#010x}"));
    if pmc & sec2_mask != 0 {
        let disable_val = pmc & !sec2_mask;
        t(&format!("RESET_2a: DISABLING SEC2 — writing PMC_ENABLE={disable_val:#010x}"));
        let _ = bar0.write_u32(misc::PMC_ENABLE, disable_val);
        t("RESET_2a: reading PMC_ENABLE (flush)");
        let _ = bar0.read_u32(misc::PMC_ENABLE);
        std::thread::sleep(std::time::Duration::from_micros(20));
        t("RESET_2a: PRI ACK after MC disable");
        let _ = bar0.write_u32(PRIV_RING_COMMAND, PRIV_RING_CMD_ACK);
        std::thread::sleep(std::time::Duration::from_millis(5));
        t("RESET_2a: MC disable complete");
    }
    notes.push(format!("MC disable: PMC bit {sec2_bit} + PRI ACK"));

    // Step 2b: MC enable (matching nouveau gp10b_flcn_reset_eng exactly:
    // just MC disable + 10us + MC enable — no ENGCTL toggle)
    std::thread::sleep(std::time::Duration::from_micros(10));
    let enable_val = pmc | sec2_mask;
    t(&format!("RESET_2b: ENABLING SEC2 — writing PMC_ENABLE={enable_val:#010x}"));
    let _ = bar0.write_u32(misc::PMC_ENABLE, enable_val);
    t("RESET_2b: reading PMC_ENABLE (flush)");
    let _ = bar0.read_u32(misc::PMC_ENABLE);
    std::thread::sleep(std::time::Duration::from_micros(20));
    t("RESET_2b: PRI ACK after MC enable");
    let _ = bar0.write_u32(PRIV_RING_COMMAND, PRIV_RING_CMD_ACK);
    std::thread::sleep(std::time::Duration::from_millis(5));
    t("RESET_2b: MC enable complete — SEC2 registers should be alive");
    notes.push("MC enable + PRI ACK (nouveau-exact: no ENGCTL toggle)".to_string());

    // Step 2d: Wait for DMACTL scrub
    t("SCRUB: polling DMACTL");
    let scrub_start = std::time::Instant::now();
    loop {
        let dmactl = r(falcon::DMACTL);
        t(&format!("SCRUB: DMACTL={dmactl:#010x}"));
        if dmactl & 0x06 == 0 {
            notes.push(format!(
                "Scrub done in {:?} DMACTL={dmactl:#010x}",
                scrub_start.elapsed()
            ));
            break;
        }
        if scrub_start.elapsed() > std::time::Duration::from_millis(500) {
            notes.push(format!("Scrub timeout DMACTL={dmactl:#010x}"));
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    // Step 2e: Write device boot0 to SEC2 debug register at 0x408
    t("DEBUG: reading BOOT0");
    let boot0 = bar0.read_u32(0x000).unwrap_or(0);
    t(&format!("DEBUG: BOOT0={boot0:#010x}, writing to SEC2_DEBUG (0x408)"));
    w(falcon::SEC2_DEBUG, boot0);
    t("DEBUG: SEC2_DEBUG written");
    notes.push(format!("Debug reg 0x408 ← {boot0:#010x}"));

    // Wait for ROM to halt
    t("HALT: polling CPUCTL for HALTED bit");
    let halt_start = std::time::Instant::now();
    let mut halted = false;
    loop {
        let cpuctl = r(falcon::CPUCTL);
        if cpuctl & falcon::CPUCTL_HALTED != 0 {
            halted = true;
            t(&format!("HALT: ROM halted cpuctl={cpuctl:#010x}"));
            notes.push(format!(
                "ROM halted in {:?} cpuctl={cpuctl:#010x}",
                halt_start.elapsed()
            ));
            break;
        }
        if halt_start.elapsed() > std::time::Duration::from_millis(3000) {
            t(&format!("HALT: TIMEOUT cpuctl={cpuctl:#010x}"));
            notes.push(format!(
                "HALT timeout (3s) cpuctl={:#010x}",
                r(falcon::CPUCTL)
            ));
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    t("POST_RESET: reading CPUCTL");
    let post_cpuctl = r(falcon::CPUCTL);
    t("POST_RESET: reading SCTL");
    let post_sctl = r(falcon::SCTL);
    let hs_mode = post_sctl & 0x02 != 0;
    t(&format!(
        "POST_RESET: cpuctl={post_cpuctl:#010x} sctl={post_sctl:#010x} halted={halted} hs={hs_mode}"
    ));
    notes.push(format!(
        "Post-reset: cpuctl={post_cpuctl:#010x} sctl={post_sctl:#010x} halted={halted} hs={hs_mode}"
    ));

    let itfen_post_reset = r(falcon::ITFEN);
    t(&format!("POST_RESET: ITFEN={itfen_post_reset:#010x}"));
    notes.push(format!("Post-reset ITFEN: {itfen_post_reset:#010x}"));

    // ── Phase 3: Configure DMA for physical VRAM access ──
    //
    // On GV100 SEC2, ITFEN bits [5:4] are hardware-locked (writes ignored).
    // The falcon ROM sets ITFEN = 0x04 (bit 2 = physical DMA mode).
    // Instead of the falcon v1 instance block bind (which requires bits 4-5),
    // we use physical DMA mode: FBIF → PHYS_VID + ITFEN bit 2 already set.
    // The falcon's DMA engine accesses VRAM at physical addresses directly.

    // Step 3a: FBIF_TRANSCFG → PHYS_VID (physical video memory target)
    t("FBIF: reading FBIF_TRANSCFG");
    let fbif_before = r(falcon::FBIF_TRANSCFG);
    let fbif_new = (fbif_before & !0xFF) | falcon::FBIF_TARGET_PHYS_VID
                                         | falcon::FBIF_PHYSICAL_OVERRIDE;
    t(&format!("FBIF: writing FBIF_TRANSCFG={fbif_new:#010x} (PHYS_VID + override)"));
    w(falcon::FBIF_TRANSCFG, fbif_new);
    let fbif_after = r(falcon::FBIF_TRANSCFG);
    t(&format!("FBIF: FBIF_TRANSCFG={fbif_before:#010x} → {fbif_after:#010x}"));
    notes.push(format!(
        "FBIF_TRANSCFG: {fbif_before:#010x} → {fbif_after:#010x} (PHYS_VID + override)"
    ));

    // Step 3b: Ensure ITFEN has ACCESS_EN (bit 0) + keep bit 2 (physical DMA)
    t("ITFEN: setting ACCESS_EN + physical DMA");
    let itfen_before = r(falcon::ITFEN);
    w(falcon::ITFEN, itfen_before | 0x05);
    let itfen_after = r(falcon::ITFEN);
    t(&format!("ITFEN: {itfen_before:#010x} → {itfen_after:#010x}"));
    notes.push(format!("ITFEN: {itfen_before:#010x} → {itfen_after:#010x}"));

    // Step 3c: Enable DMACTL
    w(falcon::DMACTL, 0x01);
    let dmactl_post = r(falcon::DMACTL);
    t(&format!("DMACTL: set to 0x01, readback={dmactl_post:#010x}"));
    notes.push(format!("DMACTL: {dmactl_post:#010x}"));

    // Step 3d: PRI drain + settle after FBIF reconfiguration
    t("POST_FBIF: PRI drain + settle");
    let _ = bar0.write_u32(PRIV_RING_COMMAND, PRIV_RING_CMD_ACK);
    std::thread::sleep(std::time::Duration::from_millis(10));
    let post_fbif_boot0 = bar0.read_u32(0x000).unwrap_or(0xDEAD_DEAD);
    t(&format!("POST_FBIF: BOOT0={post_fbif_boot0:#010x} after PRI drain + 10ms settle"));
    if post_fbif_boot0 == 0xDEAD_DEAD || post_fbif_boot0 == 0xFFFF_FFFF {
        notes.push(format!(
            "POST_FBIF: BOOT0={post_fbif_boot0:#010x} — PRI ring wedged after FBIF config"
        ));
        return (false, notes);
    }
    notes.push("PRI drained + settled 10ms after FBIF config".to_string());

    // Step 3e: On GV100, skip v1 bind (ITFEN bits 4-5 locked). Physical DMA
    // mode (ITFEN bit 2 + FBIF PHYS_VID) lets the falcon access VRAM directly
    // at physical addresses without a page table walk.
    let itfen_final = r(falcon::ITFEN);
    let dmactl_final = r(falcon::DMACTL);
    let fbif_final = r(falcon::FBIF_TRANSCFG);
    t(&format!(
        "FINAL: ITFEN={itfen_final:#010x} DMACTL={dmactl_final:#010x} FBIF={fbif_final:#010x} (physical DMA mode)"
    ));
    notes.push(format!(
        "Physical DMA: ITFEN={itfen_final:#010x} DMACTL={dmactl_final:#010x} FBIF={fbif_final:#010x}"
    ));

    t("sec2_prepare_v1 EXIT");
    (halted, notes)
}
