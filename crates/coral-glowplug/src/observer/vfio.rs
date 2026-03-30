// SPDX-License-Identifier: AGPL-3.0-only

use std::fmt;

use coral_ember::observation::{HealthResult, SwapObservation};

use super::{DriverInsight, DriverObserver, Finding, FindingCategory};

/// Extracts insights from VFIO personality — falcon state, VRAM, PMC, ring health.
///
/// After a swap to VFIO, this observer analyzes:
/// - Falcon states (FECS/GPCCS cpuctl, PC, EXCI) — are they warm from nouveau?
/// - VRAM accessibility via health result
/// - PMC_ENABLE engine state
/// - Ring/mailbox health from the swap lifecycle
/// - Diagnostic dump files (JSON falcon probe data)
#[derive(Debug, Clone)]
pub struct VfioObserver;

impl fmt::Display for VfioObserver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VfioObserver")
    }
}

impl DriverObserver for VfioObserver {
    fn personality_name(&self) -> &'static str {
        "vfio"
    }

    fn observe_swap(&self, observation: &SwapObservation) -> Option<DriverInsight> {
        if observation.to_personality != "vfio" {
            return None;
        }

        let mut findings = Vec::new();

        findings.push(Finding {
            category: FindingCategory::Other("vfio_bind".to_string()),
            description: format!(
                "VFIO bind: {}ms total (prepare={}ms, unbind={}ms, bind={}ms, stabilize={}ms)",
                observation.timing.total_ms,
                observation.timing.prepare_ms,
                observation.timing.unbind_ms,
                observation.timing.bind_ms,
                observation.timing.stabilize_ms,
            ),
            register_offset: None,
            count: None,
        });

        // Detect warm vs cold handoff from timing
        let warm_threshold_ms = 5000;
        let handoff_type = if let Some(ref from) = observation.from_personality {
            if from == "nouveau" && observation.timing.bind_ms < warm_threshold_ms {
                "warm-fecs"
            } else if from == "nouveau" {
                "cold-nouveau"
            } else if from == "nvidia-open" || from == "nvidia" {
                "post-proprietary"
            } else {
                "cold"
            }
        } else {
            "cold"
        };
        findings.push(Finding {
            category: FindingCategory::FalconBoot,
            description: format!("VFIO handoff type: {handoff_type}"),
            register_offset: None,
            count: None,
        });

        // VRAM health assessment
        let vram_ok = matches!(observation.health, HealthResult::Ok);
        findings.push(Finding {
            category: FindingCategory::Other("vram_probe".to_string()),
            description: format!(
                "VRAM accessible: {} (health: {:?})",
                vram_ok, observation.health
            ),
            register_offset: None,
            count: Some(if vram_ok { 1 } else { 0 }),
        });

        // Reset method analysis
        if let Some(ref method) = observation.reset_method_used {
            findings.push(Finding {
                category: FindingCategory::PowerStateChange,
                description: format!("reset method: {method}"),
                register_offset: None,
                count: None,
            });
        }

        Some(DriverInsight {
            personality: "vfio".to_string(),
            findings,
        })
    }

    fn observe_trace(&self, trace_path: &str) -> Option<DriverInsight> {
        let content = std::fs::read_to_string(trace_path).ok()?;
        let mut findings = Vec::new();

        // Try parsing as JSON diagnostic dump (falcon probe, GR status)
        if let Ok(diag) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(fecs_cpuctl) = diag.get("fecs_cpuctl").and_then(|v| v.as_u64()) {
                let stopped = fecs_cpuctl & 0x20 != 0;
                let halted = fecs_cpuctl & 0x10 != 0;
                let running = !stopped && !halted && fecs_cpuctl != 0xDEAD_DEAD;
                findings.push(Finding {
                    category: FindingCategory::FalconBoot,
                    description: format!(
                        "FECS cpuctl={fecs_cpuctl:#010x} running={running} stopped={stopped} halted={halted}"
                    ),
                    register_offset: Some(0x409100),
                    count: Some(if running { 1 } else { 0 }),
                });
            }

            if let Some(gpccs_cpuctl) = diag.get("gpccs_cpuctl").and_then(|v| v.as_u64()) {
                let stopped = gpccs_cpuctl & 0x20 != 0;
                let halted = gpccs_cpuctl & 0x10 != 0;
                let running = !stopped && !halted && gpccs_cpuctl != 0xDEAD_DEAD;
                findings.push(Finding {
                    category: FindingCategory::FalconBoot,
                    description: format!(
                        "GPCCS cpuctl={gpccs_cpuctl:#010x} running={running} stopped={stopped} halted={halted}"
                    ),
                    register_offset: Some(0x41a100),
                    count: Some(if running { 1 } else { 0 }),
                });
            }

            if let Some(pmc_enable) = diag.get("pmc_enable").and_then(|v| v.as_u64()) {
                let gr_enabled = pmc_enable & (1 << 12) != 0;
                let sec2_enabled = pmc_enable & (1 << 22) != 0;
                let ce_enabled = pmc_enable & (1 << 9) != 0;
                findings.push(Finding {
                    category: FindingCategory::PmcEnable,
                    description: format!(
                        "PMC_ENABLE={pmc_enable:#010x} GR={gr_enabled} SEC2={sec2_enabled} CE={ce_enabled}"
                    ),
                    register_offset: Some(0x200),
                    count: Some(
                        gr_enabled as u32 + sec2_enabled as u32 + ce_enabled as u32,
                    ),
                });
            }

            if let Some(vram) = diag.get("vram_alive").and_then(|v| v.as_bool()) {
                findings.push(Finding {
                    category: FindingCategory::Other("vram_probe".to_string()),
                    description: format!("VRAM accessible: {vram}"),
                    register_offset: None,
                    count: Some(if vram { 1 } else { 0 }),
                });
            }

            if let Some(rings) = diag.get("rings").and_then(|v| v.as_array()) {
                for ring in rings {
                    let name = ring.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let pending = ring.get("pending").and_then(|v| v.as_u64()).unwrap_or(0);
                    let fence = ring.get("fence").and_then(|v| v.as_u64()).unwrap_or(0);
                    findings.push(Finding {
                        category: FindingCategory::Other("ring_health".to_string()),
                        description: format!("ring '{name}': pending={pending} fence={fence}"),
                        register_offset: None,
                        count: Some(pending as u32),
                    });
                }
            }
        }

        if findings.is_empty() {
            return None;
        }

        Some(DriverInsight {
            personality: "vfio".to_string(),
            findings,
        })
    }
}
