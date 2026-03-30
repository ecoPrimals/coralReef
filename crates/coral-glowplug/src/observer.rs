// SPDX-License-Identifier: AGPL-3.0-only
//! Driver observer — personality-specific diagnostic extraction.
//!
//! Each GPU driver personality produces different diagnostic artifacts during
//! bind/unbind. The [`DriverObserver`] trait provides a uniform interface for
//! extracting structured insights from those artifacts.
//!
//! - **Nouveau**: parses mmiotrace files to extract PRIV ring reset counts,
//!   PRAMIN window configurations, PMC_ENABLE sequences, and falcon patterns.
//! - **VFIO**: captures BAR0 register snapshots (FalconProbe, Layer7Diagnostics).
//! - **NVIDIA/NVIDIA-Open**: stubs that capture bind timing; GSP-specific
//!   analysis can be added as we decode GSP firmware sequences.
//!
//! Observer results are fed to the experiment journal for cross-personality
//! comparison.

use serde::{Deserialize, Serialize};
use std::fmt;

use coral_ember::observation::SwapObservation;

/// Trait for personality-specific diagnostic extraction.
///
/// Observers are stateless — they extract insights from observations and
/// trace artifacts without maintaining internal state across calls.
pub trait DriverObserver: fmt::Display + fmt::Debug + Send + Sync {
    /// Which personality this observer handles.
    fn personality_name(&self) -> &'static str;

    /// Analyze a swap observation and return any personality-specific insights.
    fn observe_swap(&self, observation: &SwapObservation) -> Option<DriverInsight>;

    /// Analyze a trace file (mmiotrace, GSP log, etc.) and return insights.
    fn observe_trace(&self, trace_path: &str) -> Option<DriverInsight>;
}

/// Structured insight extracted by a [`DriverObserver`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverInsight {
    /// Personality that produced this insight.
    pub personality: String,
    /// Individual findings from the analysis.
    pub findings: Vec<Finding>,
}

/// A single observation from driver behavior analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    /// What category of hardware behavior this finding relates to.
    pub category: FindingCategory,
    /// Human-readable description.
    pub description: String,
    /// BAR0 register offset, if applicable.
    pub register_offset: Option<u32>,
    /// Number of occurrences observed.
    pub count: Option<u32>,
}

/// Categories of hardware behavior that observers can identify.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FindingCategory {
    /// PRIV ring reset writes (0x070000 = 0x1).
    PrivRingReset,
    /// Falcon engine boot sequence detected.
    FalconBoot,
    /// PRAMIN window configuration for VRAM access.
    PraminWrite,
    /// DMA controller setup.
    DmaSetup,
    /// Power state transition (D0/D3hot/D3cold).
    PowerStateChange,
    /// Firmware loading via DMA.
    FirmwareLoad,
    /// PMC_ENABLE engine power-on.
    PmcEnable,
    /// GSP falcon activity.
    GspActivity,
    /// Uncategorized finding.
    Other(String),
}

// ---------------------------------------------------------------------------
// Nouveau Observer
// ---------------------------------------------------------------------------

/// Extracts insights from nouveau's mmiotrace data.
///
/// Nouveau boots falcons entirely through DMA (firmware uploaded via
/// PRAMIN/VRAM), relying heavily on PRIV ring resets and PRAMIN window
/// setup. This observer parses the mmiotrace to quantify those patterns.
#[derive(Debug, Clone)]
pub struct NouveauObserver;

impl fmt::Display for NouveauObserver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NouveauObserver")
    }
}

