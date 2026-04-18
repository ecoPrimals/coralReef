// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sovereign GPU boot orchestration.
//!
//! Sequences the full path from unknown GPU state to compute-ready:
//! detect current driver → warm if cold → swap to vfio-pci → run sovereign init.
//!
//! This lives in glowplug (the orchestrator) rather than ember (the MMIO gateway)
//! because it needs to coordinate sysfs driver state, ember swaps, and the
//! sovereign pipeline in a single transaction.

use serde::{Deserialize, Serialize};

use crate::capture;
use crate::ember::EmberClient;
use crate::sysfs;

/// Result of the full sovereign boot sequence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootResult {
    /// PCI BDF address of the device.
    pub bdf: String,
    /// Driver bound when we started.
    pub initial_driver: Option<String>,
    /// Whether we performed a warm cycle (nouveau/nvidia bind-unbind).
    pub warm_cycle_performed: bool,
    /// Driver bound after orchestration (should be "vfio-pci").
    pub final_driver: Option<String>,
    /// Sovereign init result from ember (raw JSON).
    pub sovereign_init: Option<serde_json::Value>,
    /// Path to captured training recipe (if capture was performed).
    pub capture_recipe_path: Option<String>,
    /// Per-step log of what the orchestrator did.
    pub steps: Vec<BootStep>,
    /// Overall success.
    pub success: bool,
    /// Human-readable summary.
    pub summary: String,
}

/// A single step in the boot orchestration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootStep {
    /// Step identifier (e.g. "detect_driver", "swap_to_vfio").
    pub name: String,
    /// Whether this step succeeded, was skipped, or failed.
    pub status: StepStatus,
    /// Human-readable detail about what happened.
    pub detail: Option<String>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
}

/// Status of an orchestration step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// Step completed successfully.
    Ok,
    /// Step was not needed and was skipped.
    Skipped,
    /// Step failed (see detail for cause).
    Failed,
}

