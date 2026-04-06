// SPDX-License-Identifier: AGPL-3.0-or-later
//! Core types for vendor lifecycle: reset methods, rebind strategy, and the [`VendorLifecycle`] trait.

use std::fmt;

use crate::error::SwapError;

/// Available PCI reset methods for a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMethod {
    /// VFIO_DEVICE_RESET ioctl — requires an open VFIO fd and FLR-capable hardware.
    VfioFlr,
    /// Sysfs `reset` file on the device itself. Works on hardware with kernel-negotiated
    /// reset, but fails on VFIO-bound devices that lack FLR (e.g. GV100).
    SysfsSbr,
    /// Reset via the parent PCI bridge's `reset` file. Triggers a true Secondary Bus
    /// Reset that affects all devices behind the bridge. Works even when the device
    /// is VFIO-bound and lacks FLR — this is the primary reset path for GV100 Titan V.
    BridgeSbr,
    /// Full PCI remove + bus rescan cycle. Most aggressive: tears down the kernel's
    /// device tree and forces re-enumeration. VFIO fds become invalid after this.
    RemoveRescan,
}

/// How to transition a device from unbound to a new driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebindStrategy {
    /// Standard sysfs driver_override + drivers/{target}/bind.
    SimpleBind,

    /// Try simple bind first; if it fails (e.g. sysfs EEXIST from stale
    /// kobjects), fall back to PCI remove + bus rescan. This avoids the
    /// risk of bridge power-down that can make cards invisible after remove.
    SimpleWithRescanFallback,

    /// Go straight to PCI remove + bus rescan, skipping simple bind entirely.
    /// WARNING: does NOT work for AMD Vega 20 — bridge powers off slot on remove.
    PciRescan,

    /// PM power cycle (D3hot→D0) then simple bind. The PM cycle reinitializes
    /// the function without a bus reset, giving the native driver a clean
    /// starting state. Required for AMD Vega 20 where bus reset causes D3cold
    /// and PCI remove loses the device entirely.
    PmResetAndBind,
}

/// Vendor-specific lifecycle hooks invoked by the swap orchestrator.
///
/// Implementors encode hardware-specific knowledge about safe driver
/// transitions. The trait is intentionally coarse-grained — each method
/// maps to a phase of the swap sequence rather than individual sysfs writes.
pub trait VendorLifecycle: Send + Sync + fmt::Debug {
    /// Human-readable chip family description.
    fn description(&self) -> &str;

    /// Called before any driver unbind. Use for disabling dangerous reset
    /// methods, pinning power rails, or quiescing vendor-specific firmware.
    ///
    /// `current_driver` is the driver currently bound (e.g. "vfio-pci", "amdgpu").
    fn prepare_for_unbind(&self, bdf: &str, current_driver: &str) -> Result<(), SwapError>;

    /// How to rebind a native driver after the device is in unbound state.
    /// `target_driver` is the intended destination (e.g. "amdgpu", "nouveau").
    fn rebind_strategy(&self, target_driver: &str) -> RebindStrategy;

    /// Seconds to wait for driver initialization after bind succeeds.
    fn settle_secs(&self, target_driver: &str) -> u64;

    /// Called immediately after a driver binds and settles. Use for re-pinning
    /// power rails that the newly-bound driver may have reconfigured, clearing
    /// reset methods it restored, and disabling runtime PM it enabled.
    ///
    /// This is distinct from `prepare_for_unbind` — that runs BEFORE a swap,
    /// while this runs AFTER the destination driver is live.
    fn stabilize_after_bind(&self, bdf: &str, target_driver: &str);

    /// Post-bind health check. Called after the target driver appears in sysfs.
    /// Should verify the device is actually functional (temp sensors, VRAM, etc.)
    fn verify_health(&self, bdf: &str, target_driver: &str) -> Result<(), SwapError>;

    /// Which reset methods are safe/available for this hardware, in priority order.
    /// The caller should try methods in order and stop at the first success.
    ///
    /// Default: try VFIO FLR first, then sysfs SBR.
    fn available_reset_methods(&self) -> Vec<ResetMethod> {
        vec![ResetMethod::VfioFlr, ResetMethod::SysfsSbr]
    }
}