impl DriverObserver for NouveauObserver {
    fn personality_name(&self) -> &'static str {
        "nouveau"
    }

    fn observe_swap(&self, observation: &SwapObservation) -> Option<DriverInsight> {
        if observation.to_personality != "nouveau" {
            return None;
        }

        let mut findings = Vec::new();
        findings.push(Finding {
            category: FindingCategory::Other("swap_timing".to_string()),
            description: format!(
                "nouveau bind completed in {}ms (prepare={}ms, unbind={}ms, bind={}ms)",
                observation.timing.total_ms,
                observation.timing.prepare_ms,
                observation.timing.unbind_ms,
                observation.timing.bind_ms,
            ),
            register_offset: None,
            count: None,
        });

        Some(DriverInsight {
            personality: "nouveau".to_string(),
            findings,
        })
    }

    fn observe_trace(&self, trace_path: &str) -> Option<DriverInsight> {
        let content = std::fs::read_to_string(trace_path).ok()?;
        let mut findings = Vec::new();

        let mut priv_ring_resets = 0u32;
        let mut pramin_writes = 0u32;
        let mut pmc_enable_writes = 0u32;
        let mut gsp_writes = 0u32;
        let mut total_writes = 0u32;

        for line in content.lines() {
            if !line.starts_with('W') {
                continue;
            }
            total_writes += 1;

            // Parse mmiotrace W lines: W 4 <len> <addr> <value> ...
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 5 {
                continue;
            }
            let addr = match u64::from_str_radix(parts[3].trim_start_matches("0x"), 16) {
                Ok(a) => a,
                Err(_) => continue,
            };

            // Classify by BAR0 offset (bottom 24 bits)
            let offset = addr & 0xFF_FFFF;

            match offset {
                0x070000 => priv_ring_resets += 1,
                0x000204 => pmc_enable_writes += 1,
                0x100cb8..=0x100cfc => pramin_writes += 1,
                0x700000..=0x7FFFFF => pramin_writes += 1,
                0x800000..=0x800FFF => gsp_writes += 1,
                _ => {}
            }
        }

        findings.push(Finding {
            category: FindingCategory::PrivRingReset,
            description: format!("{priv_ring_resets} PRIV ring resets observed"),
            register_offset: Some(0x070000),
            count: Some(priv_ring_resets),
        });

        findings.push(Finding {
            category: FindingCategory::PraminWrite,
            description: format!("{pramin_writes} PRAMIN/VRAM window writes"),
            register_offset: None,
            count: Some(pramin_writes),
        });

        findings.push(Finding {
            category: FindingCategory::PmcEnable,
            description: format!("{pmc_enable_writes} PMC_ENABLE writes (engine power-on)"),
            register_offset: Some(0x000204),
            count: Some(pmc_enable_writes),
        });

        if gsp_writes > 0 {
            findings.push(Finding {
                category: FindingCategory::GspActivity,
                description: format!("{gsp_writes} GSP falcon register writes"),
                register_offset: Some(0x800000),
                count: Some(gsp_writes),
            });
        }

        findings.push(Finding {
            category: FindingCategory::Other("total_mmio_writes".to_string()),
            description: format!("{total_writes} total MMIO write lines"),
            register_offset: None,
            count: Some(total_writes),
        });

        Some(DriverInsight {
            personality: "nouveau".to_string(),
            findings,
        })
    }
}

