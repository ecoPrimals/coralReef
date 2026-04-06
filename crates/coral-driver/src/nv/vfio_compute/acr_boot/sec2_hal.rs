// SPDX-License-Identifier: AGPL-3.0-or-later

//! SEC2 falcon probing, EMEM/IMEM/DMEM helpers, and engine reset.

use std::fmt;

use crate::error::{DriverError, DriverResult};
use crate::vfio::channel::registers::{falcon, misc};
use crate::vfio::device::MappedBar;

use super::instance_block::{self, FALCON_INST_VRAM, SEC2_FLCN_BIND_INST};

mod falcon_mem_upload;

pub(crate) use falcon_mem_upload::sec2_dmem_read;
pub use falcon_mem_upload::{falcon_dmem_upload, falcon_imem_upload_nouveau};

// ── SEC2 state probing ────────────────────────────────────────────────

/// Classified SEC2 falcon state.
///
/// NOTE: SCTL (security mode) does NOT block host PIO to IMEM/DMEM/EMEM.
/// The IMEMC BIT(24) format discovery (Exp 091) proved PIO works normally
/// regardless of SCTL value. Security mode affects firmware authentication
/// and DMA behavior, not PIO access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sec2State {
    /// HS-locked (BIOS POST state): SCTL bit 0 set, firmware authentication active.
    /// PIO to IMEM/DMEM still works — use correct IMEMC format (BIT(24) write, BIT(25) read).
    HsLocked,
    /// Clean reset (post-PMC or post-unbind): SCTL bit 0 clear, no firmware loaded.
    CleanReset,
    /// Already running (mailbox active, firmware loaded).
    Running,
    /// Powered off or clock-gated (registers return PRI error).
    Inaccessible,
}

/// Detailed SEC2 probe result.
#[derive(Debug, Clone)]
pub struct Sec2Probe {
    /// SEC2 falcon `CPUCTL` register snapshot.
    pub cpuctl: u32,
    /// SEC2 `SCTL` (security mode): informational — does NOT gate PIO access.
    /// `Bits[13:12]` encode SEC_MODE (0=NS, 1=LS, 2=HS). Value 0x3000 on GV100
    /// indicates LS mode (fuse-enforced). PIO works regardless of this value.
    pub sctl: u32,
    /// SEC2 `BOOTVEC` — entry address for IMEM boot.
    pub bootvec: u32,
    /// SEC2 `HWCFG` — falcon hardware configuration.
    pub hwcfg: u32,
    /// SEC2 `MAILBOX0` — host/falcon command or status.
    pub mailbox0: u32,
    /// SEC2 `MAILBOX1` — command parameter or secondary status.
    pub mailbox1: u32,
    /// SEC2 program counter.
    pub pc: u32,
    /// SEC2 exception info register (trap/fault details).
    pub exci: u32,
    /// Classified SEC2 state from `cpuctl` / `sctl` / `mailbox0`.
    pub state: Sec2State,
}

impl Sec2Probe {
    /// Reads SEC2 falcon registers from BAR0 and classifies runtime state.
    pub fn capture(bar0: &MappedBar) -> Self {
        let base = falcon::SEC2_BASE;
        let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xBADF_DEAD);

        let cpuctl = r(falcon::CPUCTL);
        let sctl = r(falcon::SCTL);
        let bootvec = r(falcon::BOOTVEC);
        let hwcfg = r(falcon::HWCFG);
        let mailbox0 = r(falcon::MAILBOX0);
        let mailbox1 = r(falcon::MAILBOX1);
        let pc = r(falcon::PC);
        let exci = r(falcon::EXCI);

        let state = classify_sec2(cpuctl, sctl, mailbox0);

        Self {
            cpuctl,
            sctl,
            bootvec,
            hwcfg,
            mailbox0,
            mailbox1,
            pc,
            exci,
            state,
        }
    }
}

