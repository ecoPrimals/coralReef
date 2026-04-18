// SPDX-License-Identifier: AGPL-3.0-or-later

//! Falcon engine reset and SEC2 wrapper.

use crate::error::{DriverError, DriverResult};
use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

use super::super::instance_block::{self, FALCON_INST_VRAM, SEC2_FLCN_BIND_INST};
use super::pmc;

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
        pmc::pmc_reset_sec2(bar0)?;

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

    if !halted {
        super::falcon_cpu::falcon_pio_scrub_imem(bar0, base);
        tracing::info!("Manual IMEM/DMEM scrub (ROM did not halt)");
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
