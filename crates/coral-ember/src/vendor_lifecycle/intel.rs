// SPDX-License-Identifier: AGPL-3.0-only
//! Intel Xe / Arc discrete GPU lifecycle (`xe` and `i915` DRM drivers).
//!
//! On Linux, Intel discrete GPUs bind to the `xe` driver (Xe2 / newer
//! platforms and upstream Arc) or `i915` (older discrete IDs). Integrated
//! graphics also use these drivers; this lifecycle applies whenever PCI
//! detection routes Intel vendor `0x8086` here.
//!
//! # Reset and rebind
//!
//! Unlike NVIDIA (HBM2 training) or AMD Vega 20 (D3cold via bus reset),
//! Intel GPUs do **not** need sysfs `reset_method` cleared in
//! `prepare_for_unbind` or `stabilize_after_bind`. The kernel’s negotiated
//! reset path (typically FLR-capable) does not leave the device in a
//! vendor-specific broken memory state that would require ember to strip
//! reset methods before VFIO transitions.
//!
//! Rebind uses [`RebindStrategy::SimpleBind`] for every target, including
//! `vfio-pci` — no [`RebindStrategy::PmResetAndBind`], no forced PCI rescan.
//!
//! # Timing
//!
//! DRM driver bring-up for Intel hardware is typically on the order of one
//! to two seconds; `settle_secs` uses a conservative upper bound of two
//! seconds for all targets.
//!
//! # Health
//!
//! After binding `xe` or `i915`, `verify_health` requires a `cardN` node under
//! the PCI device’s `drm/` sysfs directory (see [`crate::sysfs::find_drm_card`]),
//! which corresponds to `/dev/dri/card*`. VFIO and other non-DRM targets skip
//! that check because they do not expose a DRM minor.

use crate::sysfs;

use super::types::{RebindStrategy, VendorError, VendorLifecycle};

/// Returns true when `target_driver` is a native Intel DRM driver that owns `/dev/dri/card*`.
#[inline]
fn is_intel_drm_target_driver(target_driver: &str) -> bool {
    matches!(target_driver, "xe" | "i915")
}

// ---------------------------------------------------------------------------
// Intel lifecycle (Xe / Arc discrete GPUs)
// ---------------------------------------------------------------------------

/// Intel discrete Xe / Arc — `xe`/`i915` profile (simple bind, DRM-aware health).
#[derive(Debug)]
pub struct IntelXeLifecycle {
    /// PCI device ID from config space.
    #[expect(dead_code, reason = "reserved for Arc vs Battlemage differences")]
    pub device_id: u16,
}

impl VendorLifecycle for IntelXeLifecycle {
    fn description(&self) -> &str {
        "Intel Xe/Arc (xe/i915 — simple bind, ~2s settle, DRM card sysfs check)"
    }

    fn prepare_for_unbind(&self, bdf: &str, _current_driver: &str) -> Result<(), VendorError> {
        sysfs::pin_power(bdf);
        Ok(())
    }

    fn rebind_strategy(&self, _target_driver: &str) -> RebindStrategy {
        RebindStrategy::SimpleBind
    }

    fn settle_secs(&self, _target_driver: &str) -> u64 {
        2
    }

    fn stabilize_after_bind(&self, bdf: &str, _target_driver: &str) {
        sysfs::pin_power(bdf);
    }

    fn verify_health(&self, bdf: &str, target_driver: &str) -> Result<(), VendorError> {
        let power = sysfs::read_power_state(bdf);
        if power.as_deref() == Some("D3cold") {
            return Err(VendorError::HealthCheck {
                bdf: bdf.to_string(),
                detail: "Intel GPU in D3cold after bind".to_string(),
            });
        }

        if is_intel_drm_target_driver(target_driver) && sysfs::find_drm_card(bdf).is_none() {
            return Err(VendorError::DrmCardNotFound {
                bdf: bdf.to_string(),
                driver: target_driver.to_string(),
            });
        }

        Ok(())
    }
}