impl fmt::Display for Sec2Probe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SEC2 @ {:#010x}: {:?} cpuctl={:#010x} sctl={:#010x} bootvec={:#010x} \
             hwcfg={:#010x} mb0={:#010x} mb1={:#010x} pc={:#06x} exci={:#010x}",
            falcon::SEC2_BASE,
            self.state,
            self.cpuctl,
            self.sctl,
            self.bootvec,
            self.hwcfg,
            self.mailbox0,
            self.mailbox1,
            self.pc,
            self.exci
        )
    }
}

fn classify_sec2(cpuctl: u32, sctl: u32, mailbox0: u32) -> Sec2State {
    use crate::vfio::channel::registers::pri;
    if pri::is_pri_error(cpuctl) || cpuctl == 0xBADF_DEAD {
        return Sec2State::Inaccessible;
    }
    if mailbox0 != 0 && (cpuctl & falcon::CPUCTL_HALTED == 0) {
        return Sec2State::Running;
    }
    // SCTL bit 0 indicates HS authentication state. This is informational —
    // it does NOT block PIO access. The distinction matters for whether
    // host-loaded firmware will be accepted for HS operations.
    if sctl & 1 != 0 {
        Sec2State::HsLocked
    } else {
        Sec2State::CleanReset
    }
}

// ── SEC2 EMEM interface ──────────────────────────────────────────────

/// Write data to SEC2 EMEM via PIO (always writable, even in HS lockdown).
///
/// nouveau `gp102_flcn_pio_emem_wr_init`: BIT(24) only for write mode.
/// Auto-increment is implicit in the EMEM port hardware.
pub fn sec2_emem_write(bar0: &MappedBar, offset: u32, data: &[u8]) {
    let base = falcon::SEC2_BASE;
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    // BIT(24) = write mode (nouveau: gp102_flcn_pio_emem_wr_init)
    w(falcon::EMEMC0, (1 << 24) | offset);

    for chunk in data.chunks(4) {
        let word = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
            2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
            1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
            _ => 0,
        };
        w(falcon::EMEMD0, word);
    }
}

/// Read back data from SEC2 EMEM via PIO.
///
/// nouveau `gp102_flcn_pio_emem_rd_init`: BIT(25) only for read mode.
pub fn sec2_emem_read(bar0: &MappedBar, offset: u32, len: usize) -> Vec<u32> {
    let base = falcon::SEC2_BASE;
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);

    // BIT(25) = read mode (nouveau: gp102_flcn_pio_emem_rd_init)
    w(falcon::EMEMC0, (1 << 25) | offset);

    let word_count = len.div_ceil(4);
    (0..word_count).map(|_| r(falcon::EMEMD0)).collect()
}

/// Verify EMEM write by reading back and comparing.
pub fn sec2_emem_verify(bar0: &MappedBar, offset: u32, data: &[u8]) -> bool {
    let readback = sec2_emem_read(bar0, offset, data.len());
    for (i, chunk) in data.chunks(4).enumerate() {
        let expected = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
            2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
            1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
            _ => 0,
        };
        if i >= readback.len() || readback[i] != expected {
            tracing::error!(
                offset,
                word = i,
                expected = format!("{expected:#010x}"),
                got = format!("{:#010x}", readback.get(i).copied().unwrap_or(0xDEAD)),
                "EMEM verify mismatch"
            );
            return false;
        }
    }
    true
}
// ── SEC2 TRACEPC + exit diagnostics ─────────────────────────────

/// Read SEC2 TRACEPC circular buffer via indexed `EXCI`/`TRACEPC` registers.
///
/// The falcon TRACEPC buffer stores recent PC values. The count lives in
/// `EXCI[23:16]` (upper byte of the index field). To read entry `i`, write
/// `i` to `EXCI` and read `TRACEPC`.
///
/// Returns `(entry_count, entries)`.
pub fn sec2_tracepc_dump(bar0: &MappedBar) -> (u32, Vec<u32>) {
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    let tidx = r(falcon::EXCI);
    let count = ((tidx & 0x00FF_0000) >> 16).min(32);

    let entries: Vec<u32> = (0..count)
        .map(|i| {
            w(falcon::EXCI, i);
            r(falcon::TRACEPC)
        })
        .collect();

    (count, entries)
}

