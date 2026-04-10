// SPDX-License-Identifier: AGPL-3.0-or-later
//! Raw PCI configuration space reads and capability-chain walks.

use std::collections::HashSet;

use crate::PciDiscoveryError;

use super::types::{
    PCI_CAP_ID_PCIE, PCI_CAP_ID_PM, PCI_STATUS_CAP_LIST, PciCapability, PciPmState, PciPowerInfo,
    PcieLinkInfo, PcieLinkSpeed,
};

pub(super) fn pci_config_read_u16(config: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([config[off], config[off + 1]])
}

pub(super) fn pci_config_read_u32(config: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([
        config[off],
        config[off + 1],
        config[off + 2],
        config[off + 3],
    ])
}

/// Walk the PCI capability list in config space.
///
/// Returns discovered capabilities plus the first PM and PCIe capability offsets.
pub(super) fn walk_pci_capability_chain(
    config: &[u8],
) -> (Vec<PciCapability>, Option<u8>, Option<u8>) {
    let mut capabilities = Vec::new();
    let mut pm_cap_offset = None;
    let mut pcie_cap_offset = None;

    let status = pci_config_read_u16(config, 0x06);
    let has_cap_list = status & PCI_STATUS_CAP_LIST != 0;
    if !has_cap_list || config.len() < 0x40 {
        return (capabilities, pm_cap_offset, pcie_cap_offset);
    }

    let mut cap_ptr = (config[0x34] & 0xFC) as usize;
    let mut visited = HashSet::new();
    while cap_ptr != 0 && !visited.contains(&cap_ptr) && cap_ptr + 2 <= config.len() {
        visited.insert(cap_ptr);
        let cap_id = config[cap_ptr];
        let name = PciCapability::name_for_id(cap_id);

        capabilities.push(PciCapability {
            id: cap_id,
            offset: cap_ptr as u8,
            name,
        });

        if cap_id == PCI_CAP_ID_PM {
            pm_cap_offset = Some(cap_ptr as u8);
        }
        if cap_id == PCI_CAP_ID_PCIE {
            pcie_cap_offset = Some(cap_ptr as u8);
        }

        cap_ptr = (config[cap_ptr + 1] & 0xFC) as usize;
    }

    (capabilities, pm_cap_offset, pcie_cap_offset)
}

pub(super) fn pci_power_info_without_pm_capability() -> PciPowerInfo {
    PciPowerInfo {
        pm_cap_offset: None,
        current_state: PciPmState::Unknown(0xFF),
        d1_support: false,
        d2_support: false,
        pme_support: 0,
        pmcsr_raw: 0,
    }
}

pub(super) fn parse_pci_power_info_from_config(config: &[u8], pm_cap_offset: u8) -> PciPowerInfo {
    let pm_off = pm_cap_offset as usize;
    if pm_off + 6 <= config.len() {
        let pmc = pci_config_read_u16(config, pm_off + 2);
        let pmcsr = pci_config_read_u16(config, pm_off + 4);
        PciPowerInfo {
            pm_cap_offset: Some(pm_cap_offset),
            current_state: PciPmState::from_pmcsr_bits((pmcsr & 0x03) as u8),
            d1_support: pmc & (1 << 9) != 0,
            d2_support: pmc & (1 << 10) != 0,
            pme_support: ((pmc >> 11) & 0x1F) as u8,
            pmcsr_raw: pmcsr,
        }
    } else {
        PciPowerInfo {
            pm_cap_offset: Some(pm_cap_offset),
            current_state: PciPmState::Unknown(0xFF),
            d1_support: false,
            d2_support: false,
            pme_support: 0,
            pmcsr_raw: 0,
        }
    }
}

pub(super) fn parse_pcie_link_from_config(
    config: &[u8],
    pcie_cap_offset: u8,
) -> Option<PcieLinkInfo> {
    let off = pcie_cap_offset as usize;
    if off + 0x14 > config.len() {
        return None;
    }
    let link_cap = pci_config_read_u32(config, off + 0x0C);
    let link_sta = pci_config_read_u16(config, off + 0x12);
    Some(PcieLinkInfo {
        max_speed: PcieLinkSpeed::from_encoding((link_cap & 0x0F) as u8),
        current_speed: PcieLinkSpeed::from_encoding((link_sta & 0x0F) as u8),
        max_width: ((link_cap >> 4) & 0x3F) as u8,
        current_width: ((link_sta >> 4) & 0x3F) as u8,
    })
}

/// Locate the PM capability offset in config space (first PM cap in the chain).
pub fn find_pm_capability_offset(config: &[u8]) -> Result<usize, PciDiscoveryError> {
    const MIN_HEADER: usize = 0x40;
    if config.len() < MIN_HEADER {
        return Err(PciDiscoveryError::ConfigTooShort {
            len: config.len(),
            need: MIN_HEADER,
        });
    }

    let status = pci_config_read_u16(config, 0x06);
    if status & PCI_STATUS_CAP_LIST == 0 {
        return Err(PciDiscoveryError::NoPciCapabilitiesList);
    }

    let mut cap_ptr = (config[0x34] & 0xFC) as usize;
    let mut visited = HashSet::new();
    while cap_ptr != 0 && !visited.contains(&cap_ptr) && cap_ptr + 2 <= config.len() {
        visited.insert(cap_ptr);
        if config[cap_ptr] == PCI_CAP_ID_PM {
            return Ok(cap_ptr);
        }
        cap_ptr = (config[cap_ptr + 1] & 0xFC) as usize;
    }

    Err(PciDiscoveryError::PmCapabilityNotFound)
}
