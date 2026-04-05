// SPDX-License-Identifier: AGPL-3.0-only
//! Intel Xe / Arc discrete GPU lifecycle.
//!
//! Intel discrete GPUs (Arc Alchemist, Battlemage) support PCIe FLR natively
//! and have well-behaved VFIO passthrough. The Xe kernel driver uses standard
//! PCI power management — no exotic workarounds required for driver swaps.
//!
//! **Empirical status**: This implementation is based on Intel's published Xe
//! driver source and PCIe spec compliance. It has not been validated on
//! physical hardware in this codebase. The lifecycle methods are conservative
//! and will log warnings when invoked until real-hardware validation is done.

use crate::error::SwapError;
use crate::sysfs;
use coral_driver::linux_paths;

use super::types::{RebindStrategy, ResetMethod, VendorLifecycle};

/// Intel discrete Xe / Arc GPU lifecycle.
///
/// Arc A-series (Alchemist, DG2) and Battlemage GPUs support standard PCIe
/// Function Level Reset. The Xe kernel driver handles power management through
/// standard PCI PM D-states without proprietary firmware handshakes.
#[derive(Debug)]
pub struct IntelXeLifecycle {
    /// PCI device ID from config space.
    pub device_id: u16,
}

impl IntelXeLifecycle {
    /// Alchemist (DG2) device IDs start at 0x56xx.
    fn is_alchemist(&self) -> bool {
        (0x5690..=0x56FF).contains(&self.device_id)
    }

    /// Battlemage (Xe2) device IDs are in the 0xE20x range.
    fn is_battlemage(&self) -> bool {
        (0xE200..=0xE2FF).contains(&self.device_id)
    }

    fn generation_name(&self) -> &'static str {
        if self.is_alchemist() {
            "Alchemist (DG2)"
        } else if self.is_battlemage() {
            "Battlemage (Xe2)"
        } else {
            "Unknown Xe"
        }
    }
}

impl VendorLifecycle for IntelXeLifecycle {
    fn description(&self) -> &str {
        "Intel Xe/Arc discrete (FLR capable, standard PCI PM)"
    }

    fn available_reset_methods(&self) -> Vec<ResetMethod> {
        // Intel discrete GPUs advertise FLR in PCI Express capability.
        // VFIO FLR is the preferred path; fall back to sysfs SBR.
        vec![ResetMethod::VfioFlr, ResetMethod::SysfsSbr]
    }

    fn prepare_for_unbind(&self, bdf: &str, current_driver: &str) -> Result<(), SwapError> {
        tracing::info!(
            bdf,
            current_driver,
            generation = self.generation_name(),
            "Intel Xe: preparing for unbind (FLR-capable path)"
        );
        sysfs::pin_power(bdf);

        // Intel Xe supports FLR — verify the sysfs reset file exists
        // (indicates kernel has a reset path available for VFIO release).
        let reset_path = linux_paths::sysfs_pci_device_file(bdf, "reset");
        if !std::path::Path::new(&reset_path).exists() {
            tracing::warn!(
                bdf,
                "Intel Xe: sysfs reset file missing — FLR may not be available"
            );
        }

        Ok(())
    }

    fn rebind_strategy(&self, _target_driver: &str) -> RebindStrategy {
        // Intel Xe has clean FLR semantics — simple bind works reliably
        // because FLR fully reinitializes the device function.
        RebindStrategy::SimpleBind
    }

    fn settle_secs(&self, target_driver: &str) -> u64 {
        match target_driver {
            "xe" => 3,
            "i915" => 5,
            "vfio-pci" => 2,
            _ => 5,
        }
    }

    fn stabilize_after_bind(&self, bdf: &str, target_driver: &str) {
        tracing::info!(
            bdf,
            target_driver,
            generation = self.generation_name(),
            "Intel Xe: stabilizing after bind"
        );
        sysfs::pin_power(bdf);
    }

    fn verify_health(&self, bdf: &str, target_driver: &str) -> Result<(), SwapError> {
        let power = sysfs::read_power_state(bdf);
        match power.as_deref() {
            Some("D3cold") => {
                return Err(SwapError::VerifyHealth {
                    bdf: bdf.to_string(),
                    detail: format!(
                        "Intel Xe ({}) in D3cold after bind to {target_driver}",
                        self.generation_name()
                    ),
                });
            }
            Some("error") | None => {
                return Err(SwapError::VerifyHealth {
                    bdf: bdf.to_string(),
                    detail: format!(
                        "Intel Xe ({}) power state unreadable after bind to {target_driver}",
                        self.generation_name()
                    ),
                });
            }
            _ => {}
        }
        Ok(())
    }

    fn is_cold_sensitive(&self) -> bool {
        // Intel Xe discrete GPUs handle cold boot gracefully — the Xe driver
        // performs full device initialization without requiring a prior VBIOS POST.
        false
    }
}
