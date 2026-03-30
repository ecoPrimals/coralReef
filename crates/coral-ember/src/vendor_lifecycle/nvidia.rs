// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA GPU lifecycle implementations (Kepler, Volta+, Open, Oracle).

use crate::sysfs;
use coral_driver::linux_paths;

use super::types::{RebindStrategy, ResetMethod, VendorError, VendorLifecycle};

// ---------------------------------------------------------------------------
// NVIDIA Kepler lifecycle (GK110, GK210 — Tesla K80, GTX Titan, 780 Ti)
// ---------------------------------------------------------------------------

/// NVIDIA Kepler GPUs — GDDR5, no FLR, no bus SBR, cold-hardware-sensitive.
///
/// Kepler differs fundamentally from Volta+ (HBM2) GPUs:
///
/// - **GDDR5**: Bus reset does not destroy VRAM training (unlike HBM2).
/// - **No FLR**: `reset_method` is empty — the kernel has no reset path.
/// - **Cold hardware**: When claimed by vfio-pci at boot without a prior
///   VBIOS POST, PCI config-space writes can trigger PCIe completion
///   timeouts that put the writing thread into uninterruptible D-state.
/// - **Shared root complex**: K80 (dual-die) shares PCIe root complex
///   with USB controllers; bridge power must be pinned to prevent
///   cascade failures.
///
/// The safe lifecycle: boot with nouveau (which POSTs the device),
/// then swap to vfio-pci via ember. Never unbind vfio-pci from cold
/// Kepler hardware — it will D-state.
#[derive(Debug)]
pub struct NvidiaKeplerLifecycle {
    /// PCI device ID from config space.
    #[expect(dead_code, reason = "reserved for GK110 vs GK210 differences")]
    pub device_id: u16,
}

impl VendorLifecycle for NvidiaKeplerLifecycle {
    fn description(&self) -> &str {
        "NVIDIA Kepler (GDDR5, no FLR — cold vfio-pci unbind causes D-state)"
    }

    fn available_reset_methods(&self) -> Vec<ResetMethod> {
        // Kepler has no FLR and no device-level SBR. Bridge SBR exists
        // but is risky on multi-die boards (K80) sharing root complexes
        // with USB. Remove+rescan is the only reliable fallback.
        vec![ResetMethod::RemoveRescan]
    }

    fn prepare_for_unbind(&self, bdf: &str, _current_driver: &str) -> Result<(), VendorError> {
        sysfs::pin_power(bdf);
        sysfs::pin_bridge_power(bdf);

        let reset_path = linux_paths::sysfs_pci_device_file(bdf, "reset_method");
        let _ = sysfs::sysfs_write_direct(&reset_path, "");

        Ok(())
    }

    fn rebind_strategy(&self, target_driver: &str) -> RebindStrategy {
        match target_driver {
            "vfio" | "vfio-pci" => RebindStrategy::SimpleBind,
            _ => RebindStrategy::SimpleWithRescanFallback,
        }
    }

    fn settle_secs(&self, target_driver: &str) -> u64 {
        match target_driver {
            "nouveau" => 20,
            _ => 5,
        }
    }

    fn stabilize_after_bind(&self, bdf: &str, _target_driver: &str) {
        sysfs::pin_power(bdf);
        sysfs::pin_bridge_power(bdf);

        let _ =
            sysfs::sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "");
    }

    fn verify_health(&self, bdf: &str, _target_driver: &str) -> Result<(), VendorError> {
        let power = sysfs::read_power_state(bdf);
        if power.as_deref() == Some("D3cold") {
            return Err(VendorError::HealthCheck {
                bdf: bdf.to_string(),
                detail: "Kepler device in D3cold after bind".to_string(),
            });
        }
        Ok(())
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

    fn prepare_for_unbind(&self, bdf: &str, _current_driver: &str) -> Result<(), VendorError> {
        sysfs::pin_power(bdf);

        tracing::info!(
            bdf,
            "NVIDIA: disabling reset_method (bus reset destroys HBM2 training)"
        );
        sysfs::sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "")
            .map_err(VendorError::Sysfs)?;

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

    fn verify_health(&self, bdf: &str, _target_driver: &str) -> Result<(), VendorError> {
        let power = sysfs::read_power_state(bdf);
        if power.as_deref() == Some("D3cold") {
            return Err(VendorError::HealthCheck {
                bdf: bdf.to_string(),
                detail: "device in D3cold after bind".to_string(),
            });
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

    fn prepare_for_unbind(&self, bdf: &str, _current_driver: &str) -> Result<(), VendorError> {
        sysfs::pin_power(bdf);
        tracing::info!(
            bdf,
            "NVIDIA Open: disabling reset_method (bus reset destroys HBM2 training)"
        );
        sysfs::sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "")
            .map_err(VendorError::Sysfs)?;
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

    fn verify_health(&self, bdf: &str, _target_driver: &str) -> Result<(), VendorError> {
        let power = sysfs::read_power_state(bdf);
        if power.as_deref() == Some("D3cold") {
            return Err(VendorError::HealthCheck {
                bdf: bdf.to_string(),
                detail: "NVIDIA Open device in D3cold after bind".to_string(),
            });
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

    fn prepare_for_unbind(&self, bdf: &str, _current_driver: &str) -> Result<(), VendorError> {
        sysfs::pin_power(bdf);
        sysfs::sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "")
            .map_err(VendorError::Sysfs)?;
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

    fn verify_health(&self, bdf: &str, _target_driver: &str) -> Result<(), VendorError> {
        let power = sysfs::read_power_state(bdf);
        if power.as_deref() == Some("D3cold") {
            return Err(VendorError::HealthCheck {
                bdf: bdf.to_string(),
                detail: "device in D3cold after bind".to_string(),
            });
        }
        Ok(())
    }
}
