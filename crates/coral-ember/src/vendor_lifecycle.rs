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
//! - **NVIDIA GV100 (Volta)**: Bus reset **destroys HBM2 training state**
//!   (VRAM reads return `0xbad0acXX`, memory fabric dead, PBDMA cannot DMA).
//!   The `reset_method` must be cleared before vfio-pci unbind/bind to
//!   preserve HBM2 across driver transitions.
//!
//! - **AMD Vega 20 (GFX906)**: Bus reset triggers D3cold, killing SMU
//!   firmware state. The reset_method must be disabled before vfio-pci
//!   unbind. Native driver rebind needs PCI remove/rescan to avoid
//!   sysfs EEXIST from stale kobjects.
//!
//! - **Intel Xe/Arc**: FLR typically available, expected to be well-behaved.
//!   Stubbed with conservative defaults until empirically validated.

use crate::sysfs;
use coral_driver::linux_paths;
use std::fmt;

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

    /// Which reset methods are safe/available for this hardware, in priority order.
    /// The caller should try methods in order and stop at the first success.
    ///
    /// Default: try VFIO FLR first, then sysfs SBR.
    fn available_reset_methods(&self) -> Vec<ResetMethod> {
        vec![ResetMethod::VfioFlr, ResetMethod::SysfsSbr]
    }
}

// ---------------------------------------------------------------------------
// NVIDIA lifecycle (Volta, Turing, Ampere, Ada)
// ---------------------------------------------------------------------------

/// NVIDIA GPUs — bus reset kills HBM2 training; `reset_method` must be disabled.
#[derive(Debug)]
pub struct NvidiaLifecycle {
    /// PCI device ID from config space.
    #[expect(
        dead_code,
        reason = "reserved for per-chip lifecycle refinement (Volta vs Turing vs Ada)"
    )]
    pub device_id: u16,
}

impl VendorLifecycle for NvidiaLifecycle {
    fn description(&self) -> &str {
        "NVIDIA (bus reset kills HBM2 — reset_method disabled)"
    }

    fn available_reset_methods(&self) -> Vec<ResetMethod> {
        // GV100 (Titan V / V100) does not support FLR — VFIO_DEVICE_RESET
        // returns EINVAL. Device-level SBR fails when bound to VFIO.
        // Bridge-level SBR is the reliable path — writes to the parent
        // bridge's reset file, which triggers a true bus reset.
        // Remove+rescan is the nuclear fallback.
        vec![
            ResetMethod::BridgeSbr,
            ResetMethod::SysfsSbr,
            ResetMethod::RemoveRescan,
        ]
    }

    fn prepare_for_unbind(&self, bdf: &str, _current_driver: &str) -> Result<(), String> {
        sysfs::pin_power(bdf);

        tracing::info!(
            bdf,
            "NVIDIA: disabling reset_method (bus reset destroys HBM2 training)"
        );
        sysfs::sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "")?;

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

        let _ =
            sysfs::sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "");
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
// NVIDIA Open lifecycle (open-source nvidia.ko with GSP firmware)
// ---------------------------------------------------------------------------

/// NVIDIA open kernel module — uses GSP firmware for falcon management.
/// Same reset behavior as closed-source nvidia, but different boot sequence
/// (GSP-based vs legacy PMU). Distinguished for the solution matrix.
#[derive(Debug)]
pub struct NvidiaOpenLifecycle {
    /// PCI device ID from config space.
    #[expect(dead_code, reason = "reserved for per-chip GSP support detection")]
    pub device_id: u16,
}

impl VendorLifecycle for NvidiaOpenLifecycle {
    fn description(&self) -> &str {
        "NVIDIA Open (GSP-based — bus reset kills HBM2)"
    }

    fn available_reset_methods(&self) -> Vec<ResetMethod> {
        vec![
            ResetMethod::BridgeSbr,
            ResetMethod::SysfsSbr,
            ResetMethod::RemoveRescan,
        ]
    }

