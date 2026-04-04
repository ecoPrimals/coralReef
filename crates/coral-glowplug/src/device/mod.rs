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
use crate::mailbox::MailboxSet;
use crate::personality::Personality;
use crate::ring::MultiRing;
use crate::sysfs;
use crate::sysfs_ops::{RealSysfs, SysfsOps};
use coral_ember::observation::SwapObservation;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Active experiment session — while set, health probes are paused and
/// a watchdog timer can auto-expire the session.
#[derive(Debug, Clone)]
pub struct ExperimentSession {
    pub name: String,
    pub started_at: std::time::Instant,
    pub watchdog_secs: u64,
}

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
    /// Posted-command mailboxes for GPU engine interaction (FECS, GPCCS, SEC2, PMU).
    /// Enables hotSpring-style firmware probing via `mailbox.*` RPC methods.
    pub mailboxes: MailboxSet,
    /// Multi-ring command submission buffers for ordered, timed GPU interaction.
    /// Enables granular hardware testing and multi-ring dispatch via `ring.*` RPC.
    pub rings: MultiRing,
    /// Most recent swap observation from ember, available for observers and diagnostics.
    pub last_swap_observation: Option<SwapObservation>,
    /// Timestamp of most recent driver swap completion.  Used by the graduated
    /// health model to gate which BAR0 registers are safe to probe.
    last_swap_at: Option<std::time::Instant>,
    /// Set while a long-running `spawn_blocking` task (oracle capture, compute
    /// dispatch) holds a borrowed reference to this slot's VFIO mapping or GPU
    /// context.  Mutating operations (`swap`, `reclaim`, `resurrect`) must
    /// refuse while this flag is set to prevent use-after-unmap.
    busy: Arc<AtomicBool>,
    /// Active experiment session — health checks are paused while set.
    experiment: Option<ExperimentSession>,
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
            mailboxes: MailboxSet::new(),
            rings: MultiRing::new(),
            last_swap_observation: None,
            last_swap_at: None,
            busy: Arc::new(AtomicBool::new(false)),
            experiment: None,
            #[cfg(test)]
            test_vfio_override: None,
            #[cfg(test)]
            test_quiescence_override: None,
        }
    }

    /// Returns the graduated health phase based on time since last swap.
    ///
    /// - Minimal  (0–120s):  BOOT0 + PMC_ENABLE only — safe even if PRI ring
    ///   has uncleared faults from the previous driver teardown.
    /// - Intermediate (120–300s): Add PTIMER — still avoids PRI ring reads.
    /// - Full     (300s+):  Domain probes, PRAMIN write-readback, PRI ring.
    #[must_use]
    pub fn health_phase(&self) -> HealthPhase {
        let Some(swap_at) = self.last_swap_at else {
            return HealthPhase::Full;
        };
        let elapsed = swap_at.elapsed();
        if elapsed < std::time::Duration::from_secs(120) {
            HealthPhase::Minimal
        } else if elapsed < std::time::Duration::from_secs(300) {
            HealthPhase::Intermediate
        } else {
            HealthPhase::Full
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
    pub fn vfio_bar0_handle(&self) -> Option<coral_driver::vfio::channel::mmu_oracle::Bar0Handle> {
        let holder = self.vfio_holder.as_ref()?;
        Some(coral_driver::vfio::channel::mmu_oracle::Bar0Handle::from_mapped_bar(&holder.bar0))
    }

    /// Trigger a PCIe Function Level Reset (FLR) via VFIO_DEVICE_RESET.
    ///
    /// Returns `Err` if no VFIO holder is attached (device not bound to VFIO).
    pub fn reset_device(&self) -> Result<(), crate::error::DeviceError> {
        let holder =
            self.vfio_holder
                .as_ref()
                .ok_or_else(|| crate::error::DeviceError::VfioOpen {
                    bdf: self.bdf.clone(),
                    reason: "no VFIO holder — device not bound to vfio-pci".into(),
                })?;
        holder
            .reset()
            .map_err(|e| crate::error::DeviceError::VfioOpen {
                bdf: self.bdf.clone(),
                reason: format!("VFIO_DEVICE_RESET ioctl failed: {e}"),
            })
    }

    /// Returns `true` if an experiment session is active on this device.
    #[must_use]
    pub fn is_in_experiment(&self) -> bool {
        self.experiment.is_some()
    }

    /// Returns the active experiment session, if any.
    #[must_use]
    pub fn experiment(&self) -> Option<&ExperimentSession> {
        self.experiment.as_ref()
    }

    /// Start an experiment session — pauses health probes and sets a watchdog.
    ///
    /// Returns `Err` if an experiment is already active on this device.
    pub fn experiment_start(
        &mut self,
        name: &str,
        watchdog_secs: u64,
    ) -> Result<&ExperimentSession, crate::error::DeviceError> {
        if let Some(ref existing) = self.experiment {
            return Err(crate::error::DeviceError::VfioOpen {
                bdf: self.bdf.clone(),
                reason: format!(
                    "experiment '{}' already active (started {}s ago)",
                    existing.name,
                    existing.started_at.elapsed().as_secs()
                ),
            });
        }
        self.experiment = Some(ExperimentSession {
            name: name.to_string(),
            started_at: std::time::Instant::now(),
            watchdog_secs,
        });
        Ok(self.experiment.as_ref().unwrap())
    }

    /// End the active experiment session. Returns the session or `None` if none was active.
    pub fn experiment_end(&mut self) -> Option<ExperimentSession> {
        self.experiment.take()
    }

    /// Check and expire the experiment watchdog if the deadline has passed.
    /// Returns `true` if the watchdog fired and the experiment was auto-ended.
    pub fn check_experiment_watchdog(&mut self) -> bool {
        let expired = self
            .experiment
            .as_ref()
            .is_some_and(|e| e.started_at.elapsed().as_secs() >= e.watchdog_secs);
        if expired {
            let session = self.experiment.take().unwrap();
            tracing::warn!(
                bdf = %self.bdf,
                experiment = %session.name,
                elapsed_secs = session.started_at.elapsed().as_secs(),
                watchdog_secs = session.watchdog_secs,
                "WATCHDOG: experiment session auto-expired"
            );
        }
        expired
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
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            Some(BusyGuard(Arc::clone(&self.busy)))
        } else {
            None
        }
    }

    /// Snapshot current ring/mailbox state as [`RingMeta`](coral_ember::RingMeta) for persistence via ember.
    pub fn ring_meta_snapshot(&self) -> coral_ember::RingMeta {
        let mailboxes = self
            .mailboxes
            .iter()
            .map(|m| {
                let stats = m.stats();
                coral_ember::MailboxMeta {
                    engine: m.name().to_string(),
                    capacity: stats.capacity,
                }
            })
            .collect();

        let rings = self
            .rings
            .iter()
            .map(|(name, ring)| coral_ember::RingMetaEntry {
                name: name.to_string(),
                capacity: ring.stats().capacity,
                last_fence: ring.current_fence(),
            })
            .collect();

        coral_ember::RingMeta {
            mailboxes,
            rings,
            version: 0,
        }
    }

    /// Restore ring/mailbox names, capacities, and fence state from a [`RingMeta`](coral_ember::RingMeta) snapshot.
    ///
    /// Recreates ring and mailbox structures (empty but correctly sized) and
    /// restores fence continuity so GlowPlug can resume after a restart while
    /// ember held the fds.
    pub fn restore_ring_meta(&mut self, meta: &coral_ember::RingMeta) {
        for mb in &meta.mailboxes {
            if self.mailboxes.get(&mb.engine).is_none() {
                self.mailboxes
                    .add(crate::mailbox::Mailbox::new(&mb.engine, mb.capacity));
            }
        }
        for ring_meta in &meta.rings {
            if self.rings.get(&ring_meta.name).is_none() {
                let mut ring = crate::ring::Ring::new(&ring_meta.name, ring_meta.capacity);
                ring.restore_fence(ring_meta.last_fence);
                self.rings.add(ring);
            } else if let Some(existing) = self.rings.get_mut(&ring_meta.name) {
                existing.restore_fence(ring_meta.last_fence);
            }
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
