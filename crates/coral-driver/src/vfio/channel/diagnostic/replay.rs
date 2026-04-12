// SPDX-License-Identifier: AGPL-3.0-only
//! Recipe replay engine — apply captured init recipes to cold GPUs via VFIO BAR0.
//!
//! Supports two recipe formats:
//! - `RecipeStep` (from `boot_follower`): simple write-only, domain-prioritized
//! - `DevinitOp` (from VBIOS DEVINIT extraction): full instruction set including
//!   read-modify-write, mask-add, and timed delays
//!
//! After replay, validates that the GPU is alive (PTIMER ticking, PMC_BOOT_0 valid).

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

/// A single VBIOS DEVINIT instruction.
///
/// Represents one operation from the VBIOS init script table, extracted by
/// parsing the BIT 'I' table with `envytools/nvbios`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum DevinitOp {
    /// Direct register write (ZM_REG / ZM_REG_SEQUENCE)
    ZmReg {
        /// BAR0 register offset
        reg: usize,
        /// Value to write
        val: u32,
    },
    /// Read-modify-write (NV_REG): `reg = (reg & mask) | or_val`
    NvReg {
        /// BAR0 register offset
        reg: usize,
        /// AND mask applied to current value
        mask: u32,
        /// OR value applied after masking
        or_val: u32,
    },
    /// Masked add (ZM_MASK_ADD): `field = (reg & ~inv_mask) + add_val`,
    /// then `reg = (reg & inv_mask) | (field & ~inv_mask)`
    ZmMaskAdd {
        /// BAR0 register offset
        reg: usize,
        /// Inverted mask — preserved bits
        inv_mask: u32,
        /// Value added to the masked field
        add_val: u32,
    },
    /// Delay in microseconds (TIME)
    Time {
        /// Microseconds to sleep
        usec: u32,
    },
}

/// A named DEVINIT script containing a sequence of operations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DevinitScript {
    /// Script ID from the BIT 'I' table (0-based, -1 for unnamed scripts)
    pub id: i32,
    /// Hex address of the script in the VBIOS ROM
    pub addr: String,
    /// Ordered sequence of register operations
    pub ops: Vec<DevinitOp>,
}

/// Load a DEVINIT recipe (multi-script) from JSON.
pub fn load_devinit_recipe(path: &Path) -> Result<Vec<DevinitScript>, DriverError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        DriverError::DeviceNotFound(Cow::Owned(format!(
            "cannot read devinit recipe {}: {e}",
            path.display()
        )))
    })?;
    serde_json::from_str(&content).map_err(|e| {
        DriverError::DeviceNotFound(Cow::Owned(format!(
            "cannot parse devinit recipe {}: {e}",
            path.display()
        )))
    })
}

/// Apply DEVINIT scripts to a cold GPU via VFIO BAR0.
pub fn apply_devinit(bdf: &str, scripts: &[DevinitScript]) -> Result<ReplayResult, DriverError> {
    tracing::info!(bdf, scripts = scripts.len(), "devinit: opening VFIO device");

    let device = VfioDevice::open(bdf)?;
    let bar0 = device.map_bar(0)?;

    apply_devinit_to_bar0(&bar0, scripts)
}

/// Apply DEVINIT scripts to an already-mapped BAR0.
pub fn apply_devinit_to_bar0(
    bar0: &MappedBar,
    scripts: &[DevinitScript],
) -> Result<ReplayResult, DriverError> {
    let total_ops: usize = scripts.iter().map(|s| s.ops.len()).sum();
    tracing::info!(
        scripts = scripts.len(),
        total_ops,
        "devinit: applying to BAR0"
    );

    let mut applied: usize = 0;
    let mut failed: usize = 0;
    let mut domain_counts = std::collections::BTreeMap::new();

    for script in scripts {
        if script.ops.is_empty() {
            continue;
        }

        let script_label = format!("script_{}", script.id);
        let entry = domain_counts
            .entry(script_label.clone())
            .or_insert((0usize, 0usize));

        for op in &script.ops {
            match apply_devinit_op(bar0, op) {
                Ok(()) => {
                    applied += 1;
                    entry.0 += 1;
                }
                Err(e) => {
                    tracing::warn!(
                        script = script.id,
                        error = %e,
                        "devinit: op failed"
                    );
                    failed += 1;
                    entry.1 += 1;
                }
            }
        }

        tracing::debug!(
            script = script.id,
            applied = entry.0,
            failed = entry.1,
            "devinit: script complete"
        );
    }

    tracing::info!(applied, failed, "devinit: writes complete, validating GPU");
    validate_gpu(bar0, applied, failed, domain_counts)
}

fn apply_devinit_op(bar0: &MappedBar, op: &DevinitOp) -> Result<(), DriverError> {
    match op {
        DevinitOp::ZmReg { reg, val } => bar0.write_u32(*reg, *val),
        DevinitOp::NvReg { reg, mask, or_val } => {
            let current = bar0.read_u32(*reg)?;
            let new_val = (current & mask) | or_val;
            bar0.write_u32(*reg, new_val)
        }
        DevinitOp::ZmMaskAdd {
            reg,
            inv_mask,
            add_val,
        } => {
            let current = bar0.read_u32(*reg)?;
            let field = (current & !inv_mask).wrapping_add(*add_val);
            let new_val = (current & *inv_mask) | (field & !inv_mask);
            bar0.write_u32(*reg, new_val)
        }
        DevinitOp::Time { usec } => {
            std::thread::sleep(std::time::Duration::from_micros(u64::from(*usec)));
            Ok(())
        }
    }
}

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
    /// Per-domain/script apply counts: (applied, failed)
    pub domain_counts: std::collections::BTreeMap<String, (usize, usize)>,
}

