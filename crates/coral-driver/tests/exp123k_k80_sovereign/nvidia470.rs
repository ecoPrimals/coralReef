// SPDX-License-Identifier: AGPL-3.0-only
//! nvidia-470 cold→warm register recipe for Exp 123-K4.

use coral_driver::gsp::RegisterAccess;
use coral_driver::nv::bar0::Bar0Access;

use super::helpers::is_pri_fault;

/// Path to the nvidia-470 cold→warm diff JSON relative to the workspace data dir.
pub const NVIDIA470_DIFF: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/k80/nvidia470-captures/nvidia470_cold_warm_diff.json"
);

/// Apply register writes from nvidia-470 cold→warm diff to BAR0.
/// Skips PMC_ENABLE, PRI-fault sentinel values, and addresses above BAR0 range.
pub fn apply_nvidia470_recipe(bar0: &mut Bar0Access) -> (usize, usize) {
    let data = std::fs::read_to_string(NVIDIA470_DIFF)
        .unwrap_or_else(|e| panic!("Cannot read nvidia-470 diff: {e}"));

    let json: serde_json::Value =
        serde_json::from_str(&data).expect("Failed to parse nvidia-470 diff JSON");

    let skip = [0x200u32, 0x204]; // PMC_ENABLE, PMC_SPOON
    let mut writes = 0usize;
    let mut skipped = 0usize;

    let apply_section = |section: &serde_json::Value,
                         bar0: &mut Bar0Access,
                         writes: &mut usize,
                         skipped: &mut usize,
                         is_changed: bool| {
        if let Some(obj) = section.as_object() {
            for (_domain, regs) in obj {
                if let Some(regs_obj) = regs.as_object() {
                    for (addr_s, val_entry) in regs_obj {
                        let addr = u32::from_str_radix(addr_s.trim_start_matches("0x"), 16)
                            .unwrap_or(u32::MAX);
                        if addr >= 0x0100_0000 || skip.contains(&addr) {
                            *skipped += 1;
                            continue;
                        }

                        let val_str = if is_changed {
                            val_entry
                                .get("warm")
                                .and_then(|v| v.as_str())
                                .unwrap_or("0x0")
                        } else {
                            val_entry.as_str().unwrap_or("0x0")
                        };
                        let val =
                            u32::from_str_radix(val_str.trim_start_matches("0x"), 16).unwrap_or(0);

                        if is_pri_fault(val) {
                            *skipped += 1;
                            continue;
                        }

                        let _ = bar0.write_u32(addr, val);
                        *writes += 1;
                    }
                }
            }
        }
    };

    if let Some(added) = json.get("added") {
        apply_section(added, bar0, &mut writes, &mut skipped, false);
    }
    if let Some(changed) = json.get("changed") {
        apply_section(changed, bar0, &mut writes, &mut skipped, true);
    }

    (writes, skipped)
}
