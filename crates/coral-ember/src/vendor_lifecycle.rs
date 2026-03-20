// SPDX-License-Identifier: AGPL-3.0-only
//! Vendor-specific GPU lifecycle hooks for safe driver transitions.
//!
//! Different GPU vendors (and even chip families within a vendor) have
//! wildly different behaviors when VFIO-PCI unbinds, bus resets fire,
//! or native drivers rebind. This module encodes those differences as
//! a trait so the core swap logic in [`super::swap`] stays generic.
//!
//! The key insight from empirical testing:
//!
//! - **NVIDIA GV100 (Volta)**: Bus reset is safe. HBM2 state persists.
//!   Simple unbind/bind round-trips work.
//!
//! - **AMD Vega 20 (GFX906)**: Bus reset triggers D3cold, killing SMU
//!   firmware state. The reset_method must be disabled before vfio-pci
//!   unbind. Native driver rebind needs PCI remove/rescan to avoid
//!   sysfs EEXIST from stale kobjects.
//!
//! - **Intel Xe/Arc**: FLR typically available, expected to be well-behaved.
//!   Stubbed with conservative defaults until empirically validated.

use crate::sysfs;
use std::fmt;

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
    #[expect(
        dead_code,
        reason = "valid rebind strategy for future vendor lifecycle profiles (e.g. Intel Xe FLR)"
    )]
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
    fn prepare_for_unbind(&self, bdf: &str, current_driver: &str) -> Result<(), String>;

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
    fn verify_health(&self, bdf: &str, target_driver: &str) -> Result<(), String>;
}

// ---------------------------------------------------------------------------
// NVIDIA lifecycle (Volta, Turing, Ampere, Ada)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct NvidiaLifecycle {
    #[allow(
        dead_code,
        reason = "reserved for per-chip lifecycle refinement (Volta vs Turing vs Ada)"
    )]
    pub device_id: u16,
}

impl VendorLifecycle for NvidiaLifecycle {
    fn description(&self) -> &str {
        "NVIDIA (bus reset safe, HBM2 state preserved)"
    }

    fn prepare_for_unbind(&self, bdf: &str, _current_driver: &str) -> Result<(), String> {
        sysfs::pin_power(bdf);
        Ok(())
    }

    fn rebind_strategy(&self, _target_driver: &str) -> RebindStrategy {
        RebindStrategy::SimpleBind
    }

    fn settle_secs(&self, target_driver: &str) -> u64 {
        match target_driver {
            "nouveau" => 10,
            _ => 5,
        }
    }

    fn stabilize_after_bind(&self, bdf: &str, _target_driver: &str) {
        sysfs::pin_power(bdf);
    }

    fn verify_health(&self, bdf: &str, _target_driver: &str) -> Result<(), String> {
        let power = sysfs::read_power_state(bdf);
        if power.as_deref() == Some("D3cold") {
            return Err(format!("{bdf}: device in D3cold after bind"));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// AMD lifecycle (Vega 20 / GFX906 — MI50, MI60, Radeon VII)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct AmdVega20Lifecycle {
    #[allow(
        dead_code,
        reason = "reserved for MI50 vs MI60 vs Radeon VII differences"
    )]
    pub device_id: u16,
}

impl VendorLifecycle for AmdVega20Lifecycle {
    fn description(&self) -> &str {
        "AMD Vega 20 (bus reset causes D3cold — reset_method must be disabled)"
    }

    fn prepare_for_unbind(&self, bdf: &str, _current_driver: &str) -> Result<(), String> {
        sysfs::pin_power(bdf);
        sysfs::pin_bridge_power(bdf);

        tracing::info!(
            bdf,
            "AMD Vega 20: disabling reset_method (prevents D3cold on any transition)"
        );
        sysfs::sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/reset_method"), "")?;

        Ok(())
    }

    fn rebind_strategy(&self, target_driver: &str) -> RebindStrategy {
        match target_driver {
            "vfio" | "vfio-pci" => RebindStrategy::SimpleBind,
            _ => RebindStrategy::PmResetAndBind,
        }
    }

    fn settle_secs(&self, target_driver: &str) -> u64 {
        match target_driver {
            "vfio" | "vfio-pci" => 3,
            _ => 15,
        }
    }

    fn stabilize_after_bind(&self, bdf: &str, target_driver: &str) {
        sysfs::pin_power(bdf);
        sysfs::pin_bridge_power(bdf);

        let _ = sysfs::sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/reset_method"), "");

        if target_driver == "amdgpu" {
            let _ = sysfs::sysfs_write(
                &format!("/sys/bus/pci/devices/{bdf}/power/autosuspend_delay_ms"),
                "-1",
            );
            let _ = sysfs::sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/power/control"), "on");
        }

        tracing::info!(
            bdf,
            target_driver,
            "AMD Vega 20: post-bind stabilized (power pinned, reset_method cleared)"
        );
    }

