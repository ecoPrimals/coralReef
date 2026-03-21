// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

use super::*;

fn make_config_bytes(vendor_id: u16, device_id: u16, class_code: u32) -> Vec<u8> {
    let mut config = vec![0u8; 256];
    config[0..2].copy_from_slice(&vendor_id.to_le_bytes());
    config[2..4].copy_from_slice(&device_id.to_le_bytes());
    config[8..12].copy_from_slice(&(class_code << 8).to_le_bytes());
    config[0x34] = 0x40; // cap ptr at 0x40
    config[0x06] = 0x10; // capability list present
    config
}

#[test]
fn from_config_bytes_minimal() {
    let config = make_config_bytes(0x10DE, 0x1B80, 0x03_00_00); // NVIDIA, class VGA
    let info = PciDeviceInfo::from_config_bytes("0000:01:00.0", &config, Vec::new(), None)
        .expect("parse minimal config");
    assert_eq!(info.vendor_id, 0x10DE);
    assert_eq!(info.device_id, 0x1B80);
    assert_eq!(info.bdf, "0000:01:00.0");
    assert!(matches!(info.vendor, GpuVendor::Nvidia));
}

#[test]
fn from_config_bytes_amd_vendor() {
    let config = make_config_bytes(0x1002, 0x73DF, 0x03_00_00); // AMD
    let info = PciDeviceInfo::from_config_bytes("0000:4a:00.0", &config, Vec::new(), None)
        .expect("parse AMD config");
    assert_eq!(info.vendor_id, 0x1002);
    assert!(matches!(info.vendor, GpuVendor::Amd));
}

#[test]
fn from_config_bytes_too_short() {
    let config = [0u8; 32];
    let result = PciDeviceInfo::from_config_bytes("0000:00:00.0", &config, Vec::new(), None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("too short"));
}

#[test]
fn gpu_vendor_from_id() {
    assert!(matches!(
        GpuVendor::from_vendor_id(0x10DE),
        GpuVendor::Nvidia
    ));
    assert!(matches!(GpuVendor::from_vendor_id(0x1002), GpuVendor::Amd));
    assert!(matches!(
        GpuVendor::from_vendor_id(0x8086),
        GpuVendor::Intel
    ));
    let unknown = GpuVendor::from_vendor_id(0x1234);
    assert!(matches!(unknown, GpuVendor::Unknown(0x1234)));
}

#[test]
fn gpu_vendor_display() {
    assert_eq!(GpuVendor::Nvidia.to_string(), "NVIDIA");
    assert_eq!(GpuVendor::Amd.to_string(), "AMD");
    assert_eq!(GpuVendor::Intel.to_string(), "Intel");
}

#[test]
fn pci_pm_state_pmcsr_bits() {
    assert_eq!(PciPmState::D0.pmcsr_bits(), 0);
    assert_eq!(PciPmState::D1.pmcsr_bits(), 1);
    assert_eq!(PciPmState::D2.pmcsr_bits(), 2);
    assert_eq!(PciPmState::D3Hot.pmcsr_bits(), 3);
}

#[test]
fn pci_pm_state_display() {
    assert_eq!(PciPmState::D0.to_string(), "D0");
    assert_eq!(PciPmState::D3Hot.to_string(), "D3hot");
    assert_eq!(PciPmState::D3Cold.to_string(), "D3cold");
}

#[test]
fn pci_bar_construction() {
    let bar = PciBar {
        index: 0,
        base: 0xF000_0000,
        size: 0x10_0000,
        is_mmio: true,
        is_64bit: true,
        is_prefetchable: true,
    };
    assert_eq!(bar.base, 0xF000_0000);
    assert!(bar.is_mmio);
}

#[test]
fn pci_capability_construction() {
    let cap = PciCapability {
        id: 0x10,
        offset: 0x40,
        name: "PCI Express",
    };
    assert_eq!(cap.id, 0x10);
    assert_eq!(cap.offset, 0x40);
}