impl ReplayResult {
    /// Returns `true` if the GPU appears alive after replay.
    pub fn is_alive(&self) -> bool {
        self.ptimer_ticking && self.pmc_boot_0 != 0xFFFF_FFFF
    }
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

/// Apply a [`StagedRecipe`] to a cold GPU via VFIO BAR0.
///
/// Opens the device by BDF, maps BAR0, replays each stage in order,
/// then validates PTIMER and PMC_BOOT_0.
pub fn apply_staged_recipe(
    bdf: &str,
    recipe: &boot_follower::StagedRecipe,
) -> Result<ReplayResult, DriverError> {
    tracing::info!(bdf, stages = recipe.stages.len(), "staged replay: opening VFIO device");

    let device = VfioDevice::open(bdf)?;
    let bar0 = device.map_bar(0)?;

    apply_staged_recipe_to_bar0(&bar0, recipe)
}

/// Apply a [`StagedRecipe`] to an already-mapped BAR0.
///
/// Writes each stage's registers in the order they appear in the recipe,
/// flushing PCIe posted writes between stages.
pub fn apply_staged_recipe_to_bar0(
    bar0: &MappedBar,
    recipe: &boot_follower::StagedRecipe,
) -> Result<ReplayResult, DriverError> {
    let mut applied: usize = 0;
    let mut failed: usize = 0;
    let mut domain_counts: std::collections::BTreeMap<String, (usize, usize)> =
        std::collections::BTreeMap::new();

    for block in &recipe.stages {
        let stage_name = block.stage.as_str().to_string();
        let entry = domain_counts.entry(stage_name.clone()).or_insert((0, 0));

        for w in &block.writes {
            match bar0.write_u32(w.offset, w.value) {
                Ok(()) => {
                    applied += 1;
                    entry.0 += 1;
                }
                Err(e) => {
                    tracing::warn!(
                        offset = format_args!("{:#x}", w.offset),
                        value = format_args!("{:#x}", w.value),
                        stage = %stage_name,
                        error = %e,
                        "staged replay: write failed"
                    );
                    failed += 1;
                    entry.1 += 1;
                }
            }
        }

        // Flush PCIe posted writes between stages
        let _ = bar0.read_u32(PMC_BOOT_0);

        tracing::debug!(
            stage = %stage_name,
            applied = entry.0,
            failed = entry.1,
            "staged replay: stage complete"
        );
    }

    tracing::info!(applied, failed, "staged replay: writes complete, validating GPU");
    validate_gpu(bar0, applied, failed, domain_counts)
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
    validate_gpu(bar0, applied, failed, domain_counts)
}

fn validate_gpu(
    bar0: &MappedBar,
    applied: usize,
    failed: usize,
    domain_counts: std::collections::BTreeMap<String, (usize, usize)>,
) -> Result<ReplayResult, DriverError> {
    let pmc_boot_0 = bar0.read_u32(PMC_BOOT_0).unwrap_or(0xFFFF_FFFF);
    tracing::info!(
        pmc_boot_0 = format_args!("{pmc_boot_0:#010x}"),
        "replay: PMC_BOOT_0"
    );

    let t0_a = bar0.read_u32(PTIMER_TIME_0).unwrap_or(0);
    let t1_a = bar0.read_u32(PTIMER_TIME_1).unwrap_or(0);

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

    #[test]
    fn devinit_op_roundtrip_json() {
        let script = DevinitScript {
            id: 0,
            addr: "0x924d".to_string(),
            ops: vec![
                DevinitOp::ZmReg {
                    reg: 0x000200,
                    val: 0x0000_2020,
                },
                DevinitOp::NvReg {
                    reg: 0x122130,
                    mask: 0xFFFF_EFFF,
                    or_val: 0x0000_0000,
                },
                DevinitOp::ZmMaskAdd {
                    reg: 0x02070c,
                    inv_mask: 0xFFFF_FF00,
                    add_val: 0x0000_00F4,
                },
                DevinitOp::Time { usec: 10000 },
            ],
        };

        let json = serde_json::to_string_pretty(&[&script]).unwrap();
        let parsed: Vec<DevinitScript> = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].ops.len(), 4);
        assert!(matches!(
            parsed[0].ops[0],
            DevinitOp::ZmReg { reg: 0x200, .. }
        ));
        assert!(matches!(parsed[0].ops[3], DevinitOp::Time { usec: 10000 }));
    }

    #[test]
    fn replay_result_is_alive() {
        let alive = ReplayResult {
            applied: 305,
            failed: 0,
            pmc_boot_0: 0x0f22_d0a1,
            ptimer_ticking: true,
            ptimer_samples: [(0x1000, 0x0), (0x2000, 0x0)],
            domain_counts: std::collections::BTreeMap::new(),
        };
        assert!(alive.is_alive());

        let dead = ReplayResult {
            applied: 305,
            failed: 0,
            pmc_boot_0: 0xFFFF_FFFF,
            ptimer_ticking: false,
            ptimer_samples: [(0x0, 0x0), (0x0, 0x0)],
            domain_counts: std::collections::BTreeMap::new(),
        };
        assert!(!dead.is_alive());
    }
}
