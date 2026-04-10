// SPDX-License-Identifier: AGPL-3.0-or-later
//! PM capability offset discovery — delegates to [`crate::vfio::pci_discovery::config_space`].
//!
//! This module keeps a stable `vfio::pci_config` path aligned with the PCI discovery implementation.

use crate::PciDiscoveryError;

/// Locate the PM capability offset in config space (first PM cap in the chain).
#[expect(
    dead_code,
    reason = "Stable `vfio::pci_config` shim; callers use `pci_discovery::config_space` directly."
)]
pub(crate) fn find_pm_capability_offset(config: &[u8]) -> Result<usize, PciDiscoveryError> {
    super::pci_discovery::find_pm_capability_offset(config)
}
