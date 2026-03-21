// SPDX-License-Identifier: AGPL-3.0-only
//! Sysfs helpers — self-contained in ember to avoid cross-crate dependency
//! on glowplug internals. Ember is the sole writer of driver/unbind and bind.

/// Parses the body of a sysfs PCI id file (e.g. `"0x10de\n"`).
#[must_use]
pub(crate) fn parse_pci_id_hex(content: &str) -> u16 {
    u16::from_str_radix(content.trim().trim_start_matches("0x"), 16).unwrap_or(0)
}

/// Parses an IOMMU group id from the last segment of a sysfs symlink target.
#[must_use]
pub(crate) fn parse_iommu_group_file_name(name: &str) -> u32 {
    name.parse().unwrap_or(0)
}

pub fn sysfs_write(path: &str, value: &str) -> Result<(), String> {
    std::fs::write(path, value).map_err(|e| format!("sysfs write {path}: {e}"))
}

pub fn read_current_driver(bdf: &str) -> Option<String> {
    std::fs::read_link(format!("/sys/bus/pci/devices/{bdf}/driver"))
        .ok()
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
}

pub fn read_iommu_group(bdf: &str) -> u32 {
    std::fs::read_link(format!("/sys/bus/pci/devices/{bdf}/iommu_group"))
        .ok()
        .and_then(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(parse_iommu_group_file_name)
        })
        .unwrap_or(0)
}

pub fn find_drm_card(bdf: &str) -> Option<String> {
    let drm_dir = format!("/sys/bus/pci/devices/{bdf}/drm");
    for entry in std::fs::read_dir(&drm_dir).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("card") {
            return Some(format!("/dev/dri/{name}"));
        }
    }
    None
}

