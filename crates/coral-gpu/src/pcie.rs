// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

use coral_reef::GpuTarget;

#[cfg(target_os = "linux")]
use crate::driver;

/// `PCIe` topology information for multi-GPU device grouping.
///
/// Used by `shader.compile.wgsl.multi` to communicate device affinity.
/// Devices on the same `PCIe` switch have lower inter-device latency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcieDeviceInfo {
    /// Render node path (e.g. `/dev/dri/renderD128`).
    pub render_node: String,
    /// `PCIe` bus address (e.g. `0000:01:00.0`).
    pub pcie_address: Option<String>,
    /// `PCIe` switch group (devices sharing a switch get the same ID).
    pub switch_group: Option<u32>,
    /// GPU target architecture.
    pub target: GpuTarget,
}

#[cfg(target_os = "linux")]
const DEFAULT_DRI_PATH: &str = "/dev/dri";

#[cfg(target_os = "linux")]
fn dri_base_path() -> std::path::PathBuf {
    std::env::var("CORALREEF_DRI_PATH")
        .ok()
        .filter(|s| !s.is_empty())
        .map_or_else(
            || std::path::PathBuf::from(DEFAULT_DRI_PATH),
            std::path::PathBuf::from,
        )
}

/// Probe `PCIe` topology for all available GPU render nodes.
///
/// Reads sysfs to discover render nodes, their `PCIe` addresses, and
/// groups them by shared `PCIe` switch (based on common bus prefix).
#[cfg(target_os = "linux")]
#[must_use]
pub fn probe_pcie_topology() -> Vec<PcieDeviceInfo> {
    let dri_path = dri_base_path();
    let mut devices = Vec::new();

    let Ok(entries) = std::fs::read_dir(&dri_path) else {
        return devices;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("renderD") {
            continue;
        }

        let render_path = dri_path.join(&name).to_string_lossy().into_owned();
        let sysfs_device = format!("/sys/class/drm/{name_str}/device");

        let pcie_address = std::fs::read_link(&sysfs_device)
            .ok()
            .and_then(|link| link.file_name().map(|n| n.to_string_lossy().into_owned()));

        let vendor = std::fs::read_to_string(format!("{sysfs_device}/vendor"))
            .ok()
            .and_then(|v| u16::from_str_radix(v.trim().trim_start_matches("0x"), 16).ok());

        let target = match vendor {
            Some(coral_driver::nv::identity::PCI_VENDOR_NVIDIA) => {
                let sm = driver::sm_from_sysfs(&render_path);
                GpuTarget::Nvidia(driver::sm_to_nvarch(sm))
            }
            Some(coral_driver::nv::identity::PCI_VENDOR_AMD) => {
                GpuTarget::Amd(driver::amd_arch_from_sysfs(&render_path))
            }
            _ => continue,
        };

        devices.push(PcieDeviceInfo {
            render_node: render_path,
            pcie_address,
            switch_group: None,
            target,
        });
    }

    assign_switch_groups(&mut devices);
    devices
}

/// Probe `PCIe` topology for all available GPU render nodes.
///
/// On non-Linux targets there is no sysfs-based discovery; returns an empty list.
#[cfg(not(target_os = "linux"))]
#[must_use]
pub fn probe_pcie_topology() -> Vec<PcieDeviceInfo> {
    Vec::new()
}

/// Group devices by shared `PCIe` switch based on bus address prefix.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[expect(clippy::redundant_pub_crate, reason = "needed by crate::tests::pcie")]
pub(crate) fn assign_switch_groups(devices: &mut [PcieDeviceInfo]) {
    let mut group_map: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut next_group = 0u32;

    for device in devices.iter_mut() {
        if let Some(ref addr) = device.pcie_address {
            let prefix = addr.split(':').take(2).collect::<Vec<_>>().join(":");
            let group = *group_map.entry(prefix).or_insert_with(|| {
                let g = next_group;
                next_group += 1;
                g
            });
            device.switch_group = Some(group);
        }
    }
}
