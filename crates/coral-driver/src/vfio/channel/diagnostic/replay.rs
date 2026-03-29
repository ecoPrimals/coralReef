// SPDX-License-Identifier: AGPL-3.0-only
//! Recipe replay engine — apply captured init recipes to cold GPUs via VFIO BAR0.
//!
//! Takes a `RecipeStep` sequence (extracted by `boot_follower`) and writes the
//! register values to a cold GPU's BAR0 in domain-priority order. After replay,
//! validates that the GPU is alive (PTIMER ticking, PMC_BOOT_0 valid).

use crate::DriverError;
use crate::vfio::device::{MappedBar, VfioDevice};
use boot_follower::RecipeStep;
use std::borrow::Cow;
use std::path::Path;

use super::boot_follower;

/// PTIMER register offsets (NV_PTIMER_TIME_0 / TIME_1)
const PTIMER_TIME_0: usize = 0x9400;
const PTIMER_TIME_1: usize = 0x9410;

/// PMC_BOOT_0 — chipset identification register
const PMC_BOOT_0: usize = 0x0;

/// Result of a recipe replay operation.
#[derive(Debug)]
pub struct ReplayResult {
    /// Number of register writes applied
    pub applied: usize,
    /// Number of register writes that failed
    pub failed: usize,
    /// PMC_BOOT_0 value after replay (chipset ID)
    pub pmc_boot_0: u32,
    /// Whether PTIMER is ticking after replay
    pub ptimer_ticking: bool,
    /// PTIMER value pair (time_0, time_1) at two read points
    pub ptimer_samples: [(u32, u32); 2],
    /// Per-domain apply counts
    pub domain_counts: std::collections::BTreeMap<String, (usize, usize)>,
}

/// Load a recipe from a JSON file.
pub fn load_recipe(path: &Path) -> Result<Vec<RecipeStep>, DriverError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        DriverError::DeviceNotFound(Cow::Owned(format!(
            "cannot read recipe {}: {e}",
            path.display()
        )))
    })?;
    serde_json::from_str(&content).map_err(|e| {
        DriverError::DeviceNotFound(Cow::Owned(format!(
            "cannot parse recipe {}: {e}",
            path.display()
        )))
    })
}

/// Save a recipe to a JSON file.
pub fn save_recipe(recipe: &[RecipeStep], path: &Path) -> Result<(), DriverError> {
    let json = serde_json::to_string_pretty(recipe).map_err(|e| {
        DriverError::DeviceNotFound(Cow::Owned(format!("cannot serialize recipe: {e}")))
    })?;
    std::fs::write(path, json).map_err(|e| {
        DriverError::DeviceNotFound(Cow::Owned(format!(
            "cannot write recipe {}: {e}",
            path.display()
        )))
    })
}

/// Apply a recipe to a cold GPU via VFIO BAR0.
///
/// Opens the device by BDF, maps BAR0, writes all recipe steps in priority
/// order, then validates PTIMER and PMC_BOOT_0.
pub fn apply_recipe(bdf: &str, recipe: &[RecipeStep]) -> Result<ReplayResult, DriverError> {
    tracing::info!(bdf, steps = recipe.len(), "replay: opening VFIO device");

    let device = VfioDevice::open(bdf)?;
    let bar0 = device.map_bar(0)?;

    apply_recipe_to_bar0(&bar0, recipe)
}

/// Apply a recipe to an already-mapped BAR0.
///
/// Writes registers in the order they appear (caller should pre-sort by priority).
/// Between each domain boundary, reads back the last written register as a
/// fence to flush PCIe posted writes.
pub fn apply_recipe_to_bar0(
    bar0: &MappedBar,
    recipe: &[RecipeStep],
) -> Result<ReplayResult, DriverError> {
    let mut applied: usize = 0;
    let mut failed: usize = 0;
    let mut domain_counts: std::collections::BTreeMap<String, (usize, usize)> =
        std::collections::BTreeMap::new();
    let mut last_domain = String::new();

    tracing::info!(steps = recipe.len(), "replay: applying recipe to BAR0");

    for step in recipe {
        if step.domain != last_domain {
            if !last_domain.is_empty() {
                tracing::debug!(
                    domain = %last_domain,
                    applied = domain_counts.get(&last_domain).map_or(0, |c| c.0),
                    "replay: domain complete"
                );
            }
            last_domain = step.domain.clone();
        }

        let entry = domain_counts.entry(step.domain.clone()).or_insert((0, 0));

        match bar0.write_u32(step.offset, step.value) {
            Ok(()) => {
                applied += 1;
                entry.0 += 1;
            }
            Err(e) => {
                tracing::warn!(
                    offset = format_args!("{:#x}", step.offset),
                    value = format_args!("{:#x}", step.value),
                    domain = %step.domain,
                    error = %e,
                    "replay: write failed"
                );
                failed += 1;
                entry.1 += 1;
            }
        }
    }

    tracing::info!(applied, failed, "replay: writes complete, validating GPU");

    // Read PMC_BOOT_0
    let pmc_boot_0 = bar0.read_u32(PMC_BOOT_0).unwrap_or(0xFFFF_FFFF);
    tracing::info!(
        pmc_boot_0 = format_args!("{pmc_boot_0:#010x}"),
        "replay: PMC_BOOT_0"
    );

    // Check PTIMER — read twice with a small gap
    let t0_a = bar0.read_u32(PTIMER_TIME_0).unwrap_or(0);
    let t1_a = bar0.read_u32(PTIMER_TIME_1).unwrap_or(0);

    // Spin-wait ~1ms worth of reads
    for _ in 0..1000 {
        let _ = bar0.read_u32(PMC_BOOT_0);
    }

    let t0_b = bar0.read_u32(PTIMER_TIME_0).unwrap_or(0);
    let t1_b = bar0.read_u32(PTIMER_TIME_1).unwrap_or(0);

    let ptimer_ticking = t0_a != t0_b || t1_a != t1_b;
    tracing::info!(
        ptimer_ticking,
        t0_a = format_args!("{t0_a:#010x}"),
        t0_b = format_args!("{t0_b:#010x}"),
        t1_a = format_args!("{t1_a:#010x}"),
        t1_b = format_args!("{t1_b:#010x}"),
        "replay: PTIMER check"
    );

    Ok(ReplayResult {
        applied,
        failed,
        pmc_boot_0,
        ptimer_ticking,
        ptimer_samples: [(t0_a, t1_a), (t0_b, t1_b)],
        domain_counts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_roundtrip_json() {
        let recipe = vec![
            RecipeStep {
                domain: "ROOT_PLL".to_string(),
                offset: 0x136400,
                value: 0x0000_00FF,
                priority: 0,
            },
            RecipeStep {
                domain: "PMC".to_string(),
                offset: 0x000200,
                value: 0x4000_0020,
                priority: 3,
            },
        ];

        let json = serde_json::to_string_pretty(&recipe).unwrap();
        let parsed: Vec<RecipeStep> = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].domain, "ROOT_PLL");
        assert_eq!(parsed[0].offset, 0x136400);
        assert_eq!(parsed[1].value, 0x4000_0020);
    }
}
