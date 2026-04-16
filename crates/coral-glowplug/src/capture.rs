// SPDX-License-Identifier: AGPL-3.0-only
//! Training recipe capture — observe an external driver's memory training and
//! distill it into a replayable recipe for sovereign GPU boot.
//!
//! The capture flow:
//! 1. Cold BAR0 snapshot via sysfs oracle (device on vfio-pci)
//! 2. Traced swap to a warm driver (nouveau/nvidia) with kernel mmiotrace
//! 3. Settle time for driver initialization + memory training
//! 4. Warm BAR0 snapshot via sysfs oracle (warm driver holds device)
//! 5. Swap back to vfio-pci
//! 6. Diff cold vs warm → extract training writes per HBM2 domain
//! 7. Save recipe JSON to `/var/lib/coralreef/training/{chip}.json`

use coral_driver::vfio::channel::hbm2_training::{
    DomainCapture, GoldenCapture, capture_oracle_state,
};
use coral_driver::nv::chip::detect_from_boot0;

use crate::ember::EmberClient;
use crate::sovereign::{BootStep, StepStatus};
use crate::sysfs;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DEFAULT_TRAINING_DIR: &str = "/var/lib/coralreef/training";
const DEFAULT_SETTLE_SECS: u64 = 15;

/// A replayable training recipe captured from an external driver's init sequence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingRecipe {
    /// Chip codename derived from BOOT0 (e.g. "gv100", "gk210").
    pub chip: String,
    /// PCI BDF address used during capture.
    pub bdf: String,
    /// Driver that performed the memory training (e.g. "nouveau", "nvidia").
    pub warm_driver: String,
    /// BOOT0 register value from the cold GPU.
    pub cold_boot0: u32,
    /// Per-domain register writes that differ between cold and warm state.
    /// Ordered by HBM2 domain priority (FBPA → LTC → PCLOCK → PFB → ...).
    pub training_writes: Vec<DomainCapture>,
    /// Total number of register writes in the recipe.
    pub total_writes: usize,
    /// Path to the kernel mmiotrace file (if captured).
    pub mmiotrace_path: Option<String>,
    /// ISO-style timestamp of capture.
    pub timestamp: String,
}

impl TrainingRecipe {
    /// Load a training recipe from a JSON file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read recipe {}: {e}", path.display()))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("cannot parse recipe {}: {e}", path.display()))
    }

    /// Save this recipe to a JSON file, creating parent directories as needed.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create directory {}: {e}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialize recipe: {e}"))?;
        std::fs::write(path, json)
            .map_err(|e| format!("cannot write recipe {}: {e}", path.display()))
    }

    /// Flatten all domain writes into a single `(offset, value)` list.
    pub fn flat_writes(&self) -> Vec<(usize, u32)> {
        self.training_writes
            .iter()
            .flat_map(|d| d.registers.iter().copied())
            .collect()
    }
}

/// Result of the training capture flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureResult {
    /// PCI BDF address.
    pub bdf: String,
    /// Driver used for warming.
    pub warm_driver: String,
    /// The captured recipe (if successful).
    pub recipe_path: Option<String>,
    /// Total training writes captured.
    pub total_writes: usize,
    /// Per-step log.
    pub steps: Vec<BootStep>,
    /// Overall success.
    pub success: bool,
    /// Human-readable summary.
    pub summary: String,
}

/// Resolve the default recipe storage directory.
pub fn training_dir() -> PathBuf {
    std::env::var("CORALREEF_TRAINING_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_TRAINING_DIR))
}

/// Resolve the recipe file path for a given chip.
pub fn recipe_path_for_chip(chip: &str) -> PathBuf {
    training_dir().join(format!("{chip}.json"))
}

/// Auto-detect the best warm driver for a GPU.
///
/// Prefers nvidia (better memory training coverage) but falls back to nouveau.
fn auto_warm_driver() -> &'static str {
    if Path::new("/sys/module/nvidia").exists() {
        "nvidia"
    } else {
        "nouveau"
    }
}