    fn verify_health(&self, bdf: &str, target_driver: &str) -> Result<(), String> {
        let power = sysfs::read_power_state(bdf);
        if power.as_deref() == Some("D3cold") {
            return Err(format!(
                "{bdf}: AMD Vega 20 in D3cold — SMU firmware lost, reboot required"
            ));
        }

        if target_driver == "amdgpu" {
            for attempt in 0..5 {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let temp_path = format!("/sys/bus/pci/devices/{bdf}/hwmon");
                if let Ok(entries) = std::fs::read_dir(&temp_path)
                    && entries
                        .flatten()
                        .any(|e| e.file_name().to_string_lossy().starts_with("hwmon"))
                {
                    return Ok(());
                }
                tracing::debug!(bdf, attempt, "waiting for hwmon to appear");
            }
            tracing::warn!(
                bdf,
                "amdgpu hwmon not found after 5 attempts — SMU may be slow"
            );
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// AMD RDNA lifecycle (RX 5000/6000/7000 series)
// Conservative defaults — needs empirical validation.
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct AmdRdnaLifecycle {
    #[allow(dead_code, reason = "reserved for RDNA1/2/3 differences")]
    pub device_id: u16,
}

impl VendorLifecycle for AmdRdnaLifecycle {
    fn description(&self) -> &str {
        "AMD RDNA (conservative — needs empirical validation)"
    }

    fn prepare_for_unbind(&self, bdf: &str, _current_driver: &str) -> Result<(), String> {
        sysfs::pin_power(bdf);
        sysfs::pin_bridge_power(bdf);

        tracing::info!(bdf, "AMD RDNA: disabling reset_method (conservative)");
        sysfs::sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/reset_method"), "")?;

        Ok(())
    }

    fn rebind_strategy(&self, target_driver: &str) -> RebindStrategy {
        match target_driver {
            "vfio" | "vfio-pci" => RebindStrategy::SimpleBind,
            _ => RebindStrategy::PmResetAndBind,
        }
    }

    fn settle_secs(&self, _target_driver: &str) -> u64 {
        12
    }

    fn stabilize_after_bind(&self, bdf: &str, target_driver: &str) {
        sysfs::pin_power(bdf);
        sysfs::pin_bridge_power(bdf);

        let _ = sysfs::sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/reset_method"), "");

        if target_driver == "amdgpu" {
            let _ = sysfs::sysfs_write(
                &format!("/sys/bus/pci/devices/{bdf}/power/autosuspend_delay_ms"),
                "-1",
            );
            let _ = sysfs::sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/power/control"), "on");
        }
    }

    fn verify_health(&self, bdf: &str, _target_driver: &str) -> Result<(), String> {
        let power = sysfs::read_power_state(bdf);
        if power.as_deref() == Some("D3cold") {
            return Err(format!("{bdf}: AMD RDNA in D3cold after bind"));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Intel lifecycle (Xe / Arc discrete GPUs)
// Stubbed with conservative FLR-aware defaults.
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct IntelXeLifecycle {
    #[allow(dead_code, reason = "reserved for Arc vs Battlemage differences")]
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

// ---------------------------------------------------------------------------
// BrainChip lifecycle (AKD1000 Akida neuromorphic NPU)
// Simple PCIe accelerator — no GPU, no DRM, no SMU complexity.
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct BrainChipLifecycle {
    #[allow(dead_code, reason = "reserved for AKD1000 vs future Akida variants")]
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

// ---------------------------------------------------------------------------
// Generic / unknown vendor
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct GenericLifecycle {
    pub vendor_id: u16,
    #[allow(dead_code, reason = "reserved for future vendor-specific refinement")]
    pub device_id: u16,
}

impl VendorLifecycle for GenericLifecycle {
    fn description(&self) -> &str {
        "Unknown vendor (conservative defaults)"
    }

    fn prepare_for_unbind(&self, bdf: &str, current_driver: &str) -> Result<(), String> {
        sysfs::pin_power(bdf);

        if current_driver == "vfio-pci" {
            tracing::warn!(
                bdf,
                vendor_id = format!("0x{:04x}", self.vendor_id),
                "unknown vendor: disabling reset_method as precaution"
            );
            let _ = sysfs::sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/reset_method"), "");
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

    fn verify_health(&self, bdf: &str, _target_driver: &str) -> Result<(), String> {
        let power = sysfs::read_power_state(bdf);
        if power.as_deref() == Some("D3cold") {
            return Err(format!("{bdf}: device in D3cold after bind"));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Factory: auto-detect lifecycle from PCI vendor/device IDs
// ---------------------------------------------------------------------------

const NVIDIA_VENDOR: u16 = 0x10de;
const AMD_VENDOR: u16 = 0x1002;
const INTEL_VENDOR: u16 = 0x8086;
const BRAINCHIP_VENDOR: u16 = 0x1e7c;

const AMD_VEGA20_IDS: &[u16] = &[0x66a0, 0x66a1, 0x66af];

fn is_amd_vega20(device_id: u16) -> bool {
    AMD_VEGA20_IDS.contains(&device_id)
}

/// Auto-detect the appropriate VendorLifecycle for a PCI device.
pub fn detect_lifecycle(bdf: &str) -> Box<dyn VendorLifecycle> {
    let vendor_id = sysfs::read_pci_id(bdf, "vendor");
    let device_id = sysfs::read_pci_id(bdf, "device");

    tracing::info!(
        bdf,
        vendor = format!("0x{vendor_id:04x}"),
        device = format!("0x{device_id:04x}"),
        "detecting vendor lifecycle"
    );

    match vendor_id {
        NVIDIA_VENDOR => {
            tracing::info!(bdf, "lifecycle: NVIDIA");
            Box::new(NvidiaLifecycle { device_id })
        }
        AMD_VENDOR => {
            if is_amd_vega20(device_id) {
                tracing::info!(bdf, "lifecycle: AMD Vega 20 (D3cold-sensitive)");
                Box::new(AmdVega20Lifecycle { device_id })
            } else {
                tracing::info!(bdf, "lifecycle: AMD RDNA (conservative)");
                Box::new(AmdRdnaLifecycle { device_id })
            }
        }
        INTEL_VENDOR => {
            tracing::info!(bdf, "lifecycle: Intel Xe");
            Box::new(IntelXeLifecycle { device_id })
        }
        BRAINCHIP_VENDOR => {
            tracing::info!(bdf, "lifecycle: BrainChip Akida");
            Box::new(BrainChipLifecycle { device_id })
        }
        _ => {
            tracing::warn!(
                bdf,
                vendor = format!("0x{vendor_id:04x}"),
                "lifecycle: unknown vendor, using conservative defaults"
            );
            Box::new(GenericLifecycle {
                vendor_id,
                device_id,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vega20_ids_recognized() {
        assert!(is_amd_vega20(0x66a0)); // MI50
        assert!(is_amd_vega20(0x66a1)); // MI60
        assert!(is_amd_vega20(0x66af)); // Radeon VII
        assert!(!is_amd_vega20(0x7340)); // Navi 14
    }

    #[test]
    fn amd_vega20_uses_pm_reset_for_native() {
        let lc = AmdVega20Lifecycle { device_id: 0x66af };
        assert_eq!(lc.rebind_strategy("amdgpu"), RebindStrategy::PmResetAndBind);
        assert_eq!(lc.rebind_strategy("vfio-pci"), RebindStrategy::SimpleBind);
    }

    #[test]
    fn nvidia_uses_simple_bind() {
        let lc = NvidiaLifecycle { device_id: 0x1d81 };
        assert_eq!(lc.rebind_strategy("nouveau"), RebindStrategy::SimpleBind);
        assert_eq!(lc.rebind_strategy("nvidia"), RebindStrategy::SimpleBind);
        assert_eq!(lc.rebind_strategy("vfio-pci"), RebindStrategy::SimpleBind);
    }

    #[test]
    fn nvidia_nouveau_gets_longer_settle() {
        let lc = NvidiaLifecycle { device_id: 0x1d81 };
        assert_eq!(lc.settle_secs("nouveau"), 10);
        assert_eq!(lc.settle_secs("nvidia"), 5);
    }

    #[test]
    fn intel_xe_simple_bind() {
        let lc = IntelXeLifecycle { device_id: 0x56a0 };
        assert_eq!(lc.rebind_strategy("xe"), RebindStrategy::SimpleBind);
        assert_eq!(lc.rebind_strategy("i915"), RebindStrategy::SimpleBind);
    }

    #[test]
    fn generic_conservative_fallback_for_native() {
        let lc = GenericLifecycle {
            vendor_id: 0xdead,
            device_id: 0xbeef,
        };
        assert_eq!(
            lc.rebind_strategy("some-driver"),
            RebindStrategy::SimpleWithRescanFallback
        );
        assert_eq!(lc.rebind_strategy("vfio-pci"), RebindStrategy::SimpleBind);
    }
}
