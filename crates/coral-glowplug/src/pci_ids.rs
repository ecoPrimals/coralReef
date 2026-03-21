// SPDX-License-Identifier: AGPL-3.0-only
//! PCI vendor and device ID constants for capability-based device identification.
//!
//! Used by boot safety validation and chip identification. Keeps magic numbers
//! out of main.rs and sysfs.rs.

/// PCI vendor ID: NVIDIA Corporation.
pub const NVIDIA_VENDOR_ID: u16 = 0x10de;

/// PCI device ID: GV100 (Titan V) — consumer Volta, no GSP, requires vfio-pci at boot.
pub const TITAN_V_DEVICE_ID: u16 = 0x1d81;

/// vfio-pci.ids string for Titan V (vendor:device) — used in kernel cmdline and modprobe.
pub const TITAN_V_VFIO_IDS: &str = "10de:1d81";

/// Full kernel cmdline param for Titan V vfio-pci binding.
pub const TITAN_V_VFIO_IDS_CMDLINE: &str = "vfio-pci.ids=10de:1d81";

/// Alternate form with uppercase device ID (some kernels report this).
pub const TITAN_V_VFIO_IDS_CMDLINE_ALT: &str = "vfio-pci.ids=10de:1D81";

// ---- AMD ----

/// PCI vendor ID: Advanced Micro Devices, Inc.
pub const AMD_VENDOR_ID: u16 = 0x1002;

/// PCI device ID: Vega 20 (Radeon Instinct MI50 — 16 GB HBM2).
pub const MI50_DEVICE_ID: u16 = 0x66a0;

/// PCI device ID: Vega 20 (Radeon Instinct MI60 — 32 GB HBM2).
pub const MI60_DEVICE_ID: u16 = 0x66a1;

/// PCI device ID: Vega 20 (Radeon VII — 16 GB HBM2, consumer variant of MI50).
pub const RADEON_VII_DEVICE_ID: u16 = 0x66af;

/// vfio-pci.ids string for MI50 (vendor:device).
pub const MI50_VFIO_IDS: &str = "1002:66a0";

/// vfio-pci.ids string for MI60 (vendor:device).
pub const MI60_VFIO_IDS: &str = "1002:66a1";

/// vfio-pci.ids string for Radeon VII (vendor:device).
pub const RADEON_VII_VFIO_IDS: &str = "1002:66af";

/// Returns true if the device ID is any Vega 20 variant (MI50, MI60, Radeon VII).
#[must_use]
pub const fn is_vega20(device_id: u16) -> bool {
    matches!(
        device_id,
        MI50_DEVICE_ID | MI60_DEVICE_ID | RADEON_VII_DEVICE_ID
    )
}

// ---- Intel ----

/// PCI vendor ID: Intel Corporation.
pub const INTEL_VENDOR_ID: u16 = 0x8086;

// ---- BrainChip ----

/// PCI vendor ID: BrainChip Inc.
pub const BRAINCHIP_VENDOR_ID: u16 = 0x1e7c;

/// PCI device ID: AKD1000 Neural Network Coprocessor (Akida).
pub const AKD1000_DEVICE_ID: u16 = 0xbca1;

// ---- Helpers ----

/// Returns the HBM2-training driver name for a given PCI vendor.
///
/// NVIDIA GPUs use `nouveau`; AMD GPUs use `amdgpu`.
/// Returns `None` for unknown vendors.
#[must_use]
pub const fn hbm2_training_driver(vendor_id: u16) -> Option<&'static str> {
    match vendor_id {
        NVIDIA_VENDOR_ID => Some("nouveau"),
        AMD_VENDOR_ID => Some("amdgpu"),
        INTEL_VENDOR_ID => Some("xe"),
        _ => None,
    }
}

