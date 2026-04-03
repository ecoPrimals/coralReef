// SPDX-License-Identifier: AGPL-3.0-only
//! PCI topology helpers — upstream bridge power, parent bridge discovery,
//! bridge-level SBR, and remove/rescan cycles.

use coral_driver::linux_paths;
use std::time::Duration;

/// Pin power on all upstream PCI bridges to prevent them from
/// powering down after a device remove. Walks the sysfs topology
/// from the device up to the root port.
///
/// Uses direct writes — bridge power attributes are config-space
/// and always complete synchronously.
pub fn pin_bridge_power(bdf: &str) {
    let device_path = linux_paths::sysfs_pci_device_path(bdf);
    let Ok(real_path) = std::fs::canonicalize(&device_path) else {
        return;
    };

    let mut current = real_path.parent();
    while let Some(parent) = current {
        let power_control = parent.join("power/control");
        let d3cold = parent.join("d3cold_allowed");

        if power_control.exists() {
            let _ = super::sysfs_write_direct(power_control.to_str().unwrap_or(""), "on");
            let _ = super::sysfs_write_direct(d3cold.to_str().unwrap_or(""), "0");
        }

        if parent
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("pci"))
        {
            break;
        }
        current = parent.parent();
    }
}

/// Discover the parent PCI bridge for a device by walking sysfs.
///
/// Returns the BDF of the parent bridge (e.g. `0000:00:01.3` for a device
/// at `0000:03:00.0`). Returns `None` if the topology cannot be resolved.
pub fn find_parent_bridge(bdf: &str) -> Option<String> {
    let device_path = linux_paths::sysfs_pci_device_path(bdf);
    let real_path = std::fs::canonicalize(&device_path).ok()?;
    let parent = real_path.parent()?;
    let parent_name = parent.file_name()?.to_str()?;

    // Parent directory should be a PCI BDF like "0000:00:01.3"
    if parent_name.contains(':') && parent_name.contains('.') {
        tracing::debug!(bdf, bridge = parent_name, "found parent PCI bridge");
        Some(parent_name.to_string())
    } else {
        tracing::debug!(bdf, parent = parent_name, "parent is not a PCI bridge");
        None
    }
}

/// Reset a device via its parent PCI bridge's `reset` file (bridge-level SBR).
///
/// This is the correct reset mechanism for hardware that lacks FLR (like GV100).
/// Writing to the bridge's reset triggers a Secondary Bus Reset that affects all
/// devices behind the bridge. This works even when the device is VFIO-bound,
/// unlike the device-level `reset` file which often fails with I/O errors on
/// FLR-incapable hardware.
pub fn pci_bridge_reset(bdf: &str) -> Result<(), String> {
    let bridge_bdf = find_parent_bridge(bdf)
        .ok_or_else(|| format!("{bdf}: cannot find parent PCI bridge for bridge-level SBR"))?;

    let bridge_reset = linux_paths::sysfs_pci_device_file(&bridge_bdf, "reset");
    if !std::path::Path::new(&bridge_reset).exists() {
        return Err(format!(
            "{bdf}: parent bridge {bridge_bdf} has no reset file"
        ));
    }

    tracing::info!(
        bdf,
        bridge = %bridge_bdf,
        path = %bridge_reset,
        "triggering bridge-level SBR"
    );
    super::sysfs_write(&bridge_reset, "1")?;

    // Brief settle after bridge reset — device needs time to re-enumerate
    std::thread::sleep(Duration::from_millis(500));

    // Re-pin power after reset (bridge reset can change power state)
    super::pin_power(bdf);
    pin_bridge_power(bdf);

    tracing::info!(bdf, bridge = %bridge_bdf, "bridge-level SBR complete");
    Ok(())
}

/// Full PCI remove + bus rescan cycle. This is the most aggressive reset
/// available: it tears down the kernel's entire device tree entry and
/// forces full re-enumeration and driver re-probe on rescan.
///
/// Used as a fallback when both device-level and bridge-level resets fail.
/// WARNING: The device will be absent from sysfs between remove and rescan.
/// VFIO fds become invalid and must be reacquired after rescan.
pub fn pci_remove_rescan(bdf: &str) -> Result<(), String> {
    pci_remove_rescan_targeted(bdf, None)
}