// ---------------------------------------------------------------------------
// VFIO Observer
// ---------------------------------------------------------------------------

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
        let vram_ok = matches!(
            observation.health,
            coral_ember::observation::HealthResult::Ok
        );
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
                let halted = fecs_cpuctl & 0x20 != 0;
                let hreset = fecs_cpuctl & 0x10 != 0;
                let running = !halted && !hreset && fecs_cpuctl != 0xDEAD_DEAD;
                findings.push(Finding {
                    category: FindingCategory::FalconBoot,
                    description: format!(
                        "FECS cpuctl={fecs_cpuctl:#010x} running={running} halted={halted} hreset={hreset}"
                    ),
                    register_offset: Some(0x409100),
                    count: Some(if running { 1 } else { 0 }),
                });
            }

            if let Some(gpccs_cpuctl) = diag.get("gpccs_cpuctl").and_then(|v| v.as_u64()) {
                let halted = gpccs_cpuctl & 0x20 != 0;
                let hreset = gpccs_cpuctl & 0x10 != 0;
                let running = !halted && !hreset && gpccs_cpuctl != 0xDEAD_DEAD;
                findings.push(Finding {
                    category: FindingCategory::FalconBoot,
                    description: format!(
                        "GPCCS cpuctl={gpccs_cpuctl:#010x} running={running} halted={halted} hreset={hreset}"
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

// ---------------------------------------------------------------------------
// NVIDIA Observer (closed-source)
// ---------------------------------------------------------------------------

/// Observer for the proprietary NVIDIA driver.
///
/// The closed-source driver's MMIO patterns are opaque, but mmiotrace
/// captures allow us to extract timing, PRIV ring resets, PMC enable
/// sequences, and falcon boot patterns — the same register addresses
/// as nouveau, just driven by a different code path.
#[derive(Debug, Clone)]
pub struct NvidiaObserver;

const NVIDIA_BIND_SLOW_THRESHOLD_MS: u64 = 5000;

impl fmt::Display for NvidiaObserver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NvidiaObserver")
    }
}

impl DriverObserver for NvidiaObserver {
    fn personality_name(&self) -> &'static str {
        "nvidia"
    }

    fn observe_swap(&self, observation: &SwapObservation) -> Option<DriverInsight> {
        if observation.to_personality != "nvidia" {
            return None;
        }

        let mut findings = vec![Finding {
            category: FindingCategory::Other("nvidia_bind".to_string()),
            description: format!(
                "NVIDIA proprietary bind completed in {}ms",
                observation.timing.total_ms,
            ),
            register_offset: None,
            count: None,
        }];

        if observation.timing.total_ms > NVIDIA_BIND_SLOW_THRESHOLD_MS {
            findings.push(Finding {
                category: FindingCategory::PowerStateChange,
                description: format!(
                    "slow bind ({}ms > {NVIDIA_BIND_SLOW_THRESHOLD_MS}ms) — \
                     GPU may be recovering from D3cold or performing full DEVINIT",
                    observation.timing.total_ms,
                ),
                register_offset: None,
                count: None,
            });
        }

        Some(DriverInsight {
            personality: "nvidia".to_string(),
            findings,
        })
    }

    fn observe_trace(&self, trace_path: &str) -> Option<DriverInsight> {
        let content = std::fs::read_to_string(trace_path).ok()?;
        let mut findings = Vec::new();
        let mut total_writes = 0u32;
        let mut priv_resets = 0u32;
        let mut pmc_enables = 0u32;
        let mut falcon_boots = 0u32;

        for line in content.lines() {
            if !line.starts_with('W') {
                continue;
            }
            total_writes += 1;

            if let Some(addr) = extract_mmio_addr(line) {
                if addr == 0x0007_0000 {
                    priv_resets += 1;
                } else if addr == 0x0000_0200 {
                    pmc_enables += 1;
                } else if (0x0010_a000..0x0010_b000).contains(&addr)
                    || (0x0008_4000..0x0008_5000).contains(&addr)
                {
                    falcon_boots += 1;
                }
            }
        }

        findings.push(Finding {
            category: FindingCategory::Other("total_mmio_writes".to_string()),
            description: format!("{total_writes} total MMIO write lines from nvidia driver"),
            register_offset: None,
            count: Some(total_writes),
        });

        if priv_resets > 0 {
            findings.push(Finding {
                category: FindingCategory::PrivRingReset,
                description: format!("{priv_resets} PRIV ring reset writes (0x070000)"),
                register_offset: Some(0x0007_0000),
                count: Some(priv_resets),
            });
        }
        if pmc_enables > 0 {
            findings.push(Finding {
                category: FindingCategory::PmcEnable,
                description: format!("{pmc_enables} PMC_ENABLE writes"),
                register_offset: Some(0x0000_0200),
                count: Some(pmc_enables),
            });
        }
        if falcon_boots > 0 {
            findings.push(Finding {
                category: FindingCategory::FalconBoot,
                description: format!(
                    "{falcon_boots} falcon engine register writes (FECS/SEC2 range)"
                ),
                register_offset: None,
                count: Some(falcon_boots),
            });
        }

        Some(DriverInsight {
            personality: "nvidia".to_string(),
            findings,
        })
    }
}

/// Extract the MMIO address from an mmiotrace write line.
///
/// Format: `W <width> <timestamp> <address> <value> ...`
fn extract_mmio_addr(line: &str) -> Option<u64> {
    let mut parts = line.split_whitespace();
    parts.next()?; // W
    parts.next()?; // width
    parts.next()?; // timestamp
    let addr_str = parts.next()?;
    u64::from_str_radix(addr_str.trim_start_matches("0x"), 16).ok()
}

// ---------------------------------------------------------------------------
// NVIDIA Open Observer
// ---------------------------------------------------------------------------

/// Observer for the open-source NVIDIA kernel module (GSP-based).
///
/// The open module uses GSP firmware for falcon management, producing
/// different MMIO patterns compared to the closed-source driver.
#[derive(Debug, Clone)]
pub struct NvidiaOpenObserver;

impl fmt::Display for NvidiaOpenObserver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NvidiaOpenObserver")
    }
}