/// Returns the native compute driver name for a given PCI vendor.
pub const fn native_compute_driver(vendor_id: u16) -> Option<&'static str> {
    match vendor_id {
        NVIDIA_VENDOR_ID => Some("nouveau"),
        AMD_VENDOR_ID => Some("amdgpu"),
        INTEL_VENDOR_ID => Some("xe"),
        BRAINCHIP_VENDOR_ID => Some("akida-pcie"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_vendor_id_matches_vfio_format() {
        let expected = format!("{NVIDIA_VENDOR_ID:04x}");
        assert_eq!(expected, "10de", "NVIDIA vendor ID 0x10DE = 10de in hex");
        assert!(TITAN_V_VFIO_IDS.starts_with("10de"));
        assert!(TITAN_V_VFIO_IDS_CMDLINE.contains("10de"));
    }

    #[test]
    fn titan_v_device_id_matches_vfio_format() {
        let expected = format!("{TITAN_V_DEVICE_ID:04x}");
        assert_eq!(expected, "1d81", "Titan V device ID 0x1D81 = 1d81 in hex");
        assert!(TITAN_V_VFIO_IDS.ends_with("1d81"));
    }

    #[test]
    fn titan_v_vfio_ids_format_vendor_device() {
        let parts: Vec<&str> = TITAN_V_VFIO_IDS.split(':').collect();
        assert_eq!(parts.len(), 2, "vfio.ids format is vendor:device");
        let vendor = u16::from_str_radix(parts[0], 16).unwrap();
        let device = u16::from_str_radix(parts[1], 16).unwrap();
        assert_eq!(vendor, NVIDIA_VENDOR_ID);
        assert_eq!(device, TITAN_V_DEVICE_ID);
    }

    #[test]
    fn titan_v_vfio_ids_cmdline_alt_parses_same_values() {
        let ids_part = TITAN_V_VFIO_IDS_CMDLINE_ALT
            .strip_prefix("vfio-pci.ids=")
            .unwrap();
        let parts: Vec<&str> = ids_part.split(':').collect();
        let vendor = u16::from_str_radix(parts[0], 16).unwrap();
        let device = u16::from_str_radix(parts[1], 16).unwrap();
        assert_eq!(vendor, NVIDIA_VENDOR_ID);
        assert_eq!(device, TITAN_V_DEVICE_ID);
    }

    #[test]
    fn amd_vendor_id_hex() {
        assert_eq!(format!("{AMD_VENDOR_ID:04x}"), "1002");
    }

    #[test]
    fn mi50_vfio_ids_format() {
        let parts: Vec<&str> = MI50_VFIO_IDS.split(':').collect();
        assert_eq!(parts.len(), 2);
        let vendor = u16::from_str_radix(parts[0], 16).unwrap();
        let device = u16::from_str_radix(parts[1], 16).unwrap();
        assert_eq!(vendor, AMD_VENDOR_ID);
        assert_eq!(device, MI50_DEVICE_ID);
    }

    #[test]
    fn mi60_vfio_ids_format() {
        let parts: Vec<&str> = MI60_VFIO_IDS.split(':').collect();
        assert_eq!(parts.len(), 2);
        let vendor = u16::from_str_radix(parts[0], 16).unwrap();
        let device = u16::from_str_radix(parts[1], 16).unwrap();
        assert_eq!(vendor, AMD_VENDOR_ID);
        assert_eq!(device, MI60_DEVICE_ID);
    }

    #[test]
    fn hbm2_training_driver_nvidia() {
        assert_eq!(hbm2_training_driver(NVIDIA_VENDOR_ID), Some("nouveau"));
    }

    #[test]
    fn hbm2_training_driver_amd() {
        assert_eq!(hbm2_training_driver(AMD_VENDOR_ID), Some("amdgpu"));
    }

    #[test]
    fn hbm2_training_driver_intel() {
        assert_eq!(hbm2_training_driver(INTEL_VENDOR_ID), Some("xe"));
    }

    #[test]
    fn hbm2_training_driver_unknown() {
        assert_eq!(hbm2_training_driver(0xdead), None);
    }

    #[test]
    fn is_vega20_covers_all_mi_and_radeon_vii() {
        assert!(is_vega20(MI50_DEVICE_ID));
        assert!(is_vega20(MI60_DEVICE_ID));
        assert!(is_vega20(RADEON_VII_DEVICE_ID));
        assert!(!is_vega20(0x1234));
    }

    #[test]
    fn native_compute_driver_brainchip() {
        assert_eq!(
            native_compute_driver(BRAINCHIP_VENDOR_ID),
            Some("akida-pcie")
        );
    }

    #[test]
    fn native_compute_driver_unknown_vendor() {
        assert_eq!(native_compute_driver(0xffff), None);
    }

    #[test]
    fn native_compute_driver_nvidia_amd_intel() {
        assert_eq!(native_compute_driver(NVIDIA_VENDOR_ID), Some("nouveau"));
        assert_eq!(native_compute_driver(AMD_VENDOR_ID), Some("amdgpu"));
        assert_eq!(native_compute_driver(INTEL_VENDOR_ID), Some("xe"));
    }

    #[test]
    fn radeon_vii_vfio_ids_format() {
        let parts: Vec<&str> = RADEON_VII_VFIO_IDS.split(':').collect();
        assert_eq!(parts.len(), 2);
        let vendor = u16::from_str_radix(parts[0], 16).unwrap();
        let device = u16::from_str_radix(parts[1], 16).unwrap();
        assert_eq!(vendor, AMD_VENDOR_ID);
        assert_eq!(device, RADEON_VII_DEVICE_ID);
    }
}
