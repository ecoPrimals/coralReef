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

mod nouveau;
mod nvidia;
mod nvidia_open;
mod vfio;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use std::fmt;

use coral_ember::observation::SwapObservation;

pub use nouveau::NouveauObserver;
pub use nvidia::NvidiaObserver;
pub use nvidia_open::NvidiaOpenObserver;
pub use vfio::VfioObserver;

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