/// Unified SEC2 exit diagnostics — captures SCTL, EMEM, TRACEPC, and EXCI.
///
/// Called from [`super::sec2_queue::probe_and_bootstrap`] so all 13 strategy
/// exits get the same diagnostic data for cross-strategy comparison.
pub fn sec2_exit_diagnostics(bar0: &MappedBar, notes: &mut Vec<String>) {
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);

    let sctl = r(falcon::SCTL);
    let hs_mode = sctl & 0x02 != 0;
    let exci = r(falcon::EXCI);
    let pc = r(falcon::PC);
    notes.push(format!(
        "Exit diag: SCTL={sctl:#010x} HS={hs_mode} EXCI={exci:#010x} PC={pc:#06x}"
    ));

    let (trace_count, traces) = sec2_tracepc_dump(bar0);
    if trace_count > 0 {
        let trace_str: Vec<String> = traces.iter().map(|t| format!("{t:#06x}")).collect();
        notes.push(format!(
            "TRACEPC[0..{trace_count}]: {}",
            trace_str.join(" ")
        ));
    }

    let emem = sec2_emem_read(bar0, 0, 256);
    let nz_emem: Vec<String> = emem
        .iter()
        .enumerate()
        .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
        .take(24)
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", i * 4))
        .collect();
    if !nz_emem.is_empty() {
        notes.push(format!("EMEM(64w): {}", nz_emem.join(" ")));
    }
}

// ── SEC2 falcon-level reset ──────────────────────────────────────

