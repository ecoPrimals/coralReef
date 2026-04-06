// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fallback lifecycle for unknown PCI vendors.

use crate::error::SwapError;
use crate::sysfs;
use coral_driver::linux_paths;

use super::types::{RebindStrategy, VendorLifecycle};

// ---------------------------------------------------------------------------
// Generic / unknown vendor
// ---------------------------------------------------------------------------

/// Fallback lifecycle for unknown PCI vendor IDs.
#[derive(Debug)]
pub struct GenericLifecycle {
    /// PCI vendor ID from config space.
    pub vendor_id: u16,
    /// PCI device ID from config space.
    #[expect(dead_code, reason = "reserved for future vendor-specific refinement")]
    pub device_id: u16,
}

impl VendorLifecycle for GenericLifecycle {
    fn description(&self) -> &str {
        "Unknown vendor (conservative defaults)"
    }

    fn prepare_for_unbind(&self, bdf: &str, current_driver: &str) -> Result<(), SwapError> {
        sysfs::pin_power(bdf);

        if current_driver == "vfio-pci" {
            tracing::warn!(
                bdf,
                vendor_id = format!("0x{:04x}", self.vendor_id),
                "unknown vendor: disabling reset_method as precaution"
            );
            let _ = sysfs::sysfs_write_direct(
                &linux_paths::sysfs_pci_device_file(bdf, "reset_method"),
                "",
            );
        }

        Ok(())
    }

    fn rebind_strategy(&self, target_driver: &str) -> RebindStrategy {
        match target_driver {
            "vfio" | "vfio-pci" => RebindStrategy::SimpleBind,
            _ => RebindStrategy::SimpleWithRescanFallback,
        }
    }

    fn settle_secs(&self, _target_driver: &str) -> u64 {
        10
    }

    fn stabilize_after_bind(&self, bdf: &str, _target_driver: &str) {
        sysfs::pin_power(bdf);
    }

    fn verify_health(&self, bdf: &str, _target_driver: &str) -> Result<(), SwapError> {
        let power = sysfs::read_power_state(bdf);
        if power.as_deref() == Some("D3cold") {
            return Err(SwapError::VerifyHealth {
                bdf: bdf.to_string(),
                detail: "device in D3cold after bind".to_string(),
            });
        }
        Ok(())
    }
}
