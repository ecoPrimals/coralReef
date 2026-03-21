// SPDX-License-Identifier: AGPL-3.0-only
#![expect(
    missing_docs,
    reason = "DeviceSlot and submodule re-exports are described in this file's module docs; per-field rustdoc deferred."
)]
//! `DeviceSlot` — persistent ownership of a `PCIe` device.
//!
//! Each slot manages one GPU/accelerator from boot to shutdown.
//! It tracks the current driver personality, power state, VRAM
//! health, and provides the VFIO fd for ecosystem consumers.

pub mod activate;
pub mod health;
pub mod swap;
pub mod types;

pub use types::*;

use crate::config::DeviceConfig;
use crate::personality::Personality;
use crate::sysfs;
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct DeviceSlot {
    pub config: DeviceConfig,
    pub bdf: Arc<str>,
    pub personality: Personality,
    pub health: DeviceHealth,
    pub vendor_id: u16,
    pub device_id: u16,
    pub chip_name: String,
    vfio_holder: Option<types::VfioHolder>,
    register_snapshot: BTreeMap<usize, u32>,
}

impl DeviceSlot {
    pub fn new(config: DeviceConfig) -> Self {
        let bdf: Arc<str> = Arc::from(config.bdf.as_str());
        let (vendor_id, device_id) = sysfs::read_pci_ids(&bdf);
        let chip_name = sysfs::identify_chip(vendor_id, device_id);

        Self {
            config,
            bdf,
            personality: Personality::Unbound,
            health: DeviceHealth {
                vram_alive: false,
                boot0: 0,
                pmc_enable: 0,
                power: PowerState::Unknown,
                pci_link_width: None,
                domains_alive: 0,
                domains_faulted: 0,
            },
            vendor_id,
            device_id,
            chip_name,
            vfio_holder: None,
            register_snapshot: BTreeMap::new(),
        }
    }

    #[must_use]
    pub const fn has_vfio(&self) -> bool {
        self.vfio_holder.is_some()
    }
}

#[cfg(test)]
mod tests;
