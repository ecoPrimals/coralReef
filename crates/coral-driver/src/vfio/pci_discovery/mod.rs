// SPDX-License-Identifier: AGPL-3.0-only
//! Vendor-agnostic PCI device discovery and power management.
//!
//! Parses PCI configuration space via sysfs to enumerate BARs, capabilities,
//! power states, and link info for any PCI device. This layer is completely
//! vendor-agnostic — it works on NVIDIA, AMD, Intel, or any PCI device.
//!
//! Key operations:
//! - Config space parsing (vendor, device, class, BARs, capabilities)
//! - Power Management capability discovery and state transitions
//! - PCIe link information
//! - D3cold power cycling (PCI remove/rescan)

mod config_space;
mod device_info;
mod parse;
mod power_mgmt;
mod types;

pub use device_info::PciDeviceInfo;
pub use power_mgmt::{force_pci_d0, pci_power_cycle, set_pci_power_state, snapshot_config_space};
pub use types::{
    GpuVendor, PCI_CAP_ID_PCIE, PCI_CAP_ID_PM, PCI_STATUS_CAP_LIST, PciBar, PciCapability,
    PciPmState, PciPowerInfo, PcieLinkInfo, PcieLinkSpeed,
};

#[cfg(test)]
mod tests;
