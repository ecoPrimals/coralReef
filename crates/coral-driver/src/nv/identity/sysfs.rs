// SPDX-License-Identifier: AGPL-3.0-or-later

use super::gpu_identity::GpuIdentity;

/// Probe sysfs for the GPU chipset on a nouveau render node.
///
/// Looks for `{sysfs}/class/drm/renderDN/device/` to identify the PCI device.
/// Returns the PCI vendor:device ID pair if readable.
#[must_use]
pub fn probe_gpu_identity(render_node_path: &str) -> Option<GpuIdentity> {
    let node_name = render_node_path.rsplit('/').next()?;
    let sysfs_device = crate::linux_paths::sysfs_class_drm_device(node_name);

    let vendor = std::fs::read_to_string(format!("{sysfs_device}/vendor")).ok()?;
    let device = std::fs::read_to_string(format!("{sysfs_device}/device")).ok()?;

    let vendor_id = u16::from_str_radix(vendor.trim().trim_start_matches("0x"), 16).ok()?;
    let device_id = u16::from_str_radix(device.trim().trim_start_matches("0x"), 16).ok()?;

    Some(GpuIdentity {
        vendor_id,
        device_id,
        sysfs_path: sysfs_device,
    })
}
