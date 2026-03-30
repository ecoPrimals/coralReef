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
    /// Pre-swap firmware state (FECS/GPCCS/PMU falcon registers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware_pre: Option<FirmwareState>,
    /// Post-swap firmware state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware_post: Option<FirmwareState>,
}

/// Lightweight firmware state captured during swaps.
///
/// A subset of the full `FirmwareSnapshot` from coral-driver, capturing
/// the key registers needed to understand what a driver transition changed.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FirmwareState {
    /// FECS CPUCTL register.
    pub fecs_cpuctl: u32,
    /// FECS CPU stopped/idle (CPUCTL bit 5).
    pub fecs_stopped: bool,
    /// FECS firmware halted via HALT instruction (CPUCTL bit 4).
    #[serde(alias = "fecs_hreset")]
    pub fecs_halted: bool,
    /// FECS SCTL (security mode).
    pub fecs_sctl: u32,
    /// FECS mailbox0.
    pub fecs_mailbox0: u32,
    /// FECS method status.
    pub fecs_mthd_status: u32,
    /// GPCCS CPUCTL.
    pub gpccs_cpuctl: u32,
    /// PMU CPUCTL.
    pub pmu_cpuctl: u32,
    /// PMC_ENABLE (which engines are powered).
    pub pmc_enable: u32,
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
            firmware_pre: None,
            firmware_post: None,
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
    fn firmware_state_default_is_all_zeros() {
        let fs = FirmwareState::default();
        assert_eq!(fs.fecs_cpuctl, 0);
        assert!(!fs.fecs_stopped);
        assert!(!fs.fecs_halted);
        assert_eq!(fs.fecs_sctl, 0);
        assert_eq!(fs.pmc_enable, 0);
    }

    #[test]
    fn firmware_state_json_roundtrip() {
        let fs = FirmwareState {
            fecs_cpuctl: 0x0000_0030,
            fecs_stopped: false,
            fecs_halted: false,
            fecs_sctl: 0x0000_2000,
            fecs_mailbox0: 0x0000_0001,
            fecs_mthd_status: 0,
            gpccs_cpuctl: 0x0000_0030,
            pmu_cpuctl: 0x0000_0020,
            pmc_enable: 0x0000_1100,
        };
        let json = serde_json::to_string(&fs).expect("serialize");
        let back: FirmwareState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.fecs_cpuctl, 0x30);
        assert_eq!(back.fecs_sctl, 0x2000);
        assert_eq!(back.pmc_enable, 0x1100);
    }

    #[test]
    fn swap_observation_without_firmware_fields_deserializes() {
        let json = r#"{
            "bdf": "0000:03:00.0",
            "from_personality": null,
            "to_personality": "vfio",
            "timestamp_epoch_ms": 0,
            "timing": {"prepare_ms":0,"unbind_ms":0,"bind_ms":0,"stabilize_ms":0,"total_ms":0},
            "trace_path": null,
            "health": {"status":"Ok"},
            "lifecycle_description": "test",
            "reset_method_used": null
        }"#;
        let obs: SwapObservation = serde_json::from_str(json).expect("deserialize legacy JSON");
        assert!(obs.firmware_pre.is_none());
        assert!(obs.firmware_post.is_none());
        assert_eq!(obs.bdf, "0000:03:00.0");
    }

    #[test]
    fn swap_observation_with_firmware_roundtrips() {
        let obs = SwapObservation {
            bdf: "0000:03:00.0".into(),
            from_personality: Some("nouveau".into()),
            to_personality: "vfio".into(),
            timestamp_epoch_ms: 1700000000000,
            timing: SwapTiming {
                prepare_ms: 10,
                unbind_ms: 100,
                bind_ms: 3000,
                stabilize_ms: 50,
                total_ms: 3160,
            },
            trace_path: None,
            health: HealthResult::Ok,
            lifecycle_description: "test".into(),
            reset_method_used: None,
            firmware_pre: Some(FirmwareState {
                fecs_cpuctl: 0x30,
                fecs_stopped: false,
                fecs_halted: false,
                fecs_sctl: 0x2000,
                fecs_mailbox0: 0,
                fecs_mthd_status: 0,
                gpccs_cpuctl: 0x30,
                pmu_cpuctl: 0x20,
                pmc_enable: 0x1100,
            }),
            firmware_post: Some(FirmwareState {
                fecs_cpuctl: 0x10,
                fecs_stopped: false,
                fecs_halted: true,
                fecs_sctl: 0x2000,
                fecs_mailbox0: 0,
                fecs_mthd_status: 0,
                gpccs_cpuctl: 0x10,
                pmu_cpuctl: 0x10,
                pmc_enable: 0x1100,
            }),
        };
        let json = serde_json::to_string(&obs).expect("serialize");
        let back: SwapObservation = serde_json::from_str(&json).expect("deserialize");
        assert!(back.firmware_pre.is_some());
        assert!(back.firmware_post.is_some());
        let pre = back.firmware_pre.unwrap();
        assert_eq!(pre.fecs_cpuctl, 0x30);
        let post = back.firmware_post.unwrap();
        assert!(post.fecs_halted);
    }

    #[test]
    fn firmware_none_fields_omitted_in_json() {
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
            firmware_pre: None,
            firmware_post: None,
        };
        let json = serde_json::to_string(&obs).expect("serialize");
        assert!(
            !json.contains("firmware_pre"),
            "None firmware fields should be omitted"
        );
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
            firmware_pre: None,
            firmware_post: None,
        };
        let json = serde_json::to_value(&obs).expect("to_value");
        assert_eq!(json["bdf"], "0000:03:00.0");
        assert_eq!(json["to_personality"], "vfio");
    }
}
