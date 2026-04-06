// SPDX-License-Identifier: AGPL-3.0-or-later

//! PMC enable/reset and PTOP-based SEC2 bit discovery.

use crate::error::{DriverError, DriverResult};
use crate::vfio::channel::registers::misc;
use crate::vfio::device::MappedBar;

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
        if entry == 0 || entry == 0xFFFF_FFFF {
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
        if entry != 0 && entry != 0xFFFF_FFFF {
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