/// Capture a training recipe by observing an external driver's memory initialization.
///
/// Orchestrates the full cold→warm→capture→diff→save flow. The GPU should be
/// on vfio-pci (held by ember) before calling this.
pub fn capture_training(bdf: &str, warm_driver: Option<&str>) -> CaptureResult {
    let mut steps = Vec::new();
    let start = std::time::Instant::now();
    let warm_driver: &str = match warm_driver {
        Some(d) => d,
        None => auto_warm_driver(),
    };

    let fail = |steps: Vec<BootStep>, summary: String| CaptureResult {
        bdf: bdf.to_string(),
        warm_driver: warm_driver.to_string(),
        recipe_path: None,
        total_writes: 0,
        steps,
        success: false,
        summary,
    };

    // Step 1: Verify driver state
    let step_start = std::time::Instant::now();
    let current_driver = sysfs::read_current_driver(bdf);
    let driver_name = current_driver.as_deref().unwrap_or("none");
    steps.push(BootStep {
        name: "detect_driver".into(),
        status: StepStatus::Ok,
        detail: Some(format!("driver={driver_name}")),
        duration_ms: step_start.elapsed().as_millis() as u64,
    });

    // Step 2: Cold BAR0 snapshot via sysfs oracle
    let step_start = std::time::Instant::now();
    let cold_snapshot = match capture_oracle_state(bdf) {
        Ok(snap) => {
            steps.push(BootStep {
                name: "cold_snapshot".into(),
                status: StepStatus::Ok,
                detail: Some(format!(
                    "boot0={:#010x} domains={} regs={}",
                    snap.boot0,
                    snap.domains.len(),
                    snap.register_count(),
                )),
                duration_ms: step_start.elapsed().as_millis() as u64,
            });
            snap
        }
        Err(e) => {
            steps.push(BootStep {
                name: "cold_snapshot".into(),
                status: StepStatus::Failed,
                detail: Some(format!("sysfs oracle failed: {e}")),
                duration_ms: step_start.elapsed().as_millis() as u64,
            });
            return fail(steps, format!("cold snapshot failed: {e}"));
        }
    };

    // Determine chip from BOOT0
    let chip_cap = detect_from_boot0(cold_snapshot.boot0);
    let chip = chip_cap.chip_name().to_string();

    // Step 3: Connect to ember
    let step_start = std::time::Instant::now();
    let ember = match EmberClient::connect_for_bdf(bdf) {
        Some(e) => {
            steps.push(BootStep {
                name: "connect_ember".into(),
                status: StepStatus::Ok,
                detail: None,
                duration_ms: step_start.elapsed().as_millis() as u64,
            });
            e
        }
        None => {
            steps.push(BootStep {
                name: "connect_ember".into(),
                status: StepStatus::Failed,
                detail: Some("no ember reachable for BDF".into()),
                duration_ms: step_start.elapsed().as_millis() as u64,
            });
            return fail(steps, "ember not reachable".into());
        }
    };

    // Step 4: Swap to warm driver WITH mmiotrace
    let step_start = std::time::Instant::now();
    let trace_path = match ember.swap_device_traced(bdf, warm_driver, true) {
        Ok(obs) => {
            let tp = obs.trace_path.clone();
            steps.push(BootStep {
                name: "warm_swap".into(),
                status: StepStatus::Ok,
                detail: Some(format!(
                    "target={warm_driver} total_ms={} trace={}",
                    obs.timing.total_ms,
                    tp.as_deref().unwrap_or("none"),
                )),
                duration_ms: step_start.elapsed().as_millis() as u64,
            });
            tp
        }
        Err(e) => {
            steps.push(BootStep {
                name: "warm_swap".into(),
                status: StepStatus::Failed,
                detail: Some(format!("swap to {warm_driver} failed: {e}")),
                duration_ms: step_start.elapsed().as_millis() as u64,
            });
            return fail(steps, format!("warm swap failed: {e}"));
        }
    };

    // Step 5: Settle — let the warm driver fully initialize
    let step_start = std::time::Instant::now();
    std::thread::sleep(std::time::Duration::from_secs(DEFAULT_SETTLE_SECS));
    steps.push(BootStep {
        name: "settle".into(),
        status: StepStatus::Ok,
        detail: Some(format!("{DEFAULT_SETTLE_SECS}s settle for {warm_driver} init")),
        duration_ms: step_start.elapsed().as_millis() as u64,
    });

    // Step 6: Warm BAR0 snapshot via sysfs oracle (warm driver holds device)
    let step_start = std::time::Instant::now();
    let warm_snapshot = match capture_oracle_state(bdf) {
        Ok(snap) => {
            steps.push(BootStep {
                name: "warm_snapshot".into(),
                status: StepStatus::Ok,
                detail: Some(format!(
                    "boot0={:#010x} domains={} regs={}",
                    snap.boot0,
                    snap.domains.len(),
                    snap.register_count(),
                )),
                duration_ms: step_start.elapsed().as_millis() as u64,
            });
            snap
        }
        Err(e) => {
            steps.push(BootStep {
                name: "warm_snapshot".into(),
                status: StepStatus::Failed,
                detail: Some(format!("sysfs oracle failed: {e}")),
                duration_ms: step_start.elapsed().as_millis() as u64,
            });
            // Still try to swap back before failing
            let _ = ember.swap_device(bdf, "vfio");
            return fail(steps, format!("warm snapshot failed: {e}"));
        }
    };

    // Step 7: Swap back to vfio-pci
    let step_start = std::time::Instant::now();
    match ember.swap_device(bdf, "vfio") {
        Ok(_) => {
            steps.push(BootStep {
                name: "swap_back_vfio".into(),
                status: StepStatus::Ok,
                detail: None,
                duration_ms: step_start.elapsed().as_millis() as u64,
            });
        }
        Err(e) => {
            steps.push(BootStep {
                name: "swap_back_vfio".into(),
                status: StepStatus::Failed,
                detail: Some(format!("swap back to vfio failed: {e}")),
                duration_ms: step_start.elapsed().as_millis() as u64,
            });
            return fail(steps, format!("swap back to vfio failed: {e}"));
        }
    }

    // Step 8: Diff cold vs warm to extract training writes
    let step_start = std::time::Instant::now();
    let training_writes = diff_snapshots(&cold_snapshot, &warm_snapshot);
    let total_writes: usize = training_writes.iter().map(|d| d.registers.len()).sum();
    steps.push(BootStep {
        name: "diff_snapshots".into(),
        status: if total_writes > 0 { StepStatus::Ok } else { StepStatus::Failed },
        detail: Some(format!(
            "{total_writes} training writes across {} domains",
            training_writes.len(),
        )),
        duration_ms: step_start.elapsed().as_millis() as u64,
    });

    if total_writes == 0 {
        return fail(steps, "no register differences found — driver may not have trained memory".into());
    }

    // Step 9: Build and save recipe
    let step_start = std::time::Instant::now();
    let recipe = TrainingRecipe {
        chip: chip.clone(),
        bdf: bdf.to_string(),
        warm_driver: warm_driver.to_string(),
        cold_boot0: cold_snapshot.boot0,
        training_writes,
        total_writes,
        mmiotrace_path: trace_path,
        timestamp: timestamp_now(),
    };

    let recipe_file = recipe_path_for_chip(&chip);
    match recipe.save(&recipe_file) {
        Ok(()) => {
            steps.push(BootStep {
                name: "save_recipe".into(),
                status: StepStatus::Ok,
                detail: Some(format!("{}", recipe_file.display())),
                duration_ms: step_start.elapsed().as_millis() as u64,
            });
        }
        Err(e) => {
            steps.push(BootStep {
                name: "save_recipe".into(),
                status: StepStatus::Failed,
                detail: Some(format!("save failed: {e}")),
                duration_ms: step_start.elapsed().as_millis() as u64,
            });
            return fail(steps, format!("recipe save failed: {e}"));
        }
    }

    let total_ms = start.elapsed().as_millis();
    CaptureResult {
        bdf: bdf.to_string(),
        warm_driver: warm_driver.to_string(),
        recipe_path: Some(recipe_file.display().to_string()),
        total_writes,
        steps,
        success: true,
        summary: format!(
            "captured {total_writes} training writes for {chip} via {warm_driver} → {} (total: {total_ms}ms)",
            recipe_file.display(),
        ),
    }
}