pub fn bind_iommu_group_to_vfio(primary_bdf: &str, group_id: u32) {
    let group_path = format!("/sys/kernel/iommu_groups/{group_id}/devices");
    let Ok(entries) = std::fs::read_dir(&group_path) else {
        return;
    };
    for entry in entries.flatten() {
        let peer_bdf = entry.file_name().to_string_lossy().to_string();
        if peer_bdf == primary_bdf {
            continue;
        }
        let driver = read_current_driver(&peer_bdf);
        if driver.as_deref() == Some("vfio-pci") {
            continue;
        }
        tracing::info!(peer = %peer_bdf, group = group_id, "binding IOMMU group peer to vfio-pci");
        if driver.is_some() {
            let _ = sysfs_write(
                &format!("/sys/bus/pci/devices/{peer_bdf}/driver/unbind"),
                &peer_bdf,
            );
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let _ = sysfs_write(
            &format!("/sys/bus/pci/devices/{peer_bdf}/driver_override"),
            "vfio-pci",
        );
        let _ = sysfs_write("/sys/bus/pci/drivers/vfio-pci/bind", &peer_bdf);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// Pin power state to prevent D3 transitions during driver swaps.
pub fn pin_power(bdf: &str) {
    let _ = sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/power/control"), "on");
    let _ = sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/d3cold_allowed"), "0");
}

/// Error when a PM power cycle leaves the device in `D3cold`.
pub(crate) fn err_if_pm_cycle_d3cold(bdf: &str, after_power_state: &str) -> Result<(), String> {
    if after_power_state == "D3cold" {
        return Err(format!("{bdf}: PM power cycle resulted in D3cold"));
    }
    Ok(())
}

/// Read a PCI ID field (vendor, device, subsystem_vendor, subsystem_device).
/// Returns 0 on failure. The sysfs files contain hex values like "0x10de\n".
pub fn read_pci_id(bdf: &str, field: &str) -> u16 {
    let path = format!("/sys/bus/pci/devices/{bdf}/{field}");
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| parse_pci_id_hex(&s))
        .unwrap_or(0)
}

/// Read the current PCIe power state (D0, D3hot, D3cold, unknown).
pub fn read_power_state(bdf: &str) -> Option<String> {
    let path = format!("/sys/bus/pci/devices/{bdf}/power_state");
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
}

/// Pin power on all upstream PCI bridges to prevent them from
/// powering down after a device remove. Walks the sysfs topology
/// from the device up to the root port.
pub fn pin_bridge_power(bdf: &str) {
    let device_path = format!("/sys/bus/pci/devices/{bdf}");
    let Ok(real_path) = std::fs::canonicalize(&device_path) else {
        return;
    };

    let mut current = real_path.parent();
    while let Some(parent) = current {
        let power_control = parent.join("power/control");
        let d3cold = parent.join("d3cold_allowed");

        if power_control.exists() {
            let _ = std::fs::write(&power_control, "on");
            let _ = std::fs::write(&d3cold, "0");
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

/// Remove a PCI device from the kernel's device tree.
/// This forces full cleanup of sysfs entries, DRM nodes, hwmon, etc.
pub fn pci_remove(bdf: &str) -> Result<(), String> {
    let path = format!("/sys/bus/pci/devices/{bdf}/remove");
    sysfs_write(&path, "1")
}

/// Trigger a PCI bus rescan, causing the kernel to re-enumerate
/// all devices and probe matching drivers.
pub fn pci_rescan() -> Result<(), String> {
    sysfs_write("/sys/bus/pci/rescan", "1")
}

/// PM power cycle: transition through D3hot → D0 to reinitialize the
/// function without a bus reset. The PCIe spec requires D3hot→D0 to
/// reset function-level state while preserving PCI topology.
pub fn pm_power_cycle(bdf: &str) -> Result<(), String> {
    let power_state_path = format!("/sys/bus/pci/devices/{bdf}/power_state");

    let current = std::fs::read_to_string(&power_state_path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    tracing::info!(bdf, current_state = %current, "PM power cycle: entering D3hot");

    pin_power(bdf);
    pin_bridge_power(bdf);

    sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/power/control"), "on")?;

    std::thread::sleep(std::time::Duration::from_millis(500));

    let config_path = format!("/sys/bus/pci/devices/{bdf}/config");
    let saved_config = std::fs::read(&config_path).ok();

    sysfs_write(&power_state_path, "D3hot")?;
    std::thread::sleep(std::time::Duration::from_secs(2));

    sysfs_write(&power_state_path, "D0")?;
    std::thread::sleep(std::time::Duration::from_secs(1));

    if let Some(config) = saved_config {
        let _ = std::fs::write(&config_path, &config);
    }

    pin_power(bdf);
    pin_bridge_power(bdf);

    let after = std::fs::read_to_string(&power_state_path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    tracing::info!(bdf, power_state = %after, "PM power cycle complete");

    err_if_pm_cycle_d3cold(bdf, &after)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECTED_NVIDIA_VENDOR: u16 = 0x10de;

    #[test]
    fn parse_pci_id_hex_accepts_0x_prefix_and_whitespace() {
        assert_eq!(parse_pci_id_hex("0x10de\n"), EXPECTED_NVIDIA_VENDOR);
        assert_eq!(parse_pci_id_hex("  0xABCD  "), 0xabcd);
    }

    #[test]
    fn parse_pci_id_hex_invalid_returns_zero() {
        assert_eq!(parse_pci_id_hex("not-hex"), 0);
        assert_eq!(parse_pci_id_hex(""), 0);
    }

    #[test]
    fn parse_iommu_group_file_name_numeric() {
        const EXPECTED_GROUP: u32 = 42;
        assert_eq!(parse_iommu_group_file_name("42"), EXPECTED_GROUP);
        assert_eq!(parse_iommu_group_file_name("0"), 0);
    }

    #[test]
    fn parse_iommu_group_file_name_invalid_returns_zero() {
        assert_eq!(parse_iommu_group_file_name("not-a-number"), 0);
    }

    #[test]
    fn sysfs_write_round_trip_tmpfile() {
        let dir = std::env::temp_dir();
        let path = dir.join("coral_ember_sysfs_write_test");
        let payload = "on";
        sysfs_write(path.to_str().unwrap(), payload).unwrap();
        let read_back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(read_back, payload);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn err_if_pm_cycle_d3cold_rejects_d3cold() {
        let bdf = "0000:01:00.0";
        let err = err_if_pm_cycle_d3cold(bdf, "D3cold").unwrap_err();
        assert!(err.contains(bdf));
        assert!(err.contains("D3cold"));
    }

    #[test]
    fn err_if_pm_cycle_d3cold_accepts_other_states() {
        err_if_pm_cycle_d3cold("0000:01:00.0", "D0").unwrap();
        err_if_pm_cycle_d3cold("0000:01:00.0", "D3hot").unwrap();
    }

    #[test]
    fn pci_remove_invalid_bdf_is_error() {
        let remove_err = pci_remove("ff:ff:ff.f");
        assert!(remove_err.is_err());
    }

    #[test]
    fn sysfs_write_missing_parent_is_error() {
        let err = sysfs_write("/nonexistent-coral-ember-path/nope", "1").unwrap_err();
        assert!(err.contains("sysfs write"));
    }
}
