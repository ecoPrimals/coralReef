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

/// Issue STARTCPU to a falcon, using CPUCTL_ALIAS if ALIAS_EN (bit 6) is set.
///
/// Matches Nouveau's `nvkm_falcon_v1_start`:
/// ```c
/// u32 reg = nvkm_falcon_rd32(falcon, 0x100);
/// if (reg & BIT(6))
///     nvkm_falcon_wr32(falcon, 0x130, 0x2);
/// else
///     nvkm_falcon_wr32(falcon, 0x100, 0x2);
/// ```
pub fn falcon_start_cpu(bar0: &MappedBar, base: usize) {
    let cpuctl = bar0.read_u32(base + falcon::CPUCTL).unwrap_or(0);
    let bootvec = bar0.read_u32(base + falcon::BOOTVEC).unwrap_or(0xDEAD);
    let alias_en = cpuctl & (1 << 6) != 0;
    tracing::info!(
        "falcon_start_cpu: base={:#x} cpuctl={:#010x} bootvec={:#010x} alias_en={}",
        base,
        cpuctl,
        bootvec,
        alias_en
    );
    if alias_en {
        let _ = bar0.write_u32(base + falcon::CPUCTL_ALIAS, falcon::CPUCTL_STARTCPU);
    } else {
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
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };
    let mut notes = Vec::new();

    let pre_cpuctl = r(falcon::CPUCTL);
    let pre_sctl = r(falcon::SCTL);
    notes.push(format!(
        "Pre-reset: cpuctl={pre_cpuctl:#010x} sctl={pre_sctl:#010x}"
    ));

    // ── Phase 0: Ensure PFIFO + PMU are enabled in PMC_ENABLE ──
    //
    // Post-nouveau, PMC_ENABLE may only have SEC2 + TOP. The falcon
    // instance block bind walks VRAM page tables via FBIF, and the
    // bind completion FSM depends on PFIFO fabric routing being alive.
    // PFB is always-on on GV100 (bit 16 is not accepted), but PFIFO
    // (bit 8) and PMU (bit 0) must be explicitly enabled.
    {
        let pmc_pre = bar0.read_u32(misc::PMC_ENABLE).unwrap_or(0);
        let pfifo_bit = 1u32 << 8;
        let pmu_bit = 1u32 << 0;
        let needed = pfifo_bit | pmu_bit;
        if pmc_pre & needed != needed {
            let pmc_new = pmc_pre | needed;
            let _ = bar0.write_u32(misc::PMC_ENABLE, pmc_new);
            std::thread::sleep(std::time::Duration::from_millis(5));
            let pmc_after = bar0.read_u32(misc::PMC_ENABLE).unwrap_or(0);
            tracing::info!(
                pmc_before = format_args!("{pmc_pre:#010x}"),
                pmc_after = format_args!("{pmc_after:#010x}"),
                "sec2_prepare_v1: enabled PFIFO+PMU in PMC_ENABLE"
            );
            notes.push(format!(
                "PMC_ENABLE: {pmc_pre:#010x} → {pmc_after:#010x} (PFIFO+PMU enabled)"
            ));
        } else {
            notes.push(format!("PMC_ENABLE: {pmc_pre:#010x} (PFIFO+PMU already set)"));
        }
    }

    // ── Phase 1: nvkm_falcon_reset (disable + enable) ──
    //
    // SAFETY: After MC disable, SEC2 PRI registers are DEAD. Any read to
    // SEC2 address space (0x87000+) while the engine is off can hang the
    // CPU forever in the PRI fabric. All writes during the disabled window
    // must be BLIND (no read-modify-write). PRI blind ACK after each PMC
    // toggle clears any cascading routing faults.

    const PRIV_RING_COMMAND: usize = 0x0012_004C;
    const PRIV_RING_CMD_ACK: u32 = 0x2;

    // Step 1a: nvkm_falcon_v1_disable → MC disable
    let sec2_bit = find_sec2_pmc_bit(bar0).unwrap_or(5);
    let sec2_mask = 1u32 << sec2_bit;
    let pmc = bar0.read_u32(misc::PMC_ENABLE).unwrap_or(0);
    if pmc & sec2_mask != 0 {
        let _ = bar0.write_u32(misc::PMC_ENABLE, pmc & !sec2_mask);
        let _ = bar0.read_u32(misc::PMC_ENABLE);
        std::thread::sleep(std::time::Duration::from_micros(20));
        // Blind ACK — disabling an engine can generate PRI routing faults
        let _ = bar0.write_u32(PRIV_RING_COMMAND, PRIV_RING_CMD_ACK);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    notes.push(format!("MC disable: PMC bit {sec2_bit} + PRI ACK"));

    // Step 1b: gp102_sec2_flcn_enable — ENGCTL toggle (SEC2-specific WAR)
    //
    // CRITICAL: SEC2 is DISABLED in PMC — DO NOT read any SEC2 registers.
    // The prior code did `r(ENGCTL) | 0x01` which reads from a dead engine
    // and hangs the CPU. Use blind writes only.
    let _ = bar0.write_u32(base + falcon::ENGCTL, 0x01);
    std::thread::sleep(std::time::Duration::from_micros(10));
    let _ = bar0.write_u32(base + falcon::ENGCTL, 0x00);
    notes.push("ENGCTL blind toggle (SEC2 pre-enable WAR — no reads)".to_string());

    // Step 1c: nvkm_falcon_v1_enable → MC enable
    let _ = bar0.write_u32(misc::PMC_ENABLE, pmc | sec2_mask);
    let _ = bar0.read_u32(misc::PMC_ENABLE);
    std::thread::sleep(std::time::Duration::from_micros(20));
    // Blind ACK — re-enabling can produce transient PRI faults as the
    // engine's clock domain comes up
    let _ = bar0.write_u32(PRIV_RING_COMMAND, PRIV_RING_CMD_ACK);
    std::thread::sleep(std::time::Duration::from_millis(5));
    notes.push("MC enable + PRI ACK".to_string());

    // Step 1d: nvkm_falcon_v1_wait_idle (wait for DMACTL scrub)
    let scrub_start = std::time::Instant::now();
    loop {
        let dmactl = r(falcon::DMACTL);
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

    // Step 1e: Write device boot0 to SEC2 debug register at 0x408
    let boot0 = bar0.read_u32(0x000).unwrap_or(0);
    w(falcon::SEC2_DEBUG, boot0);
    notes.push(format!(
        "Debug reg 0x408 ← {boot0:#010x}"
    ));

    // Wait for ROM to halt
    let halt_start = std::time::Instant::now();
    let mut halted = false;
    loop {
        let cpuctl = r(falcon::CPUCTL);
        if cpuctl & falcon::CPUCTL_HALTED != 0 {
            halted = true;
            notes.push(format!(
                "ROM halted in {:?} cpuctl={cpuctl:#010x}",
                halt_start.elapsed()
            ));
            break;
        }
        if halt_start.elapsed() > std::time::Duration::from_millis(3000) {
            notes.push(format!(
                "HALT timeout (3s) cpuctl={:#010x}",
                r(falcon::CPUCTL)
            ));
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    let post_cpuctl = r(falcon::CPUCTL);
    let post_sctl = r(falcon::SCTL);
    notes.push(format!(
        "Post-reset: cpuctl={post_cpuctl:#010x} sctl={post_sctl:#010x} halted={halted}"
    ));

    // ── Phase 2: FBIF_TRANSCFG → VRAM routing ──
    //
    // Before binding the instance block, route FBIF DMA to physical VRAM.
    // This breaks the circular dependency where the MMU walker needs FBIF to
    // read page tables from VRAM, but FBIF defaults to VIRT (which requires
    // the bind that hasn't completed yet). This matches nouveau's sequence:
    // nvkm_falcon_mask(falcon, 0x624, 0x03, 0x01)  [PHYS_VID]
    let fbif_before = r(falcon::FBIF_TRANSCFG);
    w(
        falcon::FBIF_TRANSCFG,
        (fbif_before & !0x03) | falcon::FBIF_TARGET_PHYS_VID,
    );
    let fbif_after = r(falcon::FBIF_TRANSCFG);
    notes.push(format!(
        "FBIF_TRANSCFG: {fbif_before:#010x} → {fbif_after:#010x} (PHYS_VID for VRAM PT walk)"
    ));

    // ── Phase 3: nvkm_falcon_bind_context (v1 path) ──

    // Build VRAM page tables first
    let pt_ok = instance_block::build_vram_falcon_inst_block(bar0);
    notes.push(format!("VRAM page tables: ok={pt_ok}"));

    let (bind_ok, bind_notes) = instance_block::falcon_v1_bind_context(
        &|off| r(off),
        &|off, val| w(off, val),
        FALCON_INST_VRAM as u64,
        0, // target = VRAM
    );
    notes.extend(bind_notes);

    notes.push(format!(
        "Bind: ok={bind_ok} ITFEN={:#010x} stat={:#010x}",
        r(falcon::ITFEN),
        r(instance_block::FALCON_BIND_STAT)
    ));

    (bind_ok && halted, notes)
}