impl DriverObserver for NvidiaOpenObserver {
    fn personality_name(&self) -> &'static str {
        "nvidia-open"
    }

    fn observe_swap(&self, observation: &SwapObservation) -> Option<DriverInsight> {
        if observation.to_personality != "nvidia-open" {
            return None;
        }

        Some(DriverInsight {
            personality: "nvidia-open".to_string(),
            findings: vec![Finding {
                category: FindingCategory::Other("nvidia_open_bind".to_string()),
                description: format!(
                    "NVIDIA Open (GSP) bind completed in {}ms",
                    observation.timing.total_ms,
                ),
                register_offset: None,
                count: None,
            }],
        })
    }

    fn observe_trace(&self, trace_path: &str) -> Option<DriverInsight> {
        let content = std::fs::read_to_string(trace_path).ok()?;
        let mut findings = Vec::new();
        let mut gsp_writes = 0u32;
        let mut total_writes = 0u32;

        for line in content.lines() {
            if !line.starts_with('W') {
                continue;
            }
            total_writes += 1;

            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 5 {
                continue;
            }
            if let Ok(addr) = u64::from_str_radix(parts[3].trim_start_matches("0x"), 16) {
                let offset = addr & 0xFF_FFFF;
                if (0x800000..=0x80FFFF).contains(&offset) {
                    gsp_writes += 1;
                }
            }
        }

        if gsp_writes > 0 {
            findings.push(Finding {
                category: FindingCategory::GspActivity,
                description: format!("{gsp_writes} GSP falcon region writes"),
                register_offset: Some(0x800000),
                count: Some(gsp_writes),
            });
        }

        findings.push(Finding {
            category: FindingCategory::Other("total_mmio_writes".to_string()),
            description: format!("{total_writes} total MMIO writes from nvidia-open driver"),
            register_offset: None,
            count: Some(total_writes),
        });

        Some(DriverInsight {
            personality: "nvidia-open".to_string(),
            findings,
        })
    }
}

// ---------------------------------------------------------------------------
// Observer Registry
// ---------------------------------------------------------------------------

/// Registry of all available driver observers.
pub struct ObserverRegistry {
    observers: Vec<Box<dyn DriverObserver>>,
}

impl ObserverRegistry {
    /// Build the default registry with all known observers.
    #[must_use]
    pub fn default_observers() -> Self {
        Self {
            observers: vec![
                Box::new(NouveauObserver),
                Box::new(VfioObserver),
                Box::new(NvidiaObserver),
                Box::new(NvidiaOpenObserver),
            ],
        }
    }

    /// Find the observer matching a personality name.
    pub fn for_personality(&self, name: &str) -> Option<&dyn DriverObserver> {
        self.observers
            .iter()
            .find(|o| o.personality_name() == name)
            .map(|o| o.as_ref())
    }

    /// Run all matching observers on a swap observation, returning all insights.
    pub fn observe_swap(&self, observation: &SwapObservation) -> Vec<DriverInsight> {
        self.observers
            .iter()
            .filter_map(|o| o.observe_swap(observation))
            .collect()
    }

