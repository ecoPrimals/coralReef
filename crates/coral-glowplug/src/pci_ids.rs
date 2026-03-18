// SPDX-License-Identifier: AGPL-3.0-only
//! PCI vendor and device ID constants for capability-based device identification.
//!
//! Used by boot safety validation and chip identification. Keeps magic numbers
//! out of main.rs and sysfs.rs.

/// PCI vendor ID: NVIDIA Corporation.
pub(crate) const NVIDIA_VENDOR_ID: u16 = 0x10de;

/// PCI device ID: GV100 (Titan V) — consumer Volta, no GSP, requires vfio-pci at boot.
pub(crate) const TITAN_V_DEVICE_ID: u16 = 0x1d81;

/// vfio-pci.ids string for Titan V (vendor:device) — used in kernel cmdline and modprobe.
pub(crate) const TITAN_V_VFIO_IDS: &str = "10de:1d81";

/// Full kernel cmdline param for Titan V vfio-pci binding.
pub(crate) const TITAN_V_VFIO_IDS_CMDLINE: &str = "vfio-pci.ids=10de:1d81";

/// Alternate form with uppercase device ID (some kernels report this).
pub(crate) const TITAN_V_VFIO_IDS_CMDLINE_ALT: &str = "vfio-pci.ids=10de:1D81";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_vendor_id_matches_vfio_format() {
        let expected = format!("{:04x}", NVIDIA_VENDOR_ID);
        assert_eq!(expected, "10de", "NVIDIA vendor ID 0x10DE = 10de in hex");
        assert!(TITAN_V_VFIO_IDS.starts_with("10de"));
        assert!(TITAN_V_VFIO_IDS_CMDLINE.contains("10de"));
    }

    #[test]
    fn titan_v_device_id_matches_vfio_format() {
        let expected = format!("{:04x}", TITAN_V_DEVICE_ID);
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
        let ids_part = TITAN_V_VFIO_IDS_CMDLINE_ALT.strip_prefix("vfio-pci.ids=").unwrap();
        let parts: Vec<&str> = ids_part.split(':').collect();
        let vendor = u16::from_str_radix(parts[0], 16).unwrap();
        let device = u16::from_str_radix(parts[1], 16).unwrap();
        assert_eq!(vendor, NVIDIA_VENDOR_ID);
        assert_eq!(device, TITAN_V_DEVICE_ID);
    }
}
