// SPDX-License-Identifier: AGPL-3.0-only
//! Structured observations from driver swaps and resets.
//!
//! Every `ember.swap` and `ember.device_reset` produces an observation
//! that captures timing, health results, and trace paths. These
//! observations flow back through IPC to GlowPlug and are persisted
//! in the experiment journal for cross-personality comparison.

use serde::{Deserialize, Serialize};

/// Complete observation from a driver swap operation.
///
/// Returned by `ember.swap` and appended to the experiment journal.
/// Contains everything needed to compare driver behavior across
/// personalities: timing breakdown, trace artifacts, and health status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapObservation {
    /// PCI BDF address of the device.
    pub bdf: String,
    /// Driver/personality before the swap (if known).
    pub from_personality: Option<String>,
    /// Driver/personality after the swap.
    pub to_personality: String,
    /// Unix epoch milliseconds when the swap started.
    pub timestamp_epoch_ms: u64,
    /// Per-phase timing breakdown.
    pub timing: SwapTiming,
    /// Path to mmiotrace capture file, if tracing was enabled.
    pub trace_path: Option<String>,
    /// Post-bind health check result.
    pub health: HealthResult,
    /// Human-readable description of the vendor lifecycle used.
    pub lifecycle_description: String,
    /// Reset method used during the swap, if any.
    pub reset_method_used: Option<String>,
}

/// Per-phase timing breakdown of a swap operation (all values in milliseconds).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapTiming {
    /// Time spent in vendor-specific preparation (power pinning, reset_method disable).
    pub prepare_ms: u64,
    /// Time spent unbinding the current driver.
    pub unbind_ms: u64,
    /// Time spent binding the target driver (includes settle wait).
    pub bind_ms: u64,
    /// Time spent in post-bind stabilization and health checks.
    pub stabilize_ms: u64,
    /// Wall-clock time from start to finish.
    pub total_ms: u64,
}

/// Result of post-bind health verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum HealthResult {
    /// Device passed all health checks.
    Ok,
    /// Device is functional but with a non-fatal concern.
    Warning {
        /// Description of the warning condition.
        message: String,
    },
    /// Device failed health checks.
    Failed {
        /// Description of the failure.
        message: String,
    },
}

/// Observation from a device reset operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResetObservation {
    /// PCI BDF address of the device.
    pub bdf: String,
    /// Reset method that was attempted.
    pub method: String,
    /// Whether the reset succeeded.
    pub success: bool,
    /// Error message if the reset failed.
    pub error: Option<String>,
    /// Unix epoch milliseconds when the reset was attempted.
    pub timestamp_epoch_ms: u64,
    /// How long the reset took in milliseconds.
    pub duration_ms: u64,
}

/// Timestamp helper: current time as Unix epoch milliseconds.
pub fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_observation_roundtrip_json() {
        let obs = SwapObservation {
            bdf: "0000:03:00.0".into(),
            from_personality: Some("vfio".into()),
            to_personality: "nouveau".into(),
            timestamp_epoch_ms: 1700000000000,
            timing: SwapTiming {
                prepare_ms: 50,
                unbind_ms: 200,
                bind_ms: 3000,
                stabilize_ms: 500,
                total_ms: 3750,
            },
            trace_path: Some("/var/lib/coralreef/traces/test.mmiotrace".into()),
            health: HealthResult::Ok,
            lifecycle_description: "NVIDIA (bus reset kills HBM2)".into(),
            reset_method_used: None,
        };
        let json = serde_json::to_string(&obs).expect("serialize");
        let back: SwapObservation = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.bdf, "0000:03:00.0");
        assert_eq!(back.to_personality, "nouveau");
        assert_eq!(back.timing.bind_ms, 3000);
        assert!(back.trace_path.is_some());
    }

    #[test]
    fn health_result_variants_serialize_with_tag() {
        let ok = serde_json::to_string(&HealthResult::Ok).unwrap();
        assert!(ok.contains("\"status\":\"Ok\""));

        let warn = serde_json::to_string(&HealthResult::Warning {
            message: "slow settle".into(),
        })
        .unwrap();
        assert!(warn.contains("\"status\":\"Warning\""));
        assert!(warn.contains("slow settle"));

        let fail = serde_json::to_string(&HealthResult::Failed {
            message: "D3cold".into(),
        })
        .unwrap();
        assert!(fail.contains("\"status\":\"Failed\""));
    }

    #[test]
    fn reset_observation_roundtrip() {
        let obs = ResetObservation {
            bdf: "0000:4a:00.0".into(),
            method: "bridge-sbr".into(),
            success: true,
            error: None,
            timestamp_epoch_ms: 1700000000000,
            duration_ms: 750,
        };
        let json = serde_json::to_string(&obs).expect("serialize");
        let back: ResetObservation = serde_json::from_str(&json).expect("deserialize");
        assert!(back.success);
        assert_eq!(back.method, "bridge-sbr");
    }

    #[test]
    fn epoch_ms_returns_nonzero() {
        assert!(epoch_ms() > 0);
    }

    #[test]
    fn swap_observation_backward_compat_has_bdf_and_personality() {
        let obs = SwapObservation {
            bdf: "0000:03:00.0".into(),
            from_personality: None,
            to_personality: "vfio".into(),
            timestamp_epoch_ms: 0,
            timing: SwapTiming {
                prepare_ms: 0,
                unbind_ms: 0,
                bind_ms: 0,
                stabilize_ms: 0,
                total_ms: 0,
            },
            trace_path: None,
            health: HealthResult::Ok,
            lifecycle_description: "test".into(),
            reset_method_used: None,
        };
        let json = serde_json::to_value(&obs).expect("to_value");
        assert_eq!(json["bdf"], "0000:03:00.0");
        assert_eq!(json["to_personality"], "vfio");
    }

    #[test]
    fn swap_timing_phase_sum_relationship() {
        let timing = SwapTiming {
            prepare_ms: 12,
            unbind_ms: 34,
            bind_ms: 5600,
            stabilize_ms: 78,
            total_ms: 6000,
        };
        let phase_sum = timing.prepare_ms + timing.unbind_ms + timing.bind_ms + timing.stabilize_ms;
        assert!(
            phase_sum <= timing.total_ms,
            "recorded phases should fit within wall-clock total (phase_sum={phase_sum}, total={})",
            timing.total_ms
        );
    }
}