    /// Run the matching observer on a trace file.
    pub fn observe_trace(&self, personality: &str, trace_path: &str) -> Option<DriverInsight> {
        self.for_personality(personality)
            .and_then(|o| o.observe_trace(trace_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coral_ember::observation::{HealthResult, SwapTiming};

    fn make_obs(personality: &str) -> SwapObservation {
        SwapObservation {
            bdf: "0000:03:00.0".to_string(),
            from_personality: Some("vfio".to_string()),
            to_personality: personality.to_string(),
            timestamp_epoch_ms: 1700000000000,
            timing: SwapTiming {
                prepare_ms: 50,
                unbind_ms: 200,
                bind_ms: 5000,
                stabilize_ms: 100,
                total_ms: 5350,
            },
            trace_path: None,
            health: HealthResult::Ok,
            lifecycle_description: "test".to_string(),
            reset_method_used: None,
        }
    }

    #[test]
    fn nouveau_observer_produces_swap_insight() {
        let obs = NouveauObserver;
        let insight = obs.observe_swap(&make_obs("nouveau")).unwrap();
        assert_eq!(insight.personality, "nouveau");
        assert!(!insight.findings.is_empty());
    }

    #[test]
    fn nouveau_observer_skips_wrong_personality() {
        let obs = NouveauObserver;
        assert!(obs.observe_swap(&make_obs("vfio")).is_none());
    }

    #[test]
    fn vfio_observer_produces_swap_insight() {
        let obs = VfioObserver;
        let insight = obs.observe_swap(&make_obs("vfio")).unwrap();
        assert_eq!(insight.personality, "vfio");
        assert!(insight.findings.len() >= 3);
        assert!(
            insight
                .findings
                .iter()
                .any(|f| f.description.contains("VFIO bind"))
        );
        assert!(
            insight
                .findings
                .iter()
                .any(|f| f.description.contains("handoff type"))
        );
        assert!(
            insight
                .findings
                .iter()
                .any(|f| f.description.contains("VRAM accessible"))
        );
    }

    #[test]
    fn vfio_observer_parses_diagnostic_dump() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dump_path = dir.path().join("vfio_diag.json");
        let dump = serde_json::json!({
            "fecs_cpuctl": 0x00000002_u64,
            "gpccs_cpuctl": 0x00000010_u64,
            "pmc_enable": 0x5fecdff1_u64,
            "vram_alive": true,
            "rings": [
                {"name": "gpfifo", "pending": 0, "fence": 42},
                {"name": "ce0", "pending": 3, "fence": 15}
            ]
        });
        std::fs::write(&dump_path, serde_json::to_string(&dump).unwrap()).unwrap();

        let obs = VfioObserver;
        let insight = obs.observe_trace(dump_path.to_str().unwrap()).unwrap();
        assert_eq!(insight.personality, "vfio");
        assert!(
            insight
                .findings
                .iter()
                .any(|f| f.category == FindingCategory::FalconBoot)
        );
        assert!(
            insight
                .findings
                .iter()
                .any(|f| f.category == FindingCategory::PmcEnable)
        );
        assert!(insight.findings.iter().any(|f| matches!(
            &f.category,
            FindingCategory::Other(s) if s == "ring_health"
        )));
    }

    #[test]
    fn registry_finds_observer_by_personality() {
        let reg = ObserverRegistry::default_observers();
        assert!(reg.for_personality("nouveau").is_some());
        assert!(reg.for_personality("vfio").is_some());
        assert!(reg.for_personality("nvidia").is_some());
        assert!(reg.for_personality("nvidia-open").is_some());
        assert!(reg.for_personality("unknown").is_none());
    }

    #[test]
    fn registry_observe_swap_returns_matching_insights() {
        let reg = ObserverRegistry::default_observers();
        let insights = reg.observe_swap(&make_obs("nouveau"));
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].personality, "nouveau");
    }

    #[test]
    fn nouveau_observer_parses_mmiotrace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let trace_path = dir.path().join("test.mmiotrace");
        // Minimal mmiotrace with a PRIV ring reset and a PMC_ENABLE write
        std::fs::write(
            &trace_path,
            "W 4 4 0xf2070000 0x00000001 1 0 0\n\
             W 4 4 0xf2070000 0x00000001 1 0 0\n\
             W 4 4 0xf2000204 0xffffffff 1 0 0\n\
             W 4 4 0xf2100cb8 0x00020000 1 0 0\n",
        )
        .expect("write test trace");

