// SPDX-License-Identifier: AGPL-3.0-only
//! [`PciDeviceInfo`] — aggregate view from config space and sysfs.

use std::fmt::Write as FmtWrite;

use crate::linux_paths;

use super::config_space::{
    parse_pci_power_info_from_config, parse_pcie_link_from_config, pci_config_read_u16,
    pci_config_read_u32, pci_power_info_without_pm_capability, walk_pci_capability_chain,
};
use super::parse::{
    parse_pci_bdf, parse_pci_resource_file, parse_pci_sysfs_hex_id, parse_sysfs_pcie_speed,
    parse_sysfs_pcie_width, parse_sysfs_power_state, pci_class_base,
};
use super::types::{
    GpuVendor, PCI_CAP_ID_PCIE, PCI_CAP_ID_PM, PCI_STATUS_CAP_LIST, PciBar, PciCapability,
    PciPmState, PciPowerInfo, PcieLinkInfo,
};

fn read_sysfs_pci_hex_id(bdf: &str, name: &str) -> Option<u16> {
    let path = linux_paths::sysfs_pci_device_file(bdf, name);
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| parse_pci_sysfs_hex_id(&s))
}

/// Complete PCI device information parsed from sysfs config space.
#[derive(Debug, Clone)]
pub struct PciDeviceInfo {
    /// PCI Bus:Device.Function address (e.g., "0000:4a:00.0").
    pub bdf: String,
    /// PCI vendor ID.
    pub vendor_id: u16,
    /// PCI device ID.
    pub device_id: u16,
    /// PCI class code (24-bit: class << 16 | subclass << 8 | prog_if).
    pub class_code: u32,
    /// Subsystem vendor and device IDs.
    pub subsystem: (u16, u16),
    /// Base Address Registers.
    pub bars: Vec<PciBar>,
    /// PCI capabilities from the capability chain.
    pub capabilities: Vec<PciCapability>,
    /// Power management info.
    pub power: PciPowerInfo,
    /// PCIe link info (if PCIe capability present).
    pub pcie_link: Option<PcieLinkInfo>,
    /// GPU vendor classification.
    pub vendor: GpuVendor,
}

impl PciDeviceInfo {
    /// Parse PCI device info from raw config space bytes (for testing without sysfs).
    ///
    /// `config` must be at least 64 bytes. `bars` and `pcie_link` are passed through
    /// (from sysfs in production; use empty/None for unit tests).
    ///
    /// # Errors
    ///
    /// Returns an error if config is too short.
    pub fn from_config_bytes(
        bdf: &str,
        config: &[u8],
        bars: Vec<PciBar>,
        pcie_link: Option<PcieLinkInfo>,
    ) -> Result<Self, String> {
        if config.len() < 64 {
            return Err(format!("PCI config too short: {} bytes", config.len()));
        }

        let vendor_id = pci_config_read_u16(config, 0x00);
        let device_id = pci_config_read_u16(config, 0x02);
        let class_code = pci_config_read_u32(config, 0x08) >> 8;
        let subsystem = (
            pci_config_read_u16(config, 0x2C),
            pci_config_read_u16(config, 0x2E),
        );

        let (capabilities, pm_cap_offset, pcie_cap_offset) = walk_pci_capability_chain(config);

        let power = if let Some(pm_off) = pm_cap_offset {
            parse_pci_power_info_from_config(config, pm_off)
        } else {
            pci_power_info_without_pm_capability()
        };

        let pcie_link_parsed =
            pcie_cap_offset.and_then(|off| parse_pcie_link_from_config(config, off));
        let pcie_link_final = pcie_link_parsed.or(pcie_link);

        Ok(Self {
            bdf: bdf.to_string(),
            vendor_id,
            device_id,
            class_code,
            subsystem,
            bars,
            capabilities,
            power,
            pcie_link: pcie_link_final,
            vendor: GpuVendor::from_vendor_id(vendor_id),
        })
    }

