// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

use super::parse::{
    parse_pci_bdf, parse_pci_resource_file, parse_pci_resource_line, parse_pci_sysfs_hex_id,
    parse_sysfs_pcie_speed, parse_sysfs_pcie_width, parse_sysfs_power_state, pci_class_base,
};
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
    assert!(result.unwrap_err().to_string().contains("too short"));
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

#[test]
fn from_config_bytes_subsystem_ids() {
    let mut config = make_config_bytes(0x10DE, 0x2204, 0x03_00_00);
    config[0x2C..0x2E].copy_from_slice(&0x1043u16.to_le_bytes());
    config[0x2E..0x30].copy_from_slice(&0x87EBu16.to_le_bytes());
    let info = PciDeviceInfo::from_config_bytes("0000:03:00.0", &config, Vec::new(), None)
        .expect("parse config");
    assert_eq!(info.subsystem, (0x1043, 0x87EB));
}

#[test]
fn from_config_bytes_pm_d3hot_pmcsr() {
    let mut config = make_config_bytes(0x10DE, 0x1B80, 0x03_00_00);
    config[0x34] = 0x40;
    config[0x06] = 0x10;
    config[0x40] = 0x01;
    config[0x41] = 0x00;
    config[0x42] = 0x00;
    config[0x43] = 0x00;
    // PMCSR at pm_off + 4: bits [1:0] = 3 (D3hot)
    config[0x44..0x46].copy_from_slice(&0x0003u16.to_le_bytes());
    let info =
        PciDeviceInfo::from_config_bytes("0000:01:00.0", &config, Vec::new(), None).expect("parse");
    assert_eq!(info.power.current_state, PciPmState::D3Hot);
    assert_eq!(info.power.pmcsr_raw & 0x03, 3);
}

#[test]
fn from_config_bytes_chained_pm_then_pcie() {
    let mut config = make_config_bytes(0x10DE, 0x1B80, 0x03_00_00);
    config[0x34] = 0x40;
    config[0x06] = 0x10;
    // PM at 0x40, next cap at 0x50
    config[0x40] = 0x01;
    config[0x41] = 0x50;
    config[0x44..0x46].copy_from_slice(&0u16.to_le_bytes());
    // PCIe at 0x50
    config[0x50] = 0x10;
    config[0x51] = 0x00;
    let link_cap_off = 0x50 + 0x0C;
    let link_sta_off = 0x50 + 0x12;
    config[link_cap_off..link_cap_off + 4].copy_from_slice(&0x3u32.to_le_bytes());
    config[link_sta_off..link_sta_off + 2].copy_from_slice(&0x3u16.to_le_bytes());
    let info = PciDeviceInfo::from_config_bytes("0000:01:00.0", &config, Vec::new(), None)
        .expect("parse chained caps");
    assert!(
        info.capabilities.iter().any(|c| c.id == PCI_CAP_ID_PM),
        "expected PM cap"
    );
    assert!(
        info.capabilities.iter().any(|c| c.id == PCI_CAP_ID_PCIE),
        "expected PCIe cap"
    );
    let link = info.pcie_link.expect("link");
    assert!(matches!(link.max_speed, PcieLinkSpeed::Gen3));
    assert!(matches!(link.current_speed, PcieLinkSpeed::Gen3));
}

#[test]
fn parse_pci_bdf_rejects_extra_colons() {
    assert_eq!(parse_pci_bdf("0000:01:00.0:extra"), None);
}

#[test]
fn parse_pci_bdf_accepts_full_domain() {
    assert_eq!(parse_pci_bdf("FFFF:FF:1F.7"), Some((0xFFFF, 0xFF, 0x1F, 7)));
}

#[test]
fn parse_pci_resource_line_zero_start_end_skipped() {
    assert!(parse_pci_resource_line("0x0 0x0 0x0", 0).is_none());
}

#[test]
fn pcie_link_speed_unknown_encoding() {
    assert!(matches!(
        PcieLinkSpeed::from_encoding(0x0F),
        PcieLinkSpeed::Unknown(0x0F)
    ));
}

#[test]
fn pci_pm_state_from_pmcsr_bits_unknown_path() {
    // `from_pmcsr_bits` is private; exercise via display of Unknown from sysfs edge cases
    let u = PciPmState::Unknown(0xAB);
    assert_eq!(u.to_string(), "Unknown(0xab)");
}
