// SPDX-License-Identifier: AGPL-3.0-or-later
//! PCI power management helpers for devinit.
//!
//! Delegates to `pci_discovery` for actual implementation.

/// Force a PCI device from D3hot back to D0 by writing to the PM capability.
///
/// Delegates to [`crate::vfio::pci_discovery::force_pci_d0`]. Kept here
/// for backward compatibility with existing call sites in glowplug.rs
/// and vfio_compute.rs.
pub fn force_pci_d0(bdf: &str) -> Result<(), crate::PciDiscoveryError> {
    crate::vfio::pci_discovery::force_pci_d0(bdf)
}

/// Trigger a PCI D3cold → D0 power cycle via sysfs.
///
/// Delegates to [`crate::vfio::pci_discovery::pci_power_cycle`]. Kept
/// here for backward compatibility.
pub fn pci_power_cycle_devinit(bdf: &str) -> Result<bool, crate::PciDiscoveryError> {
    crate::vfio::pci_discovery::pci_power_cycle(bdf)
}