    /// Parse PCI device info from sysfs config space.
    pub fn from_sysfs(bdf: &str) -> Result<Self, String> {
        parse_pci_bdf(bdf).ok_or_else(|| format!("invalid PCI BDF: {bdf}"))?;

        let config_path = linux_paths::sysfs_pci_device_file(bdf, "config");
        let config = std::fs::read(&config_path).map_err(|e| format!("read {config_path}: {e}"))?;

        if config.len() < 64 {
            return Err(format!("PCI config too short: {} bytes", config.len()));
        }

        let vendor_id = pci_config_read_u16(&config, 0x00);
        let device_id = pci_config_read_u16(&config, 0x02);
        if let Some(sys_vendor) = read_sysfs_pci_hex_id(bdf, "vendor")
            && sys_vendor != vendor_id
        {
            tracing::warn!(
                bdf,
                config_vendor = vendor_id,
                sysfs_vendor = sys_vendor,
                "PCI vendor ID mismatch (config vs sysfs)"
            );
        }
        let class_code = pci_config_read_u32(&config, 0x08) >> 8;
        let subsystem = (
            pci_config_read_u16(&config, 0x2C),
            pci_config_read_u16(&config, 0x2E),
        );

        // Parse BARs from sysfs resource file (more reliable than config space)
        let bars = Self::parse_bars_sysfs(bdf);

        // Walk capability chain.
        // vfio-pci may only expose 64 bytes of config space via sysfs,
        // so the capability pointer (0x34) may reference offsets beyond
        // what we have. We try the full config first, then fall back to
        // inferring common capabilities from the vendor ID.
        let status = pci_config_read_u16(&config, 0x06);
        let has_cap_list = status & PCI_STATUS_CAP_LIST != 0;
        let (mut capabilities, pm_cap_offset, pcie_cap_offset) = walk_pci_capability_chain(&config);

        // If vfio-pci truncated config space, populate from sysfs attributes
        if capabilities.is_empty() && has_cap_list {
            let dev_path = linux_paths::sysfs_pci_device_path(bdf);
            // PCIe link info from sysfs as evidence of PCIe capability
            if std::path::Path::new(&format!("{dev_path}/current_link_speed")).exists() {
                capabilities.push(PciCapability {
                    id: PCI_CAP_ID_PCIE,
                    offset: 0,
                    name: "PCI Express (from sysfs)",
                });
            }
            // Power state from sysfs as evidence of PM capability
            if std::path::Path::new(&format!("{dev_path}/power_state")).exists() {
                capabilities.push(PciCapability {
                    id: PCI_CAP_ID_PM,
                    offset: 0,
                    name: "Power Management (from sysfs)",
                });
            }
        }

        // Parse PM capability — try config space first, then sysfs fallback
        let power = if let Some(pm_off) = pm_cap_offset {
            parse_pci_power_info_from_config(&config, pm_off)
        } else {
            // sysfs fallback: read power_state if config space was truncated
            let dev_path = linux_paths::sysfs_pci_device_path(bdf);
            let sysfs_state = std::fs::read_to_string(format!("{dev_path}/power_state"))
                .ok()
                .and_then(|s| parse_sysfs_power_state(&s));
            PciPowerInfo {
                pm_cap_offset: None,
                current_state: sysfs_state.unwrap_or(PciPmState::Unknown(0xFF)),
                d1_support: false,
                d2_support: false,
                pme_support: 0,
                pmcsr_raw: 0,
            }
        };

        // Parse PCIe link info — config space or sysfs fallback
        let pcie_link = pcie_cap_offset
            .and_then(|off| parse_pcie_link_from_config(&config, off))
            .or_else(|| Self::parse_link_sysfs(bdf));

        Ok(Self {
            bdf: bdf.to_string(),
            vendor_id,
            device_id,
            class_code,
            subsystem,
            bars,
            capabilities,
            power,
            pcie_link,
            vendor: GpuVendor::from_vendor_id(vendor_id),
        })
    }

    fn parse_link_sysfs(bdf: &str) -> Option<PcieLinkInfo> {
        let dev = linux_paths::sysfs_pci_device_path(bdf);
        let cur_speed = std::fs::read_to_string(format!("{dev}/current_link_speed")).ok()?;
        let max_speed =
            std::fs::read_to_string(format!("{dev}/max_link_speed")).unwrap_or_default();
        let cur_width =
            std::fs::read_to_string(format!("{dev}/current_link_width")).unwrap_or_default();
        let max_width =
            std::fs::read_to_string(format!("{dev}/max_link_width")).unwrap_or_default();
        Some(PcieLinkInfo {
            current_speed: parse_sysfs_pcie_speed(&cur_speed),
            max_speed: parse_sysfs_pcie_speed(&max_speed),
            current_width: parse_sysfs_pcie_width(&cur_width),
            max_width: parse_sysfs_pcie_width(&max_width),
        })
    }

    fn parse_bars_sysfs(bdf: &str) -> Vec<PciBar> {
        let resource_path = linux_paths::sysfs_pci_device_file(bdf, "resource");
        let content = match std::fs::read_to_string(&resource_path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        parse_pci_resource_file(&content)
    }

    /// Print a human-readable summary of the device.
    pub fn print_summary(&self) {
        let mut s = String::new();
        writeln!(
            &mut s,
            "╠══ PCI DEVICE INFO ═════════════════════════════════════════╣"
        )
        .expect("writing to String is infallible");
        writeln!(&mut s, "║ BDF:     {}", self.bdf).expect("writing to String is infallible");
        writeln!(
            &mut s,
            "║ ID:      {:04x}:{:04x} ({})",
            self.vendor_id, self.device_id, self.vendor
        )
        .expect("writing to String is infallible");
        writeln!(
            &mut s,
            "║ Class:   {:#08x} (base {:#04x})",
            self.class_code,
            pci_class_base(self.class_code)
        )
        .expect("writing to String is infallible");
        writeln!(
            &mut s,
            "║ Power:   {} (PMCSR={:#06x})",
            self.power.current_state, self.power.pmcsr_raw
        )
        .expect("writing to String is infallible");
        for bar in &self.bars {
            writeln!(
                &mut s,
                "║ BAR{}:    {:#014x} ({} KB) {}{}{}",
                bar.index,
                bar.base,
                bar.size / 1024,
                if bar.is_mmio { "MMIO" } else { "IO" },
                if bar.is_64bit { " 64bit" } else { "" },
                if bar.is_prefetchable { " prefetch" } else { "" },
            )
            .expect("writing to String is infallible");
        }
        for cap in &self.capabilities {
            writeln!(
                &mut s,
                "║ Cap:     [{:#04x}] {} @ {:#04x}",
                cap.id, cap.name, cap.offset
            )
            .expect("writing to String is infallible");
        }
        if let Some(ref link) = self.pcie_link {
            writeln!(
                &mut s,
                "║ PCIe:    x{} @ {} (max x{} @ {})",
                link.current_width, link.current_speed, link.max_width, link.max_speed,
            )
            .expect("writing to String is infallible");
        }
        tracing::info!(summary = %s, "PCI device info");
    }
}
