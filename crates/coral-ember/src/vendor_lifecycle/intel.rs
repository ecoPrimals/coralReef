// SPDX-License-Identifier: AGPL-3.0-or-later
//! Intel Xe / Arc discrete GPU lifecycle — conservative FLR-aware defaults.
//!
//! Intel discrete GPUs (Arc Alchemist, Battlemage) support Function Level Reset
//! (FLR) which is well-behaved compared to NVIDIA's HBM2-destroying bus reset.
//! The defaults here are conservative pending empirical validation on real
//! hardware — FLR is expected but we don't yet have device-specific tuning.

use crate::error::SwapError;
use crate::sysfs;

use super::types::{RebindStrategy, VendorLifecycle};

/// Default settle time for Intel Xe devices (seconds).
///
/// Conservative: FLR is typically sub-second, but we allow headroom for driver
/// initialization on first bind. Override with `CORALREEF_INTEL_SETTLE_SECS`.
const DEFAULT_INTEL_SETTLE_SECS: u64 = 5;

/// Intel discrete Xe / Arc — FLR-oriented lifecycle.
///
/// Conservative defaults suitable for Arc Alchemist (A-series) and Battlemage
/// (B-series). FLR is the expected reset mechanism; bus reset is avoided.
/// Device-specific tuning (Arc vs Battlemage clock domains, EU counts) will
/// evolve as empirical data is collected from real hardware.
#[derive(Debug)]
pub struct IntelXeLifecycle {
    /// PCI device ID from config space — reserved for future Arc vs Battlemage
    /// differentiation (e.g. different settle times, reset quirks).
    #[expect(dead_code, reason = "reserved for Arc vs Battlemage differentiation")]
    pub device_id: u16,
    /// Settle time after bind, in seconds.
    settle_secs: u64,
}

impl IntelXeLifecycle {
    /// Create a lifecycle handler for an Intel Xe/Arc device.
    ///
    /// Reads `CORALREEF_INTEL_SETTLE_SECS` from environment if set;
    /// otherwise uses the conservative default.
    #[must_use]
    pub fn new(device_id: u16) -> Self {
        let settle_secs = std::env::var("CORALREEF_INTEL_SETTLE_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_INTEL_SETTLE_SECS);

        Self {
            device_id,
            settle_secs,
        }
    }
}

impl VendorLifecycle for IntelXeLifecycle {
    fn description(&self) -> &str {
        "Intel Xe/Arc (FLR-oriented, conservative defaults)"
    }

    fn prepare_for_unbind(&self, bdf: &str, _current_driver: &str) -> Result<(), SwapError> {
        sysfs::pin_power(bdf);
        Ok(())
    }

    fn rebind_strategy(&self, _target_driver: &str) -> RebindStrategy {
        RebindStrategy::SimpleBind
    }

    fn settle_secs(&self, _target_driver: &str) -> u64 {
        self.settle_secs
    }

    fn stabilize_after_bind(&self, bdf: &str, _target_driver: &str) {
        sysfs::pin_power(bdf);
    }

    fn verify_health(&self, bdf: &str, _target_driver: &str) -> Result<(), SwapError> {
        let power = sysfs::read_power_state(bdf);
        if power.as_deref() == Some("D3cold") {
            return Err(SwapError::VerifyHealth {
                bdf: bdf.to_string(),
                detail: "Intel Xe in D3cold after bind — FLR may have triggered \
                         unexpected power state transition"
                    .to_string(),
            });
        }
        Ok(())
    }
}
