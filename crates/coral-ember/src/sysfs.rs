// SPDX-License-Identifier: AGPL-3.0-only
//! Sysfs helpers — self-contained in ember to avoid cross-crate dependency
//! on glowplug internals. Ember is the sole writer of driver/unbind and bind.

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
        .and_then(|p| p.file_name()?.to_str()?.parse().ok())
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
    let _ = sysfs_write(
        &format!("/sys/bus/pci/devices/{bdf}/power/control"),
        "on",
    );
    let _ = sysfs_write(
        &format!("/sys/bus/pci/devices/{bdf}/d3cold_allowed"),
        "0",
    );
}