        let obs = NouveauObserver;
        let insight = obs
            .observe_trace(trace_path.to_str().unwrap())
            .expect("parse trace");
        assert_eq!(insight.personality, "nouveau");

        let priv_resets = insight
            .findings
            .iter()
            .find(|f| f.category == FindingCategory::PrivRingReset)
            .expect("priv ring finding");
        assert_eq!(priv_resets.count, Some(2));

        let pmc = insight
            .findings
            .iter()
            .find(|f| f.category == FindingCategory::PmcEnable)
            .expect("pmc finding");
        assert_eq!(pmc.count, Some(1));
    }

    #[test]
    fn nvidia_observer_personality_name() {
        assert_eq!(NvidiaObserver.personality_name(), "nvidia");
        assert_eq!(NvidiaOpenObserver.personality_name(), "nvidia-open");
    }

    #[test]
    fn nvidia_observer_produces_swap_insight() {
        let obs = NvidiaObserver;
        let insight = obs.observe_swap(&make_obs("nvidia")).unwrap();
        assert_eq!(insight.personality, "nvidia");
        assert!(
            insight
                .findings
                .iter()
                .any(|f| f.description.contains("bind completed"))
        );
    }

    #[test]
    fn nvidia_observer_slow_bind_produces_power_state_finding() {
        let mut obs_data = make_obs("nvidia");
        obs_data.timing.total_ms = 10_000;
        let insight = NvidiaObserver.observe_swap(&obs_data).unwrap();
        assert!(
            insight
                .findings
                .iter()
                .any(|f| f.category == FindingCategory::PowerStateChange),
            "slow bind should produce PowerStateChange finding"
        );
    }

    #[test]
    fn nvidia_observer_fast_bind_no_power_state_finding() {
        let mut obs_data = make_obs("nvidia");
        obs_data.timing.total_ms = 500;
        let insight = NvidiaObserver.observe_swap(&obs_data).unwrap();
        assert!(
            !insight
                .findings
                .iter()
                .any(|f| f.category == FindingCategory::PowerStateChange),
            "fast bind should not produce PowerStateChange finding"
        );
    }

    #[test]
    fn nvidia_observer_skips_wrong_personality() {
        assert!(NvidiaObserver.observe_swap(&make_obs("nouveau")).is_none());
    }

    #[test]
    fn nvidia_observer_parses_mmiotrace_with_patterns() {
        let dir = tempfile::tempdir().expect("tempdir");
        let trace_path = dir.path().join("nvidia.mmiotrace");
        std::fs::write(
            &trace_path,
            "W 4 1234 0x00070000 0x00000001 1 0 0\n\
             W 4 1235 0x00070000 0x00000001 1 0 0\n\
             W 4 1236 0x00000200 0xffffffff 1 0 0\n\
             W 4 1237 0x0010a100 0x00000002 1 0 0\n\
             W 4 1238 0x00084004 0x00000040 1 0 0\n\
             R 4 1239 0x00084004 0x00000040 1 0 0\n",
        )
        .expect("write test trace");

        let insight = NvidiaObserver
            .observe_trace(trace_path.to_str().unwrap())
            .expect("parse trace");
        assert_eq!(insight.personality, "nvidia");

        let total = insight
            .findings
            .iter()
            .find(|f| f.description.contains("total MMIO"))
            .unwrap();
        assert_eq!(total.count, Some(5));

        assert!(
            insight
                .findings
                .iter()
                .any(|f| f.category == FindingCategory::PrivRingReset),
            "should detect PRIV ring resets"
        );
        assert!(
            insight
                .findings
                .iter()
                .any(|f| f.category == FindingCategory::FalconBoot),
            "should detect falcon boot writes"
        );
    }

    #[test]
    fn extract_mmio_addr_parses_hex() {
        assert_eq!(
            extract_mmio_addr("W 4 1234 0x00070000 0x00000001 1 0 0"),
            Some(0x0007_0000)
        );
        assert_eq!(
            extract_mmio_addr("R 4 1234 0x00070000 0x00000001 1 0 0"),
            Some(0x0007_0000)
        );
        assert_eq!(extract_mmio_addr("invalid"), None);
    }
}
