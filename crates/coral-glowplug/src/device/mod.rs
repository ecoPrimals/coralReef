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
use crate::sysfs_ops::{RealSysfs, SysfsOps};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct DeviceSlot<S: SysfsOps = RealSysfs> {
    pub config: DeviceConfig,
    pub bdf: Arc<str>,
    pub personality: Personality,
    pub health: DeviceHealth,
    pub vendor_id: u16,
    pub device_id: u16,
    pub chip_name: String,
    vfio_holder: Option<types::VfioHolder>,
    register_snapshot: BTreeMap<usize, u32>,
    sysfs: S,
    /// Set while a long-running `spawn_blocking` task (oracle capture, compute
    /// dispatch) holds a borrowed reference to this slot's VFIO mapping or GPU
    /// context.  Mutating operations (`swap`, `reclaim`, `resurrect`) must
    /// refuse while this flag is set to prevent use-after-unmap.
    busy: Arc<AtomicBool>,
    /// When `Some`, overrides [`Self::has_vfio`] for unit tests (circuit breaker, etc.).
    #[cfg(test)]
    test_vfio_override: Option<bool>,
    /// When `Some`, overrides the GPU quiescence probe in tests (see `device::health`).
    #[cfg(test)]
    test_quiescence_override: Option<bool>,
}

impl<S: SysfsOps> DeviceSlot<S> {
    /// Construct a slot with an explicit [`SysfsOps`] backend.
    pub fn with_sysfs(config: DeviceConfig, ops: S) -> Self {
        let bdf: Arc<str> = Arc::from(config.bdf.as_str());
        let (vendor_id, device_id) = ops.read_pci_ids(&bdf);
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
            sysfs: ops,
            busy: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            test_vfio_override: None,
            #[cfg(test)]
            test_quiescence_override: None,
        }
    }

    #[must_use]
    pub fn has_vfio(&self) -> bool {
        #[cfg(test)]
        if let Some(v) = self.test_vfio_override {
            return v;
        }
        self.vfio_holder.is_some()
    }

    /// Create a `Send`-safe BAR0 handle for use in `spawn_blocking` tasks.
    ///
    /// The returned handle is valid as long as this slot's `VfioHolder` is alive.
    #[must_use]
    pub fn vfio_bar0_handle(
        &self,
    ) -> Option<coral_driver::vfio::channel::mmu_oracle::Bar0Handle> {
        let holder = self.vfio_holder.as_ref()?;
        Some(coral_driver::vfio::channel::mmu_oracle::Bar0Handle::from_mapped_bar(
            &holder.bar0,
        ))
    }

    /// Trigger a PCIe Function Level Reset (FLR) via VFIO_DEVICE_RESET.
    ///
    /// Returns `Err` if no VFIO holder is attached (device not bound to VFIO).
    pub fn reset_device(&self) -> Result<(), crate::error::DeviceError> {
        let holder = self.vfio_holder.as_ref().ok_or_else(|| {
            crate::error::DeviceError::VfioOpen {
                bdf: self.bdf.clone(),
                reason: "no VFIO holder — device not bound to vfio-pci".into(),
            }
        })?;
        holder.reset().map_err(|e| crate::error::DeviceError::VfioOpen {
            bdf: self.bdf.clone(),
            reason: format!("VFIO_DEVICE_RESET ioctl failed: {e}"),
        })
    }

    /// Returns `true` if a `spawn_blocking` task currently holds a reference
    /// to this slot's GPU resources (BAR0 mapping, CUDA context, etc.).
    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    /// Acquire a `BusyGuard` that sets the busy flag for the duration of a
    /// long-running blocking task.  Returns `None` if the slot is already busy.
    #[must_use]
    pub fn try_acquire_busy(&self) -> Option<BusyGuard> {
        if self.busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            Some(BusyGuard(Arc::clone(&self.busy)))
        } else {
            None
        }
    }
}

/// RAII guard that clears a `DeviceSlot`'s busy flag on drop.
/// Safe to send into `spawn_blocking` tasks.
pub struct BusyGuard(Arc<AtomicBool>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
impl<S: SysfsOps> DeviceSlot<S> {
    /// Override whether [`Self::has_vfio`] reports true (`None` = use real `vfio_holder`).
    pub fn test_set_vfio_override(&mut self, vfio: Option<bool>) {
        self.test_vfio_override = vfio;
    }

    /// Override quiescence probe (`None` = real BAR0 reads).
    pub fn test_set_quiescence_override(&mut self, quiescent: Option<bool>) {
        self.test_quiescence_override = quiescent;
    }
}

impl DeviceSlot<RealSysfs> {
    /// Discover the device from sysfs and construct a slot using real `/sys` access.
    pub fn new(config: DeviceConfig) -> Self {
        Self::with_sysfs(config, RealSysfs)
    }
}

#[cfg(test)]
mod coverage_tests;

#[cfg(test)]
mod health_tests;

#[cfg(test)]
mod tests;
