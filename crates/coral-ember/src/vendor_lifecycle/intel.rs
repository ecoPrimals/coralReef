// SPDX-License-Identifier: AGPL-3.0-only
//! Intel Xe / Arc discrete GPU lifecycle (stubbed conservative defaults).

use crate::sysfs;

use super::types::{RebindStrategy, VendorLifecycle};

// ---------------------------------------------------------------------------
// Intel lifecycle (Xe / Arc discrete GPUs)
// Stubbed with conservative FLR-aware defaults.
// ---------------------------------------------------------------------------

/// Intel discrete Xe / Arc — FLR-oriented defaults (stubbed).
#[derive(Debug)]
pub struct IntelXeLifecycle {
    /// PCI device ID from config space.
    #[expect(dead_code, reason = "reserved for Arc vs Battlemage differences")]
    pub device_id: u16,
}

impl VendorLifecycle for IntelXeLifecycle {
    fn description(&self) -> &str {
        "Intel Xe/Arc (FLR expected, stubbed — needs empirical validation)"
    }

    fn prepare_for_unbind(&self, bdf: &str, _current_driver: &str) -> Result<(), String> {
        sysfs::pin_power(bdf);
        Ok(())
    }

    fn rebind_strategy(&self, _target_driver: &str) -> RebindStrategy {
        RebindStrategy::SimpleBind
    }

    fn settle_secs(&self, _target_driver: &str) -> u64 {
        5
    }

    fn stabilize_after_bind(&self, bdf: &str, _target_driver: &str) {
        sysfs::pin_power(bdf);
    }

    fn verify_health(&self, bdf: &str, _target_driver: &str) -> Result<(), String> {
        let power = sysfs::read_power_state(bdf);
        if power.as_deref() == Some("D3cold") {
            return Err(format!("{bdf}: Intel Xe in D3cold after bind"));
        }
        Ok(())
    }
}