    fn prepare_for_unbind(&self, bdf: &str, _current_driver: &str) -> Result<(), String> {
        sysfs::pin_power(bdf);
        tracing::info!(
            bdf,
            "NVIDIA Open: disabling reset_method (bus reset destroys HBM2 training)"
        );
        sysfs::sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "")?;
        Ok(())
    }

    fn rebind_strategy(&self, _target_driver: &str) -> RebindStrategy {
        RebindStrategy::SimpleBind
    }

    fn settle_secs(&self, target_driver: &str) -> u64 {
        match target_driver {
            "nouveau" => 10,
            _ => 8,
        }
    }

    fn stabilize_after_bind(&self, bdf: &str, _target_driver: &str) {
        sysfs::pin_power(bdf);
        let _ =
            sysfs::sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "");
    }

    fn verify_health(&self, bdf: &str, _target_driver: &str) -> Result<(), String> {
        let power = sysfs::read_power_state(bdf);
        if power.as_deref() == Some("D3cold") {
            return Err(format!("{bdf}: NVIDIA Open device in D3cold after bind"));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// NVIDIA Oracle lifecycle (renamed nvidia module for multi-version coexistence)
// ---------------------------------------------------------------------------

/// NVIDIA Oracle — renamed `nvidia_oracle.ko` module that coexists with the
/// system `nvidia.ko`. Built by patching `MODULE_BASE_NAME` and
/// `NV_MAJOR_DEVICE_NUMBER` in the open kernel source. GlowPlug binds Titans
/// to `nvidia_oracle` via `driver_override` while the display GPU stays on `nvidia`.
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "vendor lifecycle for oracle mode — used when nvidia_oracle target is selected"
)]
pub struct NvidiaOracleLifecycle {
    /// PCI device ID from config space.
    pub device_id: u16,
    /// The oracle module name (e.g. "nvidia_oracle", "nvidia_oracle_535").
    pub module_name: String,
}

impl VendorLifecycle for NvidiaOracleLifecycle {
    fn description(&self) -> &str {
        "NVIDIA Oracle (renamed module for driver coexistence)"
    }

    fn available_reset_methods(&self) -> Vec<ResetMethod> {
        vec![
            ResetMethod::BridgeSbr,
            ResetMethod::SysfsSbr,
            ResetMethod::RemoveRescan,
        ]
    }

    fn prepare_for_unbind(&self, bdf: &str, _current_driver: &str) -> Result<(), String> {
        sysfs::pin_power(bdf);
        sysfs::sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "")?;
        Ok(())
    }

    fn rebind_strategy(&self, _target_driver: &str) -> RebindStrategy {
        RebindStrategy::SimpleBind
    }

    fn settle_secs(&self, target_driver: &str) -> u64 {
        match target_driver {
            "nouveau" => 10,
            _ => 8,
        }
    }

    fn stabilize_after_bind(&self, bdf: &str, _target_driver: &str) {
        sysfs::pin_power(bdf);
        let _ =
            sysfs::sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "");
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

