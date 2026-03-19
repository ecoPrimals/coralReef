// SPDX-License-Identifier: AGPL-3.0-only
//! Sysfs and PCI bus helpers for device discovery and lifecycle management.
//!
//! Encapsulates all direct Linux sysfs filesystem interactions:
//! PCI identity probing, IOMMU group management, DRM consumer detection,
//! driver bind/unbind, and power state queries.

use crate::pci_ids;

/// Write to a sysfs path using direct filesystem access.
///
/// Requires `CAP_SYS_ADMIN` or udev rules for the target path.
/// Does NOT fall back to `sudo` — the binary should be deployed with
/// the correct capabilities via systemd `AmbientCapabilities=CAP_SYS_ADMIN`.
///
/// Returns the I/O result so callers can decide whether to log or propagate.
pub fn sysfs_write(path: &str, value: &str) -> Result<(), std::io::Error> {
    std::fs::write(path, value).inspect_err(|e| {
        tracing::warn!(
            path,
            error = %e,
            "sysfs write failed — ensure CAP_SYS_ADMIN or udev rules grant access"
        );
    })
}

/// Read PCI vendor and device IDs from sysfs.
pub fn read_pci_ids(bdf: &str) -> (u16, u16) {
    let vendor = std::fs::read_to_string(format!("/sys/bus/pci/devices/{bdf}/vendor"))
        .ok()
        .and_then(|s| u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
        .unwrap_or(0);
    let device = std::fs::read_to_string(format!("/sys/bus/pci/devices/{bdf}/device"))
        .ok()
        .and_then(|s| u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
        .unwrap_or(0);
    (vendor, device)
}

/// Read the IOMMU group number for a PCI device.
pub fn read_iommu_group(bdf: &str) -> u32 {
    std::fs::read_link(format!("/sys/bus/pci/devices/{bdf}/iommu_group"))
        .ok()
        .and_then(|p| p.file_name()?.to_str()?.parse().ok())
        .unwrap_or(0)
}

/// Identify a GPU chip from PCI vendor/device IDs.
#[must_use]
pub fn identify_chip(vendor: u16, device: u16) -> String {
    match (vendor, device) {
        (pci_ids::NVIDIA_VENDOR_ID, pci_ids::TITAN_V_DEVICE_ID) => "GV100 (Titan V)".into(),
        (pci_ids::NVIDIA_VENDOR_ID, 0x1db1) => "GV100GL (V100)".into(),
        (pci_ids::NVIDIA_VENDOR_ID, 0x2204) => "GA102 (RTX 3090)".into(),
        (pci_ids::NVIDIA_VENDOR_ID, 0x2d05) => "GB206 (RTX 5060)".into(),
        (pci_ids::AMD_VENDOR_ID, pci_ids::MI50_DEVICE_ID) => "Vega 20 (MI50)".into(),
        (pci_ids::AMD_VENDOR_ID, pci_ids::MI60_DEVICE_ID) => "Vega 20 (MI60)".into(),
        (pci_ids::AMD_VENDOR_ID, pci_ids::RADEON_VII_DEVICE_ID) => "Vega 20 (Radeon VII)".into(),
        (v, d) => format!("{v:#06x}:{d:#06x}"),
    }
}

/// Read the current kernel driver bound to a PCI device.
pub fn read_current_driver(bdf: &str) -> Option<String> {
    std::fs::read_link(format!("/sys/bus/pci/devices/{bdf}/driver"))
        .ok()
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
}

/// Ensure all devices in the same IOMMU group are bound to `vfio-pci`.
///
/// VFIO requires group viability: every device in the group must use `vfio-pci`.
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

        tracing::info!(
            peer = %peer_bdf,
            driver = driver.as_deref().unwrap_or("none"),
            group = group_id,
            "binding IOMMU group peer to vfio-pci"
        );

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

/// Check whether a DRM device has active consumer fds (open file handles).
///
/// Scans `/proc/*/fd` for symlinks pointing to the device's DRM render node.
/// Returns `true` if any non-self process holds a DRM fd open.
pub fn has_active_drm_consumers(bdf: &str) -> bool {
    let drm_dir = format!("/sys/bus/pci/devices/{bdf}/drm");
    let Ok(entries) = std::fs::read_dir(&drm_dir) else {
        return false;
    };

    let drm_paths: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("card") || name.starts_with("renderD") {
                Some(format!("/dev/dri/{name}"))
            } else {
                None
            }
        })
        .collect();

    if drm_paths.is_empty() {
        return false;
    }

    let self_pid = std::process::id();
    let Ok(proc_entries) = std::fs::read_dir("/proc") else {
        return false;
    };

    for entry in proc_entries.flatten() {
        let pid_str = entry.file_name().to_string_lossy().to_string();
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };
        if pid == self_pid {
            continue;
        }

        let fd_dir = format!("/proc/{pid}/fd");
        let Ok(fds) = std::fs::read_dir(&fd_dir) else {
            continue;
        };

        for fd_entry in fds.flatten() {
            if let Ok(target) = std::fs::read_link(fd_entry.path()) {
                let target_str = target.to_string_lossy();
                if drm_paths.iter().any(|p| target_str.as_ref() == p.as_str()) {
                    tracing::debug!(
                        pid,
                        fd = ?fd_entry.file_name(),
                        target = %target_str,
                        "active DRM consumer found"
                    );
                    return true;
                }
            }
        }
    }

    false
}