/// Diff two oracle snapshots to find registers the warm driver changed.
///
/// Compares per-domain: for each register present in both snapshots, includes
/// the *warm* value if it differs from cold. Also includes registers present
/// only in the warm snapshot (driver enabled new domains).
fn diff_snapshots(cold: &GoldenCapture, warm: &GoldenCapture) -> Vec<DomainCapture> {
    let mut diffs = Vec::new();

    for warm_domain in &warm.domains {
        let cold_regs: std::collections::HashMap<usize, u32> = cold
            .domains
            .iter()
            .find(|d| d.name == warm_domain.name)
            .map(|d| d.registers.iter().copied().collect())
            .unwrap_or_default();

        let mut domain_diffs = Vec::new();
        for &(off, warm_val) in &warm_domain.registers {
            let cold_val = cold_regs.get(&off).copied().unwrap_or(0xDEAD_DEAD);
            if cold_val != warm_val {
                domain_diffs.push((off, warm_val));
            }
        }

        if !domain_diffs.is_empty() {
            diffs.push(DomainCapture {
                name: warm_domain.name.clone(),
                registers: domain_diffs,
            });
        }
    }

    diffs
}

fn timestamp_now() -> String {
    let now = std::time::SystemTime::now();
    let dur = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}s", dur.as_secs())
}
