// SPDX-License-Identifier: AGPL-3.0-or-later
//! AMD GPU lifecycle implementations (Vega 20, RDNA).

use crate::error::SwapError;
use crate::sysfs;
use coral_driver::linux_paths;

use super::types::{RebindStrategy, VendorLifecycle};

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

    fn prepare_for_unbind(&self, bdf: &str, _current_driver: &str) -> Result<(), SwapError> {
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

    fn verify_health(&self, bdf: &str, target_driver: &str) -> Result<(), SwapError> {
        let power = sysfs::read_power_state(bdf);
        if power.as_deref() == Some("D3cold") {
            return Err(SwapError::VerifyHealth {
                bdf: bdf.to_string(),
                detail: "AMD Vega 20 in D3cold — SMU firmware lost, reboot required".to_string(),
            });
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

    fn prepare_for_unbind(&self, bdf: &str, _current_driver: &str) -> Result<(), SwapError> {
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

    fn verify_health(&self, bdf: &str, _target_driver: &str) -> Result<(), SwapError> {
        let power = sysfs::read_power_state(bdf);
        if power.as_deref() == Some("D3cold") {
            return Err(SwapError::VerifyHealth {
                bdf: bdf.to_string(),
                detail: "AMD RDNA in D3cold after bind".to_string(),
            });
        }
        Ok(())
    }
}