/// Find the DRM card device path for a PCI device.
pub fn find_drm_card(bdf: &str) -> Option<String> {
    let drm_dir = format!("/sys/bus/pci/devices/{bdf}/drm");
    let entries = std::fs::read_dir(&drm_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("card") {
            return Some(format!("/dev/dri/{name}"));
        }
    }
    None
}

/// Read a PCI power state from sysfs.
pub fn read_power_state(bdf: &str) -> super::device::PowerState {
    use super::device::PowerState;
    let path = format!("/sys/bus/pci/devices/{bdf}/power_state");
    std::fs::read_to_string(&path).map_or(PowerState::Unknown, |s| match s.trim() {
        "D0" => PowerState::D0,
        "D3hot" => PowerState::D3Hot,
        "D3cold" => PowerState::D3Cold,
        _ => PowerState::Unknown,
    })
}

/// Read PCI link width from sysfs.
pub fn read_link_width(bdf: &str) -> Option<u8> {
    let path = format!("/sys/bus/pci/devices/{bdf}/current_link_width");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identify_chip_known_nvidia() {
        assert_eq!(identify_chip(0x10de, 0x1d81), "GV100 (Titan V)");
        assert_eq!(identify_chip(0x10de, 0x1db1), "GV100GL (V100)");
        assert_eq!(identify_chip(0x10de, 0x2204), "GA102 (RTX 3090)");
        assert_eq!(identify_chip(0x10de, 0x2d05), "GB206 (RTX 5060)");
    }

    #[test]
    fn test_identify_chip_known_amd() {
        assert_eq!(identify_chip(0x1002, 0x66a0), "Vega 20 (MI50)");
        assert_eq!(identify_chip(0x1002, 0x66a1), "Vega 20 (MI60)");
    }

    #[test]
    fn test_identify_chip_unknown() {
        let name = identify_chip(0x1234, 0x5678);
        assert!(name.contains("0x1234"));
        assert!(name.contains("0x5678"));
    }

    #[test]
    fn test_read_pci_ids_nonexistent() {
        let (vendor, device) = read_pci_ids("9999:99:99.9");
        assert_eq!(vendor, 0);
        assert_eq!(device, 0);
    }

    #[test]
    fn test_read_iommu_group_nonexistent() {
        assert_eq!(read_iommu_group("9999:99:99.9"), 0);
    }

    #[test]
    fn test_read_current_driver_nonexistent() {
        assert!(read_current_driver("9999:99:99.9").is_none());
    }

    #[test]
    fn test_has_active_drm_consumers_nonexistent() {
        assert!(!has_active_drm_consumers("9999:99:99.9"));
    }

    #[test]
    fn test_find_drm_card_nonexistent() {
        assert!(find_drm_card("9999:99:99.9").is_none());
    }

    #[test]
    fn test_read_power_state_nonexistent() {
        let state = read_power_state("9999:99:99.9");
        assert_eq!(state, crate::device::PowerState::Unknown);
    }

    #[test]
    fn test_read_link_width_nonexistent() {
        assert!(read_link_width("9999:99:99.9").is_none());
    }
}
