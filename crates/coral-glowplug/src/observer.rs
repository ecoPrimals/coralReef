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

/// Extracts insights from VFIO personality (BAR0 register state).
///
/// After a swap to VFIO, this observer can analyze register snapshots
/// captured by coral-driver's FalconProbe and Layer7Diagnostics.
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

        Some(DriverInsight {
            personality: "vfio".to_string(),
            findings: vec![Finding {
                category: FindingCategory::Other("vfio_bind".to_string()),
                description: format!(
                    "VFIO bind completed in {}ms — BAR0 now accessible for sovereign operations",
                    observation.timing.total_ms,
                ),
                register_offset: None,
                count: None,
            }],
        })
    }

    fn observe_trace(&self, _trace_path: &str) -> Option<DriverInsight> {
        None
    }
}

// ---------------------------------------------------------------------------
// NVIDIA Observer (closed-source)
// ---------------------------------------------------------------------------

/// Stub observer for the proprietary NVIDIA driver.
///
/// The closed-source driver's MMIO patterns are opaque, but we can still
/// capture timing data and health observations.
#[derive(Debug, Clone)]
pub struct NvidiaObserver;

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

        Some(DriverInsight {
            personality: "nvidia".to_string(),
            findings: vec![Finding {
                category: FindingCategory::Other("nvidia_bind".to_string()),
                description: format!(
                    "NVIDIA proprietary bind completed in {}ms",
                    observation.timing.total_ms,
                ),
                register_offset: None,
                count: None,
            }],
        })
    }

    fn observe_trace(&self, trace_path: &str) -> Option<DriverInsight> {
        let content = std::fs::read_to_string(trace_path).ok()?;
        let total_writes = content.lines().filter(|l| l.starts_with('W')).count();

        Some(DriverInsight {
            personality: "nvidia".to_string(),
            findings: vec![Finding {
                category: FindingCategory::Other("total_mmio_writes".to_string()),
                description: format!("{total_writes} total MMIO write lines from nvidia driver"),
                register_offset: None,
                count: Some(total_writes as u32),
            }],
        })
    }
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
}