/// Full engine reset: PMC-level disable/enable + falcon-local ENGCTL reset.
///
/// Nouveau's `gp102_flcn_reset_eng()` does a PMC engine reset FIRST,
/// then the falcon-local ENGCTL toggle. Without the PMC reset, the falcon
/// may not enter proper HRESET state and CPUCTL_STARTCPU has no effect.
///
/// For SEC2 on GV100: PMC_ENABLE (0x200) bit 22 = SEC2 engine.
/// Reset a Falcon microcontroller, matching Nouveau's `gm200_flcn_enable` sequence.
///
/// Order is critical — Nouveau does:
///   1. Falcon-local reset via ENGCTL pulse
///   2. PMC engine enable (for SEC2: ensures engine clock is running)
///   3. Scrub wait: poll `+0x10C` until bits `[2:1]` clear
///   4. Write GPU BOOT_0 chip ID to `+0x084`
///
/// Previous versions of this code did PMC disable+enable BEFORE the ENGCTL pulse,
/// which is the wrong order and may explain why SEC2 auto-started its ROM before
/// we could upload firmware.
pub fn falcon_engine_reset(bar0: &MappedBar, base: usize) -> DriverResult<()> {
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| -> DriverResult<()> {
        bar0.write_u32(base + off, val)
            .map_err(|e| DriverError::SubmitFailed(format!("falcon reset {off:#x}: {e}").into()))
    };

    // Step 0: Build VRAM page tables at 0x10000 BEFORE any bind attempt.
    // The bind_inst register points the falcon MMU to these page tables;
    // without valid tables, bind_stat stays at state 2 (MMU resolution failure).
    if base == falcon::SEC2_BASE {
        let pt_ok = instance_block::build_vram_falcon_inst_block(bar0);
        tracing::info!(ok = pt_ok, "VRAM page tables built at 0x10000");
    }

    w(falcon::ENGCTL, 0x01)?;

    // RACE Window A: Write bind_inst DURING reset (falcon logic disabled)
    if base == falcon::SEC2_BASE {
        let iv = instance_block::encode_bind_inst(FALCON_INST_VRAM as u64, 0);
        let _ = bar0.write_u32(base + SEC2_FLCN_BIND_INST, iv);
        let rba = bar0.read_u32(base + SEC2_FLCN_BIND_INST).unwrap_or(0xDEAD);
        tracing::info!(rb = format!("{rba:#010x}"), "bind_inst during reset");
    }

    std::thread::sleep(std::time::Duration::from_micros(10));
    w(falcon::ENGCTL, 0x00)?;

    // RACE Window B: Write bind_inst immediately after reset clear (before PMC)
    if base == falcon::SEC2_BASE {
        let iv = instance_block::encode_bind_inst(FALCON_INST_VRAM as u64, 0);
        let _ = bar0.write_u32(base + SEC2_FLCN_BIND_INST, iv);
        let rbb = bar0.read_u32(base + SEC2_FLCN_BIND_INST).unwrap_or(0xDEAD);
        tracing::info!(rb = format!("{rbb:#010x}"), "bind_inst post-reset-clear");
    }

    // Step 2: PMC engine reset (disable→enable) to fully restart ROM.
    // Just "enable" is a no-op when already enabled — the full toggle is
    // required for the ROM to run through scrub → HALTED properly.
    if base == falcon::SEC2_BASE {
        pmc_reset_sec2(bar0)?;

        // Wait for ROM to exit HRESET. The ROM runs automatically after
        // PMC enable but takes variable time to start executing.
        let hreset_start = std::time::Instant::now();
        let hreset_timeout = std::time::Duration::from_millis(3000);
        loop {
            let cpuctl_now = r(falcon::CPUCTL);
            if cpuctl_now & falcon::CPUCTL_HRESET == 0 {
                tracing::info!(
                    cpuctl = format!("{cpuctl_now:#010x}"),
                    elapsed_us = hreset_start.elapsed().as_micros(),
                    "SEC2 exited HRESET after PMC reset"
                );
                break;
            }
            if hreset_start.elapsed() > hreset_timeout {
                tracing::warn!(
                    cpuctl = format!("{cpuctl_now:#010x}"),
                    sctl = format!("{:#010x}", r(falcon::SCTL)),
                    pc = format!("{:#010x}", r(falcon::PC)),
                    "SEC2 did not exit HRESET after PMC reset (3s)"
                );
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }

        // Clear DMAIDX to VIRT before bind (nouveau: mask(0x604, 0x07, 0x00))
        let dmaidx = bar0
            .read_u32(base + instance_block::FALCON_DMAIDX)
            .unwrap_or(0);
        let _ = bar0.write_u32(base + instance_block::FALCON_DMAIDX, dmaidx & !0x07);

        // RACE Window C: Full bind sequence post-PMC-enable
        let iv = instance_block::encode_bind_inst(FALCON_INST_VRAM as u64, 0);
        let _ = bar0.write_u32(base + SEC2_FLCN_BIND_INST, iv);
        // Trigger: UNK090 bit 16 + ENG_CONTROL bit 3 (Exp 084 discovery)
        let unk090 = bar0
            .read_u32(base + instance_block::FALCON_UNK090)
            .unwrap_or(0);
        let _ = bar0.write_u32(base + instance_block::FALCON_UNK090, unk090 | 0x0001_0000);
        let eng = bar0
            .read_u32(base + instance_block::FALCON_ENG_CONTROL)
            .unwrap_or(0);
        let _ = bar0.write_u32(base + instance_block::FALCON_ENG_CONTROL, eng | 0x0000_0008);
        let _ = bar0.write_u32(base + falcon::DMACTL, 0x07);
        let rbc = bar0.read_u32(base + SEC2_FLCN_BIND_INST).unwrap_or(0xDEAD);
        let stat_c = bar0
            .read_u32(base + instance_block::FALCON_BIND_STAT)
            .unwrap_or(0xDEAD);
        tracing::info!(
            bind = format!("{rbc:#010x}"),
            bind_stat = format!("{stat_c:#010x}"),
            "bind_inst race post-PMC-enable (with triggers)"
        );
    }

    // Step 4: Read mailbox0 to clear any stale value before scrub wait
    // (matches nouveau gm200_flcn_reset_wait_mem_scrubbing sequence).
    let _ = bar0.read_u32(base + falcon::MAILBOX0);

    // Step 5: Wait for memory scrubbing (0x10C bits [2:1] = 0).
    let timeout = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();
    loop {
        let scrub = r(0x10C);
        if scrub & 0x06 == 0 {
            tracing::info!(
                scrub = format!("{scrub:#010x}"),
                elapsed_us = start.elapsed().as_micros(),
                "falcon memory scrub complete"
            );
            break;
        }
        if start.elapsed() > timeout {
            tracing::warn!(
                scrub = format!("{scrub:#010x}"),
                "falcon memory scrub timeout"
            );
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    // Step 6: Write BOOT_0 chip ID to falcon 0x084 (per nouveau gm200_flcn_enable).
    let boot0 = bar0.read_u32(0x000).unwrap_or(0);
    w(0x084, boot0)?;

    // Step 7: Wait for CPUCTL_HALTED — the ROM scrubs IMEM/DMEM then HALTs.
    // Nouveau: nvkm_falcon_v1_wait_for_halt(). This is CRITICAL: registers
    // like 0x668 (instance block binding) can only be written while HALTED.
    let halt_start = std::time::Instant::now();
    let halt_timeout = std::time::Duration::from_millis(500);
    let mut halted = false;
    loop {
        let cpuctl = r(falcon::CPUCTL);
        if cpuctl & falcon::CPUCTL_HALTED != 0 {
            halted = true;
            tracing::info!(
                cpuctl = format!("{cpuctl:#010x}"),
                elapsed_us = halt_start.elapsed().as_micros(),
                "falcon HALTED after scrub"
            );
            break;
        }
        if halt_start.elapsed() > halt_timeout {
            let sctl = r(falcon::SCTL);
            let pc = r(falcon::PC);
            tracing::warn!(
                cpuctl = format!("{cpuctl:#010x}"),
                sctl = format!("{sctl:#010x}"),
                pc = format!("{pc:#010x}"),
                "falcon did NOT halt after scrub (500ms timeout)"
            );
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    let cpuctl = r(falcon::CPUCTL);
    let sctl = r(falcon::SCTL);
    tracing::info!(
        cpuctl = format!("{cpuctl:#010x}"),
        sctl = format!("{sctl:#010x}"),
        halted,
        alias_en = cpuctl & (1 << 6) != 0,
        "falcon state after reset"
    );

    Ok(())
}
/// PMC enable for SEC2 engine (Nouveau: `nvkm_mc_enable`).
///
/// This is called AFTER the falcon-local ENGCTL reset to re-enable the
/// engine clock. Nouveau's `gm200_flcn_enable` does this as step 2,
/// after `reset_eng` and before `reset_wait_mem_scrubbing`.
///
/// Only ENABLES the engine — does not disable first. A full PMC
/// disable+enable cycle is a separate, more invasive operation.
pub(crate) fn pmc_enable_sec2(bar0: &MappedBar) -> DriverResult<()> {
    let sec2_bit = find_sec2_pmc_bit(bar0).unwrap_or(22);
    let sec2_mask = 1u32 << sec2_bit;

    let val = bar0.read_u32(misc::PMC_ENABLE).unwrap_or(0);
    let already_enabled = val & sec2_mask != 0;
    tracing::info!(
        pmc_enable = format!("{val:#010x}"),
        sec2_bit,
        already_enabled,
        "PMC SEC2 enable (post-reset)"
    );

    if !already_enabled {
        bar0.write_u32(misc::PMC_ENABLE, val | sec2_mask)
            .map_err(|e| DriverError::SubmitFailed(format!("PMC enable SEC2: {e}").into()))?;
        let _ = bar0.read_u32(misc::PMC_ENABLE);
        std::thread::sleep(std::time::Duration::from_micros(20));
    }

    Ok(())
}

/// Full PMC disable+enable cycle for SEC2 (more invasive than `pmc_enable_sec2`).
/// Used by strategies that need a complete engine power cycle.
pub(crate) fn pmc_reset_sec2(bar0: &MappedBar) -> DriverResult<()> {
    let sec2_bit = find_sec2_pmc_bit(bar0).unwrap_or(22);
    let sec2_mask = 1u32 << sec2_bit;

    let val = bar0.read_u32(misc::PMC_ENABLE).unwrap_or(0);
    tracing::info!(
        pmc_enable = format!("{val:#010x}"),
        sec2_bit,
        sec2_mask = format!("{sec2_mask:#010x}"),
        sec2_enabled = val & sec2_mask != 0,
        "PMC SEC2 reset: disabling engine"
    );

    bar0.write_u32(misc::PMC_ENABLE, val & !sec2_mask)
        .map_err(|e| DriverError::SubmitFailed(format!("PMC disable SEC2: {e}").into()))?;
    let _ = bar0.read_u32(misc::PMC_ENABLE);
    std::thread::sleep(std::time::Duration::from_micros(20));

    bar0.write_u32(misc::PMC_ENABLE, val | sec2_mask)
        .map_err(|e| DriverError::SubmitFailed(format!("PMC enable SEC2: {e}").into()))?;
    let _ = bar0.read_u32(misc::PMC_ENABLE);
    std::thread::sleep(std::time::Duration::from_micros(20));

    let after = bar0.read_u32(misc::PMC_ENABLE).unwrap_or(0);
    tracing::info!(
        pmc_after = format!("{after:#010x}"),
        sec2_enabled = after & sec2_mask != 0,
        "PMC SEC2 reset: engine re-enabled"
    );

    Ok(())
}

/// Scan PTOP table at 0x22700 to find SEC2's PMC reset/enable bits.
///
/// GV100 PTOP uses multi-entry sequences:
///
///   - type 0b01: engine definition (bits [11:2] = engine type, bits [15:12] = instance)
///   - type 0b10: fault info
///   - type 0b11: reset/enable info (bits [20:16] = reset bit, bits [25:21] = enable bit)
///
/// SEC2 = engine type 0x15 (decimal 21).
pub(crate) fn find_sec2_pmc_bit(bar0: &MappedBar) -> Option<u32> {
    let mut found_sec2 = false;
    let mut reset_bit = None;
    let mut enable_bit = None;

    for idx in 0..64u32 {
        let entry = bar0.read_u32(0x22700 + idx as usize * 4).unwrap_or(0);
        if entry == 0 || entry == 0xFFFFFFFF {
            if found_sec2 && (reset_bit.is_some() || enable_bit.is_some()) {
                break;
            }
            found_sec2 = false;
            continue;
        }

        let entry_type = entry & 0x3;

        if entry_type == 1 {
            // Engine definition entry
            let engine_type = (entry >> 2) & 0x3FF;
            if engine_type == 0x15 {
                found_sec2 = true;
                let instance = (entry >> 12) & 0xF;
                tracing::info!(
                    ptop_idx = idx,
                    entry = format!("{entry:#010x}"),
                    engine_type = format!("{engine_type:#x}"),
                    instance,
                    "PTOP: Found SEC2 engine entry"
                );
            } else if found_sec2 {
                break; // next engine, stop
            }
        } else if entry_type == 3 && found_sec2 {
            // Reset/enable info entry (follows engine def)
            let has_reset = entry & (1 << 14) != 0;
            let has_enable = entry & (1 << 15) != 0;
            let r_bit = (entry >> 16) & 0x1F;
            let e_bit = (entry >> 21) & 0x1F;
            tracing::info!(
                ptop_idx = idx,
                entry = format!("{entry:#010x}"),
                has_reset,
                reset_bit = r_bit,
                has_enable,
                enable_bit = e_bit,
                "PTOP: SEC2 reset/enable info"
            );
            if has_reset {
                reset_bit = Some(r_bit);
            }
            if has_enable {
                enable_bit = Some(e_bit);
            }
        } else if entry_type == 2 && found_sec2 {
            // Fault info entry
            let fault_id = (entry >> 2) & 0x1FF;
            tracing::info!(
                ptop_idx = idx,
                entry = format!("{entry:#010x}"),
                fault_id,
                "PTOP: SEC2 fault info"
            );
        }
    }

    // Dump ALL PTOP entries for debugging
    tracing::info!("PTOP table dump (entries 0-31):");
    for idx in 0..32u32 {
        let entry = bar0.read_u32(0x22700 + idx as usize * 4).unwrap_or(0);
        if entry != 0 && entry != 0xFFFFFFFF {
            let etype = entry & 0x3;
            tracing::info!(idx, entry = format!("{entry:#010x}"), etype, "PTOP[{idx}]");
        }
    }

    // Prefer enable_bit, fall back to reset_bit
    let result = enable_bit.or(reset_bit);
    if result.is_none() {
        tracing::warn!("SEC2 PMC bit not found in PTOP, using fallback bit 22");
    }
    result
}

/// Reset SEC2 falcon specifically.
pub fn reset_sec2(bar0: &MappedBar) -> DriverResult<()> {
    falcon_engine_reset(bar0, falcon::SEC2_BASE)
}

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

    // 1. PMC-level reset: clears falcon execution state (CPUCTL, EXCI, firmware).
    //    SCTL (security mode) is fuse-enforced on GV100 and survives PMC reset.
    match pmc_reset_sec2(bar0) {
        Ok(()) => notes.push("PMC SEC2 reset OK".to_string()),
        Err(e) => notes.push(format!("PMC SEC2 reset FAILED: {e}")),
    }

    w(falcon::ENGCTL, 0x01);
    std::thread::sleep(std::time::Duration::from_micros(10));
    w(falcon::ENGCTL, 0x00);
    notes.push("ENGCTL local reset pulse".to_string());

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
        if cpuctl & falcon::CPUCTL_HRESET != 0 {
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

    // 1. PMC-level reset
    match pmc_reset_sec2(bar0) {
        Ok(()) => notes.push("PMC SEC2 reset OK".to_string()),
        Err(e) => notes.push(format!("PMC SEC2 reset FAILED: {e}")),
    }

    w(falcon::ENGCTL, 0x01);
    std::thread::sleep(std::time::Duration::from_micros(10));
    w(falcon::ENGCTL, 0x00);
    notes.push("ENGCTL local reset pulse".to_string());

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
        if cpuctl & falcon::CPUCTL_HRESET != 0 {
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

/// Prepare a falcon for no-instance-block DMA (physical mode).
///
/// Matches Nouveau's `gm200_flcn_fw_load` for the non-instance path:
/// ```c
/// nvkm_falcon_mask(falcon, 0x624, 0x00000080, 0x00000080);
/// nvkm_falcon_wr32(falcon, 0x10c, 0x00000000);
/// ```
pub(crate) fn falcon_prepare_physical_dma(bar0: &MappedBar, base: usize) {
    let cur = bar0.read_u32(base + falcon::FBIF_TRANSCFG).unwrap_or(0);
    let _ = bar0.write_u32(
        base + falcon::FBIF_TRANSCFG,
        cur | falcon::FBIF_PHYSICAL_OVERRIDE,
    );
    let _ = bar0.write_u32(base + falcon::DMACTL, 0);
}
