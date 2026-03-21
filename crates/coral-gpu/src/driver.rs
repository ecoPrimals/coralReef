// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

use coral_reef::{GpuTarget, NvArch};

use crate::preference;

/// Fallback SM when sysfs detection fails and `$CORALREEF_DEFAULT_SM` is unset (Ampere GA102).
pub const DEFAULT_NV_SM: u32 = 86;
/// Fallback SM for nouveau path when detection fails and `$CORALREEF_DEFAULT_SM_NOUVEAU` is unset (Volta).
pub const DEFAULT_NV_SM_NOUVEAU: u32 = 70;

/// Default NVIDIA SM architecture for sysfs-based fallback detection.
///
/// Checks `$CORALREEF_DEFAULT_SM` environment variable first (e.g. "70", "86"),
/// falling back to SM 86 (Ampere GA102, RTX 3090/3080/3070).
pub(crate) fn default_nv_sm() -> u32 {
    std::env::var("CORALREEF_DEFAULT_SM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_NV_SM)
}

/// Default NVIDIA SM for the nouveau sovereign path when sysfs detection fails.
///
/// Checks `$CORALREEF_DEFAULT_SM_NOUVEAU` environment variable first,
/// falling back to SM 70 (Volta).
pub(crate) fn default_nv_sm_nouveau() -> u32 {
    std::env::var("CORALREEF_DEFAULT_SM_NOUVEAU")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_NV_SM_NOUVEAU)
}

/// Map an SM version number to the corresponding `NvArch`.
#[cfg(target_os = "linux")]
pub(crate) const fn sm_to_nvarch(sm: u32) -> NvArch {
    match sm {
        75 => NvArch::Sm75,
        80 => NvArch::Sm80,
        86 => NvArch::Sm86,
        89 => NvArch::Sm89,
        _ => NvArch::Sm70,
    }
}

/// Detect the NVIDIA SM version from any available render node.
/// Falls back to the provided default if detection fails.
#[cfg(all(target_os = "linux", feature = "nvidia-drm"))]
pub(crate) fn sm_from_sysfs_or(default: u32) -> u32 {
    use coral_driver::drm::enumerate_render_nodes;
    for node in enumerate_render_nodes() {
        if node.driver == preference::DRIVER_NVIDIA_DRM {
            return coral_driver::nv::ioctl::probe_gpu_identity(&node.path)
                .and_then(|id| id.nvidia_sm())
                .unwrap_or(default);
        }
    }
    default
}

/// Detect the NVIDIA SM version from sysfs for a render node path.
#[cfg(target_os = "linux")]
pub(crate) fn sm_from_sysfs(path: &str) -> u32 {
    coral_driver::nv::ioctl::probe_gpu_identity(path)
        .and_then(|id| id.nvidia_sm())
        .unwrap_or_else(default_nv_sm_nouveau)
}

/// Detect the GPU target from sysfs for an nvidia-drm render node.
#[cfg(all(target_os = "linux", feature = "nvidia-drm"))]
pub(crate) fn sm_target_from_sysfs(path: &str) -> GpuTarget {
    let sm = coral_driver::nv::ioctl::probe_gpu_identity(path)
        .and_then(|id| id.nvidia_sm())
        .unwrap_or_else(default_nv_sm);
    GpuTarget::Nvidia(sm_to_nvarch(sm))
}

/// Map an SM version to the NVIDIA compute class constant.
#[cfg(all(target_os = "linux", feature = "vfio"))]
pub(crate) const fn sm_to_compute_class(sm: u32) -> u32 {
    match sm {
        70..=74 => coral_driver::nv::pushbuf::class::VOLTA_COMPUTE_A,
        75..=79 => coral_driver::nv::pushbuf::class::TURING_COMPUTE_A,
        _ => coral_driver::nv::pushbuf::class::AMPERE_COMPUTE_A,
    }
}

/// Discover a VFIO-bound NVIDIA GPU by scanning sysfs for `vfio-pci` bindings.
///
/// Returns the first BDF address of an NVIDIA GPU bound to `vfio-pci`, or `None`.
#[cfg(all(target_os = "linux", feature = "vfio"))]
pub(crate) fn discover_vfio_nvidia_bdf() -> Option<String> {
    let vfio_dir = std::path::Path::new("/sys/bus/pci/drivers/vfio-pci");
    let entries = std::fs::read_dir(vfio_dir).ok()?;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let bdf = name.to_string_lossy();
        if !bdf.contains(':') {
            continue;
        }

        let vendor_path = format!("/sys/bus/pci/devices/{bdf}/vendor");
        if let Ok(vendor_str) = std::fs::read_to_string(&vendor_path) {
            let vendor_str = vendor_str.trim().trim_start_matches("0x");
            if let Ok(vendor) = u16::from_str_radix(vendor_str, 16)
                && vendor == coral_driver::nv::identity::PCI_VENDOR_NVIDIA
            {
                tracing::info!(bdf = %bdf, "discovered VFIO-bound NVIDIA GPU");
                return Some(bdf.into_owned());
            }
        }
    }
    None
}

/// Map a PCI device ID (from sysfs) to an SM major version for VFIO paths.
///
/// Used by [`vfio_detect_sm`] and unit-tested without sysfs I/O.
#[cfg(all(target_os = "linux", feature = "vfio"))]
pub(crate) fn vfio_sm_from_device_id(device_id: Option<u16>) -> u32 {
    match device_id {
        Some(0x1D81) => 70,                            // Titan V
        Some(0x1E00..=0x1E8F) => 75,                   // Turing (TU10x)
        Some(0x2200..=0x2203 | 0x2207..=0x22FF) => 80, // GA100
        Some(0x2204..=0x2206) => 86,                   // GA102 (RTX 3090/3080)
        Some(0x2300..=0x23FF) => 86,                   // GA10x
        Some(0x2400..=0x26FF) => 89,                   // Ada Lovelace
        _ => default_nv_sm(),
    }
}

/// Detect SM version for a VFIO-bound GPU from sysfs device ID.
#[cfg(all(target_os = "linux", feature = "vfio"))]
pub(crate) fn vfio_detect_sm(bdf: &str) -> u32 {
    let device_path = format!("/sys/bus/pci/devices/{bdf}/device");
    let device_id = std::fs::read_to_string(&device_path)
        .ok()
        .and_then(|s| u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok());

    vfio_sm_from_device_id(device_id)
}