/// Orchestrate the full sovereign boot for a GPU.
///
/// The vendor-ingredient loop:
/// 1. Detect current driver state from sysfs
/// 2. Connect to ember
/// 3. If cold on vfio-pci — warm via nouveau, then swap back
///    If warm source bound — swap to vfio-pci (preserves warm state)
///    If unbound — attempt warm cycle through ember
/// 4. Optionally capture training recipe (if no recipe exists for this chip)
/// 5. Run `ember.sovereign.init` (with golden_state_path if available)
/// 6. Report compute_ready status
pub fn sovereign_boot(bdf: &str) -> BootResult {
    let mut steps = Vec::new();
    let start = std::time::Instant::now();
    let mut warm_cycle_performed = false;
    let mut capture_recipe_path: Option<String> = None;

    let fail = |steps: Vec<BootStep>, summary: String| BootResult {
        bdf: bdf.to_string(),
        initial_driver: sysfs::read_current_driver(bdf),
        warm_cycle_performed: false,
        final_driver: sysfs::read_current_driver(bdf),
        sovereign_init: None,
        capture_recipe_path: None,
        steps,
        success: false,
        summary,
    };

    // ── Step 1: Detect current driver ──────────────────────────────
    let step_start = std::time::Instant::now();
    let initial_driver = sysfs::read_current_driver(bdf);
    steps.push(BootStep {
        name: "detect_driver".into(),
        status: StepStatus::Ok,
        detail: Some(format!(
            "driver={}",
            initial_driver.as_deref().unwrap_or("none")
        )),
        duration_ms: step_start.elapsed().as_millis() as u64,
    });

    let driver_name = initial_driver.as_deref().unwrap_or("none");
    let is_warm_source = matches!(driver_name, "nouveau" | "nvidia");
    let is_vfio = driver_name == "vfio-pci";

    // ── Step 2: Connect to ember ───────────────────────────────────
    let step_start = std::time::Instant::now();
    let ember = match EmberClient::connect_for_bdf(bdf) {
        Some(e) => e,
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
    steps.push(BootStep {
        name: "connect_ember".into(),
        status: StepStatus::Ok,
        detail: None,
        duration_ms: step_start.elapsed().as_millis() as u64,
    });

    // ── Step 3: Ensure GPU is warm on vfio-pci ─────────────────────
    if is_warm_source {
        // Already warm — swap to vfio-pci, preserving warm state
        let step_start = std::time::Instant::now();
        match ember.swap_device(bdf, "vfio") {
            Ok(obs) => {
                steps.push(BootStep {
                    name: "swap_to_vfio".into(),
                    status: StepStatus::Ok,
                    detail: Some(format!(
                        "from={} total_ms={}",
                        driver_name, obs.timing.total_ms
                    )),
                    duration_ms: step_start.elapsed().as_millis() as u64,
                });
            }
            Err(e) => {
                steps.push(BootStep {
                    name: "swap_to_vfio".into(),
                    status: StepStatus::Failed,
                    detail: Some(format!("swap failed: {e}")),
                    duration_ms: step_start.elapsed().as_millis() as u64,
                });
                return fail(steps, format!("swap to vfio-pci failed: {e}"));
            }
        }
    } else if is_vfio {
        // On vfio-pci — probe BAR0 to see if warm or cold
        let step_start = std::time::Instant::now();
        let probe_result = ember.simple_rpc(
            "mmio.bar0.probe",
            serde_json::json!({"bdf": bdf}),
        );
        match probe_result {
            Ok(probe) => {
                let pmc = probe["pmc_enable"].as_str().unwrap_or("0x0");
                let pmc_val = u32::from_str_radix(
                    pmc.strip_prefix("0x").unwrap_or(pmc),
                    16,
                ).unwrap_or(0);
                let is_cold = pmc_val.count_ones() < 8;

                steps.push(BootStep {
                    name: "bar0_probe".into(),
                    status: StepStatus::Ok,
                    detail: Some(format!(
                        "pmc_enable={pmc} warm={}",
                        if is_cold { "no" } else { "yes" }
                    )),
                    duration_ms: step_start.elapsed().as_millis() as u64,
                });

                if is_cold {
                    // Cold GPU — vendor-ingredient loop: swap to nouveau for training
                    let step_start = std::time::Instant::now();
                    match ember.swap_device(bdf, "nouveau") {
                        Ok(obs) => {
                            steps.push(BootStep {
                                name: "warm_swap_nouveau".into(),
                                status: StepStatus::Ok,
                                detail: Some(format!(
                                    "total_ms={}",
                                    obs.timing.total_ms
                                )),
                                duration_ms: step_start.elapsed().as_millis() as u64,
                            });
                        }
                        Err(e) => {
                            steps.push(BootStep {
                                name: "warm_swap_nouveau".into(),
                                status: StepStatus::Failed,
                                detail: Some(format!("swap to nouveau failed: {e}")),
                                duration_ms: step_start.elapsed().as_millis() as u64,
                            });
                            return fail(steps, format!("warm swap to nouveau failed: {e}"));
                        }
                    }

                    // Wait for nouveau to train HBM2
                    let step_start = std::time::Instant::now();
                    std::thread::sleep(std::time::Duration::from_secs(15));
                    steps.push(BootStep {
                        name: "settle_training".into(),
                        status: StepStatus::Ok,
                        detail: Some("15s settle for nouveau HBM2 training".into()),
                        duration_ms: step_start.elapsed().as_millis() as u64,
                    });

                    // Swap back to vfio-pci
                    let step_start = std::time::Instant::now();
                    match ember.swap_device(bdf, "vfio") {
                        Ok(obs) => {
                            warm_cycle_performed = true;
                            steps.push(BootStep {
                                name: "swap_back_vfio".into(),
                                status: StepStatus::Ok,
                                detail: Some(format!(
                                    "total_ms={}",
                                    obs.timing.total_ms
                                )),
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
                            return fail(steps, format!("swap back to vfio-pci failed: {e}"));
                        }
                    }
                }
            }
            Err(e) => {
                steps.push(BootStep {
                    name: "bar0_probe".into(),
                    status: StepStatus::Failed,
                    detail: Some(format!("probe failed: {e}")),
                    duration_ms: step_start.elapsed().as_millis() as u64,
                });
            }
        }
    } else {
        // Unbound or unknown driver — attempt full warm cycle
        let step_start = std::time::Instant::now();
        match ember.warm_cycle(bdf) {
            Ok(()) => {
                warm_cycle_performed = true;
                steps.push(BootStep {
                    name: "warm_cycle".into(),
                    status: StepStatus::Ok,
                    detail: Some("nouveau warm cycle completed".into()),
                    duration_ms: step_start.elapsed().as_millis() as u64,
                });
            }
            Err(e) => {
                steps.push(BootStep {
                    name: "warm_cycle".into(),
                    status: StepStatus::Failed,
                    detail: Some(format!("warm cycle failed: {e} — will attempt cold init")),
                    duration_ms: step_start.elapsed().as_millis() as u64,
                });
            }
        }
    }

    // ── Step 4: Check for existing training recipe ─────────────────
    let recipe_path = capture::recipe_path_for_chip("gv100");
    let golden_state_path = if recipe_path.exists() {
        let path_str = recipe_path.display().to_string();
        steps.push(BootStep {
            name: "load_recipe".into(),
            status: StepStatus::Ok,
            detail: Some(format!("existing recipe: {path_str}")),
            duration_ms: 0,
        });
        capture_recipe_path = Some(path_str.clone());
        Some(path_str)
    } else {
        steps.push(BootStep {
            name: "load_recipe".into(),
            status: StepStatus::Skipped,
            detail: Some("no cached recipe — using warm state directly".into()),
            duration_ms: 0,
        });
        None
    };

    // ── Step 5: Sovereign init via ember ───────────────────────────
    let step_start = std::time::Instant::now();
    let mut init_params = serde_json::json!({"bdf": bdf});
    if let Some(ref gsp) = golden_state_path {
        init_params["golden_state_path"] = serde_json::Value::String(gsp.clone());
    }
    let init_result = ember.simple_rpc_with_timeout(
        "ember.sovereign.init",
        init_params,
        std::time::Duration::from_secs(120),
    );

    let (sovereign_init, init_ok, init_summary) = match init_result {
        Ok(result) => {
            let all_ok = result["all_ok"].as_bool().unwrap_or(false);
            let compute = result["compute_ready"].as_bool().unwrap_or(false);
            let warm = result["warm_detected"].as_bool().unwrap_or(false);
            let halted = result["halted_at"].as_str().map(String::from);
            let summary = if compute {
                "sovereign pipeline succeeded — compute ready".to_string()
            } else if let Some(h) = &halted {
                format!("sovereign pipeline halted at: {h}")
            } else if all_ok {
                "all stages ok but compute not confirmed".to_string()
            } else if warm {
                "warm GPU detected but pipeline did not complete (falcon boot pending)".to_string()
            } else {
                "sovereign pipeline did not complete".to_string()
            };
            (Some(result), all_ok || warm, summary)
        }
        Err(e) => {
            (None, false, format!("sovereign init RPC failed: {e}"))
        }
    };

    steps.push(BootStep {
        name: "sovereign_init".into(),
        status: if init_ok { StepStatus::Ok } else { StepStatus::Failed },
        detail: Some(init_summary.clone()),
        duration_ms: step_start.elapsed().as_millis() as u64,
    });

    let final_driver = sysfs::read_current_driver(bdf);
    let total_ms = start.elapsed().as_millis();

    BootResult {
        bdf: bdf.to_string(),
        initial_driver,
        warm_cycle_performed,
        final_driver,
        sovereign_init,
        capture_recipe_path,
        steps,
        success: init_ok,
        summary: format!("{init_summary} (total: {total_ms}ms)"),
    }
}
