// SPDX-License-Identifier: AGPL-3.0-only

//! SEC2 falcon probing, EMEM/IMEM/DMEM helpers, and engine reset.

use std::fmt;

use crate::error::{DriverError, DriverResult};
use crate::vfio::channel::registers::{falcon, misc};
use crate::vfio::device::MappedBar;

use super::instance_block::{self, FALCON_INST_VRAM, SEC2_FLCN_BIND_INST};

mod falcon_mem_upload;
mod pmc;
mod prepare;

pub(crate) use falcon_mem_upload::sec2_dmem_read;
pub(crate) use pmc::{find_sec2_pmc_bit, pmc_enable_sec2, pmc_reset_sec2};
pub use falcon_mem_upload::{falcon_dmem_upload, falcon_imem_upload_nouveau};
pub use prepare::{falcon_start_cpu, sec2_prepare_direct_boot, sec2_prepare_physical_first, sec2_prepare_v1};

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
    if mailbox0 != 0 && (cpuctl & falcon::CPUCTL_STOPPED == 0) {
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
/// For SEC2 on GV100 the PMC bit is read from PTOP (engine type 0x0d);
/// fallback to bit 22 if PTOP lookup fails.
/// Resets a Falcon microcontroller, matching Nouveau's `gm200_flcn_enable` sequence.
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

        // Wait for ROM to exit STOPPED state after PMC enable.
        // The ROM runs automatically; CPUCTL bit 5 (STOPPED) clears when
        // execution begins. After the ROM finishes scrubbing it halts
        // (bit 4 = HALTED), which Step 7 below waits for.
        let hreset_start = std::time::Instant::now();
        let hreset_timeout = std::time::Duration::from_millis(3000);
        loop {
            let cpuctl_now = r(falcon::CPUCTL);
            if cpuctl_now & falcon::CPUCTL_STOPPED == 0 {
                tracing::info!(
                    cpuctl = format!("{cpuctl_now:#010x}"),
                    elapsed_us = hreset_start.elapsed().as_micros(),
                    "SEC2 exited STOPPED state after PMC reset"
                );
                break;
            }
            if hreset_start.elapsed() > hreset_timeout {
                tracing::warn!(
                    cpuctl = format!("{cpuctl_now:#010x}"),
                    sctl = format!("{:#010x}", r(falcon::SCTL)),
                    pc = format!("{:#010x}", r(falcon::PC)),
                    "SEC2 stuck in STOPPED state after PMC reset (3s)"
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

    // Step 7: Wait for CPUCTL_HALTED (bit 4) — the ROM scrubs IMEM/DMEM
    // then halts. Nouveau `nvkm_falcon_v1_wait_for_halt` checks `cpuctl & 0x10`.
    // Registers like 0x668 (instance block binding) can only be written while
    // the falcon is halted/stopped.
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
                "falcon HALTED after scrub (bit 4 set)"
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

/// Reset SEC2 falcon specifically.
pub fn reset_sec2(bar0: &MappedBar) -> DriverResult<()> {
    falcon_engine_reset(bar0, falcon::SEC2_BASE)
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
