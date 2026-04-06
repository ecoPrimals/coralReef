// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

//! sysfs fallbacks and helpers shared by [`crate::GpuContext`] and integration tests.

#[cfg(all(target_os = "linux", feature = "vfio"))]
use coral_driver::linux_paths;
#[cfg(target_os = "linux")]
use coral_reef::AmdArch;
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
#[must_use]
pub fn default_nv_sm() -> u32 {
    std::env::var("CORALREEF_DEFAULT_SM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_NV_SM)
}

/// Default NVIDIA SM for the nouveau sovereign path when sysfs detection fails.
///
/// Checks `$CORALREEF_DEFAULT_SM_NOUVEAU` environment variable first,
/// falling back to SM 70 (Volta).
#[must_use]
pub fn default_nv_sm_nouveau() -> u32 {
    std::env::var("CORALREEF_DEFAULT_SM_NOUVEAU")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_NV_SM_NOUVEAU)
}

/// Map an SM version number to the closest [`NvArch`] variant for codegen.
///
/// Maxwell (SM 50–53) maps to [`NvArch::Sm35`] (closest Kepler-class slot in
/// our enum). Pascal (SM 60–62) maps to [`NvArch::Sm75`]. Hopper (SM 90) and
/// Blackwell SM100 use the nearest available variants with a warning. Any
/// still-unmapped SM falls back to [`NvArch::Sm70`] with a warning.
#[must_use]
pub fn sm_to_nvarch(sm: u32) -> NvArch {
    match sm {
        35 | 37 | 50 | 52 | 53 => NvArch::Sm35,
        60..=62 | 75..=79 => NvArch::Sm75,
        70..=74 => NvArch::Sm70,
        80..=85 => NvArch::Sm80,
        86..=88 => NvArch::Sm86,
        89 => NvArch::Sm89,
        90 => {
            tracing::warn!(
                sm,
                "Hopper SM90 has no dedicated NvArch variant; using Sm89 for codegen"
            );
            NvArch::Sm89
        }
        100 => {
            tracing::warn!(
                sm,
                "Blackwell SM100 (datacenter) approximated as NvArch::Sm120 for codegen"
            );
            NvArch::Sm120
        }
        91..=99 | 101..=119 => {
            tracing::warn!(
                sm,
                "unmapped NVIDIA SM version; using NvArch::Sm70 for codegen"
            );
            NvArch::Sm70
        }
        120 => NvArch::Sm120,
        _ => {
            tracing::warn!(
                sm,
                "unknown NVIDIA SM version; using NvArch::Sm70 for codegen"
            );
            NvArch::Sm70
        }
    }
}

/// Map `amd_arch()` / PCI-derived strings to [`AmdArch`] for the given render node.
#[cfg(target_os = "linux")]
#[must_use]
pub fn amd_arch_from_sysfs(path: &str) -> AmdArch {
    use coral_driver::nv::ioctl::probe_gpu_identity;

    let Some(id) = probe_gpu_identity(path) else {
        tracing::warn!(
            path,
            "AMD arch: probe_gpu_identity failed; defaulting to Rdna2"
        );
        return AmdArch::Rdna2;
    };

    match id.amd_arch() {
        Some("gfx9") => AmdArch::Gcn5,
        Some("rdna1") => {
            tracing::warn!("AMD RDNA1 has no dedicated AmdArch variant; using Rdna2 for codegen");
            AmdArch::Rdna2
        }
        Some("rdna2") => AmdArch::Rdna2,
        Some("rdna3") => AmdArch::Rdna3,
        Some("rdna4") => AmdArch::Rdna4,
        Some(other) => {
            let Some(parsed) = AmdArch::parse(other) else {
                tracing::warn!(
                    arch = other,
                    "unknown AMD architecture string from sysfs; defaulting to Rdna2"
                );
                return AmdArch::Rdna2;
            };
            parsed
        }
        None => {
            tracing::warn!(
                "AMD PCI identity did not match a known architecture; defaulting to Rdna2"
            );
            AmdArch::Rdna2
        }
    }
}

/// Detect the NVIDIA SM version from any available render node.
/// Falls back to the provided default if detection fails.
#[cfg(all(target_os = "linux", feature = "nvidia-drm"))]
pub fn sm_from_sysfs_or(default: u32) -> u32 {
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
pub fn sm_from_sysfs(path: &str) -> u32 {
    coral_driver::nv::ioctl::probe_gpu_identity(path)
        .and_then(|id| id.nvidia_sm())
        .unwrap_or_else(default_nv_sm_nouveau)
}

/// Detect the GPU target from sysfs for an nvidia-drm render node.
#[cfg(all(target_os = "linux", feature = "nvidia-drm"))]
pub fn sm_target_from_sysfs(path: &str) -> GpuTarget {
    let sm = coral_driver::nv::ioctl::probe_gpu_identity(path)
        .and_then(|id| id.nvidia_sm())
        .unwrap_or_else(default_nv_sm);
    GpuTarget::Nvidia(sm_to_nvarch(sm))
}

/// Map an SM version to the NVIDIA compute engine class ID (DRM/NVIF).
///
/// Delegates to [`coral_driver::nv::identity::sm_to_compute_class`] so Kepler
/// through Blackwell use the same constants as BAR0 / VFIO paths.
#[cfg(all(target_os = "linux", feature = "vfio"))]
#[must_use]
pub const fn sm_to_compute_class(sm: u32) -> u32 {
    coral_driver::nv::identity::sm_to_compute_class(sm)
}

/// Discover a VFIO-bound NVIDIA GPU by scanning sysfs for `vfio-pci` bindings.
///
/// Returns the first BDF address of an NVIDIA GPU bound to `vfio-pci`, or `None`.
#[cfg(all(target_os = "linux", feature = "vfio"))]
pub fn discover_vfio_nvidia_bdf() -> Option<String> {
    let vfio_dir_path = linux_paths::sysfs_join(&["bus", "pci", "drivers", "vfio-pci"]);
    let vfio_dir = std::path::Path::new(&vfio_dir_path);
    let entries = std::fs::read_dir(vfio_dir).ok()?;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let bdf = name.to_string_lossy();
        if !bdf.contains(':') {
            continue;
        }

        let vendor_path = linux_paths::sysfs_pci_device_file(&bdf, "vendor");
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
pub fn vfio_sm_from_device_id(device_id: Option<u16>) -> u32 {
    match device_id {
        Some(0x1003 | 0x1004 | 0x1023 | 0x1024) => 35, // GK110/GK210 (K80, K40)
        Some(0x1D81) => 70,                            // Titan V
        Some(0x1E00..=0x1E8F) => 75,                   // Turing (TU10x)
        Some(0x2200..=0x2203 | 0x2207..=0x22FF) => 80, // GA100
        Some(0x2204..=0x2206 | 0x2300..=0x23FF) => 86, // GA102/GA10x (RTX 3090/3080)
        Some(0x2400..=0x28FF) => 89,                   // Ada Lovelace (AD102-AD107)
        Some(0x2900..=0x29FF) => 120,                  // Blackwell (GB20x)
        _ => default_nv_sm(),
    }
}

/// Detect SM version for a VFIO-bound GPU from sysfs device ID.
#[cfg(all(target_os = "linux", feature = "vfio"))]
pub fn vfio_detect_sm(bdf: &str) -> u32 {
    let device_path = linux_paths::sysfs_pci_device_file(bdf, "device");
    let device_id = std::fs::read_to_string(&device_path)
        .ok()
        .and_then(|s| u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok());

    vfio_sm_from_device_id(device_id)
}