#[test]
fn from_config_bytes_with_pm_capability() {
    let mut config = make_config_bytes(0x10DE, 0x1B80, 0x03_00_00);
    config[0x34] = 0x40;
    config[0x40] = 0x01; // PM capability
    config[0x41] = 0x00; // next cap
    config[0x44] = 0x00; // PMCSR low byte
    config[0x45] = 0x00; // PMCSR high byte (state D0)
    let info = PciDeviceInfo::from_config_bytes("0000:01:00.0", &config, Vec::new(), None)
        .expect("parse config with PM");
    assert_eq!(info.power.current_state, PciPmState::D0);
}

#[test]
fn from_config_bytes_with_pcie_link() {
    let mut config = make_config_bytes(0x10DE, 0x1B80, 0x03_00_00);
    config[0x34] = 0x40;
    config[0x40] = 0x10; // PCIe capability
    config[0x41] = 0x00;
    // link_cap at 0x4C: bits [3:0]=max_speed (4=Gen4), [9:4]=max_width (16=x16)
    config[0x4C..0x50].copy_from_slice(&0x104u32.to_le_bytes());
    // link_sta at 0x52: bits [3:0]=current_speed (3=Gen3), [9:4]=current_width
    config[0x52..0x54].copy_from_slice(&0x103u16.to_le_bytes());
    let info = PciDeviceInfo::from_config_bytes("0000:01:00.0", &config, Vec::new(), None)
        .expect("parse config with PCIe");
    let link = info.pcie_link.expect("PCIe link");
    assert!(matches!(link.max_speed, PcieLinkSpeed::Gen4));
    assert!(matches!(link.current_speed, PcieLinkSpeed::Gen3));
}

#[test]
fn pcie_link_speed_display() {
    assert!(PcieLinkSpeed::Gen1.to_string().contains("2.5"));
    assert!(PcieLinkSpeed::Gen4.to_string().contains("16"));
}

#[test]
fn parse_pci_bdf_roundtrip() {
    assert_eq!(parse_pci_bdf("0000:4a:00.0"), Some((0, 0x4a, 0, 0)));
    assert_eq!(parse_pci_bdf("bad"), None);
}

#[test]
fn pci_class_base_display_controller() {
    assert_eq!(pci_class_base(0x03_00_00), 0x03);
}

#[test]
fn parse_pci_sysfs_hex_id_variants() {
    assert_eq!(parse_pci_sysfs_hex_id("0x10de\n"), Some(0x10DE));
    assert_eq!(parse_pci_sysfs_hex_id("1002"), Some(0x1002));
}

#[test]
fn parse_pci_resource_file_two_bars() {
    let content = "0x00000000f0000000 0x00000000f01fffff 0x000000000014220c\n\
                   0x0000000000000000 0x0000000000000000 0x0000000000000000\n";
    let bars = parse_pci_resource_file(content);
    assert_eq!(bars.len(), 1);
    assert_eq!(bars[0].index, 0);
    assert_eq!(bars[0].base, 0xF000_0000);
    assert!(bars[0].is_mmio);
}

#[test]
fn parse_sysfs_pcie_speed_and_width() {
    assert!(matches!(
        parse_sysfs_pcie_speed("16.0 GT/s"),
        PcieLinkSpeed::Gen4
    ));
    assert_eq!(parse_sysfs_pcie_width("x8\n"), 8);
}

#[test]
fn parse_sysfs_power_state_trimmed() {
    assert_eq!(parse_sysfs_power_state(" D0 \n"), Some(PciPmState::D0));
}

#[test]
fn gpu_vendor_matches_vendor_id() {
    assert!(GpuVendor::Nvidia.matches_vendor_id(0x10DE));
    assert!(!GpuVendor::Unknown(0x10DE).matches_vendor_id(0x10DE));
}