/// AMD Vega 20 (GFX906) — reset-sensitive; disables `reset_method` before unbind.
#[derive(Debug)]
pub struct AmdVega20Lifecycle {
    /// PCI device ID from config space.
    #[expect(
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
        sysfs::sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "")?;

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

        let _ =
            sysfs::sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "");

        if target_driver == "amdgpu" {
            let _ = sysfs::sysfs_write_direct(
                &linux_paths::sysfs_pci_device_file(bdf, "power/autosuspend_delay_ms"),
                "-1",
            );
            let _ = sysfs::sysfs_write_direct(
                &linux_paths::sysfs_pci_device_file(bdf, "power/control"),
                "on",
            );
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
                let temp_path = linux_paths::sysfs_pci_device_file(bdf, "hwmon");
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

/// AMD RDNA discrete GPUs — conservative reset and PM handling.
#[derive(Debug)]
pub struct AmdRdnaLifecycle {
    /// PCI device ID from config space.
    #[expect(dead_code, reason = "reserved for RDNA1/2/3 differences")]
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
        sysfs::sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "")?;

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

        let _ =
            sysfs::sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "");

        if target_driver == "amdgpu" {
            let _ = sysfs::sysfs_write_direct(
                &linux_paths::sysfs_pci_device_file(bdf, "power/autosuspend_delay_ms"),
                "-1",
            );
            let _ = sysfs::sysfs_write_direct(
                &linux_paths::sysfs_pci_device_file(bdf, "power/control"),
                "on",
            );
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

    fn prepare_for_unbind(&self, bdf: &str, current_driver: &str) -> Result<(), String> {
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

/// Build a [`VendorLifecycle`] from PCI config-space IDs (used by [`detect_lifecycle`] and unit tests).
pub(crate) fn lifecycle_from_pci_ids(vendor_id: u16, device_id: u16) -> Box<dyn VendorLifecycle> {
    match vendor_id {
        NVIDIA_VENDOR => Box::new(NvidiaLifecycle { device_id }),
        AMD_VENDOR => {
            if is_amd_vega20(device_id) {
                Box::new(AmdVega20Lifecycle { device_id })
            } else {
                Box::new(AmdRdnaLifecycle { device_id })
            }
        }
        INTEL_VENDOR => Box::new(IntelXeLifecycle { device_id }),
        BRAINCHIP_VENDOR => Box::new(BrainChipLifecycle { device_id }),
        _ => Box::new(GenericLifecycle {
            vendor_id,
            device_id,
        }),
    }
}

/// Build a lifecycle for a specific target driver override (e.g. `nvidia_oracle_535`).
/// Falls back to [`detect_lifecycle`] if the target doesn't need special handling.
pub fn detect_lifecycle_for_target(bdf: &str, target: &str) -> Box<dyn VendorLifecycle> {
    if target.starts_with("nvidia_oracle") {
        let device_id = sysfs::read_pci_id(bdf, "device");
        return Box::new(NvidiaOracleLifecycle {
            device_id,
            module_name: target.to_string(),
        });
    }
    if target == "nvidia-open" {
        let device_id = sysfs::read_pci_id(bdf, "device");
        return Box::new(NvidiaOpenLifecycle { device_id });
    }
    detect_lifecycle(bdf)
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

    let lc = lifecycle_from_pci_ids(vendor_id, device_id);
    match vendor_id {
        NVIDIA_VENDOR => tracing::info!(bdf, "lifecycle: NVIDIA"),
        AMD_VENDOR => {
            if is_amd_vega20(device_id) {
                tracing::info!(bdf, "lifecycle: AMD Vega 20 (D3cold-sensitive)");
            } else {
                tracing::info!(bdf, "lifecycle: AMD RDNA (conservative)");
            }
        }
        INTEL_VENDOR => tracing::info!(bdf, "lifecycle: Intel Xe"),
        BRAINCHIP_VENDOR => tracing::info!(bdf, "lifecycle: BrainChip Akida"),
        _ => tracing::warn!(
            bdf,
            vendor = format!("0x{vendor_id:04x}"),
            "lifecycle: unknown vendor, using conservative defaults"
        ),
    }
    lc
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

    #[test]
    fn nvidia_description() {
        let lc = NvidiaLifecycle { device_id: 0x1D81 };
        assert!(lc.description().contains("NVIDIA"));
        assert!(
            lc.description().contains("reset_method"),
            "description should mention reset_method is disabled"
        );
    }

    #[test]
    fn nvidia_prepare_for_unbind_clears_reset_method() {
        let lc = NvidiaLifecycle { device_id: 0x1d81 };
        let err = lc
            .prepare_for_unbind("not-a-bdf", "nouveau")
            .expect_err("should fail on fake BDF (sysfs path absent)");
        assert!(!err.is_empty());
    }

    #[test]
    fn nvidia_stabilize_after_bind_clears_reset_method_best_effort() {
        let lc = NvidiaLifecycle { device_id: 0x1d81 };
        lc.stabilize_after_bind("9999:99:99.9", "vfio-pci");
    }

    #[test]
    fn amd_vega20_description_and_settle() {
        let lc = AmdVega20Lifecycle { device_id: 0x66af };
        assert!(lc.description().contains("Vega 20"));
        assert_eq!(lc.settle_secs("vfio-pci"), 3);
        assert_eq!(lc.settle_secs("amdgpu"), 15);
        assert_eq!(lc.settle_secs("some-other"), 15);
    }

    #[test]
    fn amd_rdna_lifecycle_basics() {
        let lc = AmdRdnaLifecycle { device_id: 0x73BF };
        assert!(lc.description().contains("RDNA"));
        assert_eq!(lc.rebind_strategy("vfio-pci"), RebindStrategy::SimpleBind);
        assert_eq!(lc.rebind_strategy("amdgpu"), RebindStrategy::PmResetAndBind);
        assert_eq!(lc.settle_secs("any"), 12);
    }

    #[test]
    fn intel_xe_description_and_settle() {
        let lc = IntelXeLifecycle { device_id: 0x56a0 };
        assert!(lc.description().contains("Intel"));
        assert_eq!(lc.settle_secs("i915"), 5);
        assert_eq!(lc.settle_secs("xe"), 5);
    }

    #[test]
    fn brainchip_lifecycle_basics() {
        let lc = BrainChipLifecycle { device_id: 0x0001 };
        assert!(lc.description().contains("BrainChip"));
        assert_eq!(lc.rebind_strategy("akida"), RebindStrategy::SimpleBind);
        assert_eq!(lc.settle_secs("akida"), 3);
    }

    #[test]
    fn generic_description_and_settle() {
        let lc = GenericLifecycle {
            vendor_id: 0xdead,
            device_id: 0xbeef,
        };
        assert!(lc.description().contains("Unknown vendor"));
        assert_eq!(lc.settle_secs("any"), 10);
        assert_eq!(lc.rebind_strategy("vfio"), RebindStrategy::SimpleBind);
    }

    #[test]
    fn rebind_strategy_debug_format() {
        assert!(format!("{:?}", RebindStrategy::SimpleBind).contains("SimpleBind"));
        assert!(format!("{:?}", RebindStrategy::PmResetAndBind).contains("PmResetAndBind"));
        assert!(format!("{:?}", RebindStrategy::PciRescan).contains("PciRescan"));
        assert!(
            format!("{:?}", RebindStrategy::SimpleWithRescanFallback)
                .contains("SimpleWithRescanFallback")
        );
    }

    #[test]
    fn rebind_strategy_clone_eq() {
        let a = RebindStrategy::SimpleBind;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn detect_lifecycle_unknown_vendor_is_generic() {
        let lc = detect_lifecycle("9999:99:99.9");
        assert!(lc.description().contains("Unknown"));
    }

    #[test]
    fn generic_prepare_vfio_pci_clears_reset_method_path_best_effort() {
        let lc = GenericLifecycle {
            vendor_id: 0xdead,
            device_id: 0xbeef,
        };
        lc.prepare_for_unbind("9999:99:99.9", "vfio-pci").unwrap();
    }

    #[test]
    fn generic_prepare_non_vfio_skips_reset_override() {
        let lc = GenericLifecycle {
            vendor_id: 0xdead,
            device_id: 0xbeef,
        };
        lc.prepare_for_unbind("9999:99:99.9", "amdgpu").unwrap();
    }

    #[test]
    fn nvidia_verify_health_ok_when_sysfs_missing_or_not_d3cold() {
        let lc = NvidiaLifecycle { device_id: 0x1d81 };
        lc.verify_health("9999:99:99.9", "nouveau").unwrap();
    }

    #[test]
    fn intel_amd_rdna_brainchip_verify_health_ok_without_d3cold_sysfs() {
        let intel = IntelXeLifecycle { device_id: 0x56a0 };
        let rdna = AmdRdnaLifecycle { device_id: 0x73bf };
        let brain = BrainChipLifecycle { device_id: 1 };
        intel.verify_health("9999:99:99.9", "xe").unwrap();
        rdna.verify_health("9999:99:99.9", "amdgpu").unwrap();
        brain.verify_health("9999:99:99.9", "akida-pcie").unwrap();
    }

    #[test]
    fn generic_verify_health_ok_when_power_unknown() {
        let lc = GenericLifecycle {
            vendor_id: 0xdead,
            device_id: 0xbeef,
        };
        lc.verify_health("9999:99:99.9", "vfio-pci").unwrap();
    }

    #[test]
    fn amd_vega20_stabilize_amdgpu_writes_autosuspend_paths_best_effort() {
        let lc = AmdVega20Lifecycle { device_id: 0x66af };
        lc.stabilize_after_bind("9999:99:99.9", "amdgpu");
    }

    #[test]
    fn amd_vega20_stabilize_non_amdgpu_skips_autosuspend() {
        let lc = AmdVega20Lifecycle { device_id: 0x66af };
        lc.stabilize_after_bind("9999:99:99.9", "vfio-pci");
    }

    #[test]
    fn nvidia_settle_secs_branches() {
        let lc = NvidiaLifecycle { device_id: 1 };
        assert_eq!(lc.settle_secs("vfio-pci"), 5);
        assert_eq!(lc.settle_secs("other"), 5);
    }

    #[test]
    fn generic_rebind_vfio_alias() {
        let lc = GenericLifecycle {
            vendor_id: 1,
            device_id: 2,
        };
        assert_eq!(lc.rebind_strategy("vfio"), RebindStrategy::SimpleBind);
    }

    #[test]
    fn lifecycle_from_pci_ids_matches_each_vendor_arm() {
        let nvidia = lifecycle_from_pci_ids(0x10de, 0x1d81);
        assert!(nvidia.description().contains("NVIDIA"));

        let vega = lifecycle_from_pci_ids(0x1002, 0x66a0);
        assert!(vega.description().contains("Vega 20"));

        let rdna = lifecycle_from_pci_ids(0x1002, 0x73bf);
        assert!(rdna.description().contains("RDNA"));

        let intel = lifecycle_from_pci_ids(0x8086, 0x56a0);
        assert!(intel.description().contains("Intel"));

        let akida = lifecycle_from_pci_ids(0x1e7c, 0xbca1);
        assert!(akida.description().contains("BrainChip"));

        let generic = lifecycle_from_pci_ids(0xabcd, 0x1234);
        assert!(generic.description().contains("Unknown"));
    }

    #[test]
    fn is_amd_vega20_excludes_adjacent_device_ids() {
        assert!(!is_amd_vega20(0x669f));
        assert!(!is_amd_vega20(0x66b0));
    }

    #[test]
    fn amd_vega20_prepare_for_unbind_errors_on_garbage_bdf() {
        let lc = AmdVega20Lifecycle { device_id: 0x66a0 };
        let err = lc
            .prepare_for_unbind("not-a-bdf", "vfio-pci")
            .expect_err("reset_method sysfs");
        assert!(!err.is_empty());
    }

    #[test]
    fn nvidia_oracle_lifecycle_description_and_strategy() {
        let lc = NvidiaOracleLifecycle {
            device_id: 0x1d81,
            module_name: "nvidia_oracle_535".to_string(),
        };
        assert!(lc.description().contains("Oracle"));
        assert_eq!(lc.rebind_strategy("amdgpu"), RebindStrategy::SimpleBind);
        assert_eq!(lc.settle_secs("nouveau"), 10);
        assert_eq!(lc.settle_secs("nvidia_oracle_535"), 8);
    }

    #[test]
    fn nvidia_oracle_prepare_and_verify_best_effort_on_missing_sysfs() {
        let lc = NvidiaOracleLifecycle {
            device_id: 0x1d81,
            module_name: "nvidia_oracle".to_string(),
        };
        let err = lc
            .prepare_for_unbind("9999:99:99.9", "vfio-pci")
            .expect_err("reset_method write on absent device");
        assert!(!err.is_empty());
        lc.verify_health("9999:99:99.9", "nvidia_oracle")
            .expect("health when power state unknown");
    }

    #[test]
    fn detect_lifecycle_for_target_nvidia_oracle_prefix_uses_oracle_lifecycle() {
        let lc = detect_lifecycle_for_target("9999:99:99.9", "nvidia_oracle");
        assert!(lc.description().contains("Oracle"));
        let lc_suffixed = detect_lifecycle_for_target("9999:99:99.9", "nvidia_oracle_535");
        assert!(lc_suffixed.description().contains("Oracle"));
    }

    #[test]
    fn detect_lifecycle_for_target_plain_driver_delegates_to_detect_lifecycle() {
        let oracle = detect_lifecycle_for_target("9999:99:99.9", "nvidia_oracle");
        let plain = detect_lifecycle_for_target("9999:99:99.9", "nouveau");
        assert!(oracle.description().contains("Oracle"));
        assert!(
            plain.description().contains("Unknown"),
            "non-oracle target should use PCI-detected lifecycle"
        );
    }
}