/// PCI remove + rescan with an optional target driver override.
///
/// When `target_driver` is `Some`, the kernel's `drivers_autoprobe` is
/// disabled before rescan, `driver_override` is set on the reappeared
/// device, and a manual `drivers_probe` triggers binding. This prevents
/// the kernel's `vfio-pci.ids` cmdline parameter (or any other built-in
/// match table) from reclaiming the device during rescan.
pub fn pci_remove_rescan_targeted(bdf: &str, target_driver: Option<&str>) -> Result<(), String> {
    pin_bridge_power(bdf);
    super::pin_power(bdf);

    let _ = super::sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "");

    // When targeting a specific driver, disable autoprobe so the kernel
    // does not match vfio-pci.ids (or any other ID table) during rescan.
    let autoprobe_disabled = target_driver.is_some();
    if autoprobe_disabled {
        tracing::info!(
            bdf,
            target = ?target_driver,
            "disabling drivers_autoprobe before rescan"
        );
        let _ = super::sysfs_write_direct(&linux_paths::sysfs_pci_drivers_autoprobe(), "0");
    }

    // Ensure autoprobe is re-enabled on all exit paths.
    let result = pci_remove_rescan_inner(bdf, target_driver);

    if autoprobe_disabled {
        let _ = super::sysfs_write_direct(&linux_paths::sysfs_pci_drivers_autoprobe(), "1");
        tracing::debug!(bdf, "drivers_autoprobe re-enabled");
    }

    result
}

fn pci_remove_rescan_inner(bdf: &str, target_driver: Option<&str>) -> Result<(), String> {
    tracing::info!(bdf, "PCI remove + rescan: removing device");
    pci_remove(bdf)?;

    for i in 0..6 {
        std::thread::sleep(Duration::from_secs(1));
        if !std::path::Path::new(&linux_paths::sysfs_pci_device_path(bdf)).exists() {
            tracing::info!(bdf, seconds = i + 1, "device removed from sysfs");
            break;
        }
    }

    std::thread::sleep(Duration::from_secs(2));

    tracing::info!(bdf, "PCI remove + rescan: rescanning bus");
    pci_rescan()?;

    for i in 0..10 {
        std::thread::sleep(Duration::from_secs(1));
        if std::path::Path::new(&linux_paths::sysfs_pci_device_path(bdf)).exists() {
            tracing::info!(bdf, seconds = i + 1, "device re-appeared after rescan");
            super::pin_power(bdf);
            pin_bridge_power(bdf);
            let _ =
                super::sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "");

            if let Some(driver) = target_driver {
                tracing::info!(
                    bdf,
                    driver,
                    "setting driver_override before probe (autoprobe disabled)"
                );
                let _ = super::sysfs_write_direct(
                    &linux_paths::sysfs_pci_device_file(bdf, "driver_override"),
                    driver,
                );
                tracing::info!(bdf, "triggering manual drivers_probe");
                let _ = super::sysfs_write(&linux_paths::sysfs_pci_drivers_probe(), bdf);
            }

            return Ok(());
        }
    }

    Err(format!("{bdf}: device did not re-appear after PCI rescan"))
}

/// Remove a PCI device from the kernel's device tree.
/// This forces full cleanup of sysfs entries, DRM nodes, hwmon, etc.
pub fn pci_remove(bdf: &str) -> Result<(), String> {
    let path = linux_paths::sysfs_pci_device_file(bdf, "remove");
    super::sysfs_write(&path, "1")
}

/// Trigger a PCI bus rescan, causing the kernel to re-enumerate
/// all devices and probe matching drivers.
pub fn pci_rescan() -> Result<(), String> {
    super::sysfs_write(&linux_paths::sysfs_pci_bus_rescan(), "1")
}
