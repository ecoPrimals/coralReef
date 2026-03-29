// SPDX-License-Identifier: AGPL-3.0-only
//! BrainChip Akida NPU lifecycle.

use crate::sysfs;

use super::types::{RebindStrategy, VendorLifecycle};

// ---------------------------------------------------------------------------
// BrainChip lifecycle (AKD1000 Akida neuromorphic NPU)
// Simple PCIe accelerator — no GPU, no DRM, no SMU complexity.
// ---------------------------------------------------------------------------

/// BrainChip Akida NPU — simple PCIe accelerator profile.
#[derive(Debug)]
pub struct BrainChipLifecycle {
    /// PCI device ID from config space.
    #[expect(dead_code, reason = "reserved for AKD1000 vs future Akida variants")]
    pub device_id: u16,
}

impl VendorLifecycle for BrainChipLifecycle {
    fn description(&self) -> &str {
        "BrainChip Akida (simple PCIe accelerator, no GPU quirks)"
    }

    fn prepare_for_unbind(&self, bdf: &str, _current_driver: &str) -> Result<(), String> {
        sysfs::pin_power(bdf);
        Ok(())
    }

    fn rebind_strategy(&self, _target_driver: &str) -> RebindStrategy {
        RebindStrategy::SimpleBind
    }

    fn settle_secs(&self, _target_driver: &str) -> u64 {
        3
    }

    fn stabilize_after_bind(&self, bdf: &str, _target_driver: &str) {
        sysfs::pin_power(bdf);
    }

    fn verify_health(&self, bdf: &str, _target_driver: &str) -> Result<(), String> {
        let power = sysfs::read_power_state(bdf);
        if power.as_deref() == Some("D3cold") {
            return Err(format!("{bdf}: BrainChip Akida in D3cold after bind"));
        }
        Ok(())
    }
}
