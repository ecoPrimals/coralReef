// SPDX-License-Identifier: AGPL-3.0-only
//! VFIO device holder — keeps file descriptors alive across glowplug restarts.
//!
//! Backend-agnostic: holds either legacy (container/group/device) or
//! iommufd (iommufd/device) fds depending on the kernel and open path.

use serde::{Deserialize, Serialize};

/// Device health — tracks the GPU's lifecycle state for operation gating.
///
/// State machine:
/// ```text
///           ┌──────────┐
///     ┌─────│  Cold     │◄──── PM reset / reboot
///     │     └──────────┘
///     │ warm cycle │
///     │            ▼
///     │     ┌──────────┐     PRAMIN writes
///     │     │ Pristine  │────────────────────┐
///     │     └──────────┘                     │
///     │            │                         ▼
///     │    engine   │                 ┌──────────────┐
///     │    reset    │                 │ Configured    │
///     │            ▼                 └──────────────┘
///     │     ┌──────────┐     bind           │
///     │     │ Active    │◄──────────────────┘
///     │     └──────────┘
///     │            │ error
///     │            ▼
///     │     ┌──────────┐  bus reset / warm
///     │     │ Faulted   │────────────────────┐
///     │     └──────────┘                     │
///     │                                      ▼
///     │     ┌──────────┐               ┌──────────┐
///     └────►│          │◄──────────────│Recovering │
///           └──────────┘  success      └──────────┘
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum DeviceHealth {
    /// No warm state — needs nouveau warm cycle before any GPU operations.
    Cold,
    /// Nouveau-warmed, no register changes yet. PRAMIN writes and register
    /// I/O are both safe. This is the starting state after a warm cycle.
    Pristine,
    /// PRAMIN page tables written, FBIF configured. Ready for bind.
    Configured,
    /// Falcon bound and running. Full operation permitted.
    Active,
    /// Error detected — needs recovery. Only diagnostics and recovery
    /// operations are allowed; all MMIO RPCs return an error.
    Faulted,
    /// Bus reset or warm cycle in progress. Operations blocked.
    Recovering,
    /// Legacy alias for `Pristine` — used by existing code that sets `Alive`.
    /// Semantically equivalent to `Pristine` for operation gating.
    Alive,
}

/// A GPU (or other PCI device) held open by ember with an associated [`coral_driver::vfio::VfioDevice`].
pub struct HeldDevice {
    /// PCI address (`0000:01:00.0` style).
    pub bdf: String,
    /// Open VFIO device — backend (legacy or iommufd) determined at open time.
    pub device: coral_driver::vfio::VfioDevice,
    /// Lazily-mapped BAR0 for server-side MMIO operations. Mapped on first
    /// `ember.mmio.*` / `ember.pramin.*` / `ember.sec2.*` / `ember.falcon.*`
    /// RPC call. Dropped on device release or experiment end.
    pub bar0: Option<coral_driver::vfio::device::MappedBar>,
    /// Ring/mailbox metadata persisted across glowplug restarts.
    /// Glowplug writes this before shutdown; reads it after reacquiring fds.
    pub ring_meta: RingMeta,
    /// Eventfd armed on `VFIO_PCI_REQ_ERR_IRQ` (index 4). The kernel
    /// signals this when a driver unbind is pending, giving ember a
    /// chance to close the VFIO fd before the unbind blocks in D-state.
    /// The [`super::spawn_req_watcher`] thread monitors all active eventfds.
    pub(crate) req_eventfd: Option<std::os::fd::OwnedFd>,
    /// Set `true` when BAR0 registers have been written via `coralctl mmio write`
    /// or any experiment path. A dirty device may be in an inconsistent state
    /// that causes D-state during driver swaps. The pre-unbind safety layer
    /// (Exp 138) uses this to apply extra caution (PRAMIN restore, BAR0 health
    /// check) before releasing VFIO fds.
    pub experiment_dirty: bool,
    /// Saved DMA prepare state from `ember.prepare_dma` — holds AER mask state
    /// needed by `ember.cleanup_dma` to restore masks after an experiment.
    pub dma_prepare_state: Option<coral_driver::vfio::device::dma_safety::DmaPrepareState>,
    /// Consecutive faulted BOOT0 reads. When this exceeds
    /// [`MMIO_CIRCUIT_BREAKER_THRESHOLD`], all MMIO RPCs are refused until
    /// the device is manually reset or recycled.
    pub mmio_fault_count: u32,
    /// Current health state — checked by all MMIO handlers before touching
    /// hardware. Set to [`DeviceHealth::Faulted`] by the MMIO watchdog
    /// when an operation times out and a bus reset is triggered.
    pub health: DeviceHealth,
    /// Per-device PCIe protection state: AER/DPC/timeout hardening and
    /// the MMIO write-ordering sequencer. Armed at acquisition, disarmed
    /// at release.
    pub pcie_armor: Option<crate::pcie_armor::PcieArmor>,
}

impl DeviceHealth {
    /// Whether the device is in a state that allows MMIO register operations.
    pub fn allows_mmio(&self) -> bool {
        matches!(
            self,
            Self::Pristine | Self::Configured | Self::Active | Self::Alive
        )
    }

    /// Whether the device is in a state that allows PRAMIN (bulk VRAM) writes.
    pub fn allows_vram_write(&self) -> bool {
        matches!(self, Self::Pristine | Self::Alive)
    }

    /// Whether the device needs a warm cycle before any operations.
    pub fn needs_warm(&self) -> bool {
        matches!(self, Self::Cold)
    }
}

/// After this many consecutive faulted pre-flight checks, ember refuses
/// further MMIO operations on the device to prevent system lockups.
pub const MMIO_CIRCUIT_BREAKER_THRESHOLD: u32 = 3;

impl HeldDevice {
    /// Construct a `HeldDevice` without arming the REQ IRQ.
    ///
    /// Used in tests and non-standard init paths where the VFIO REQ IRQ
    /// watcher is not active.
    pub fn new_unmonitored(bdf: String, device: coral_driver::vfio::VfioDevice) -> Self {
        Self {
            bdf,
            device,
            bar0: None,
            ring_meta: RingMeta::default(),
            req_eventfd: None,
            experiment_dirty: false,
            dma_prepare_state: None,
            mmio_fault_count: 0,
            health: DeviceHealth::Alive,
            pcie_armor: None,
        }
    }

    /// Ensure BAR0 is mapped, returning a reference to it. Maps lazily on
    /// first call; subsequent calls return the cached mapping.
    pub fn ensure_bar0(
        &mut self,
    ) -> Result<&coral_driver::vfio::device::MappedBar, coral_driver::error::DriverError> {
        if self.bar0.is_none() {
            let mapped = self.device.map_bar(0)?;
            self.bar0 = Some(mapped);
        }
        Ok(self.bar0.as_ref().unwrap())
    }
}

/// Persistent metadata for mailbox/ring reconstruction after daemon restart.
///
/// Ember holds this alongside VFIO fds. When glowplug dies and restarts,
/// it reads this metadata via `ember.ring_meta.get` to restore its
/// `MailboxSet` and `MultiRing` (coral-glowplug) state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RingMeta {
    /// Active mailbox engine names and their capacities.
    pub mailboxes: Vec<MailboxMeta>,
    /// Active ring names and their capacities.
    pub rings: Vec<RingMetaEntry>,
    /// Monotonic version — incremented on each update for consistency checking.
    pub version: u64,
}

/// Metadata for one mailbox (engine name + capacity).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxMeta {
    /// Engine name (e.g. `"fecs"`, `"gpccs"`, `"sec2"`).
    pub engine: String,
    /// Slot capacity.
    pub capacity: usize,
}

/// Metadata for one ring (name + capacity + last fence).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RingMetaEntry {
    /// Ring name (e.g. `"gpfifo"`, `"ce0"`).
    pub name: String,
    /// Entry capacity.
    pub capacity: usize,
    /// Last consumed fence value (for continuity after restart).
    pub last_fence: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_meta_default_is_empty() {
        let meta = RingMeta::default();
        assert!(meta.mailboxes.is_empty());
        assert!(meta.rings.is_empty());
        assert_eq!(meta.version, 0);
    }

    #[test]
    fn ring_meta_roundtrip_json() {
        let meta = RingMeta {
            mailboxes: vec![MailboxMeta {
                engine: "fecs".into(),
                capacity: 16,
            }],
            rings: vec![RingMetaEntry {
                name: "gpfifo".into(),
                capacity: 64,
                last_fence: 42,
            }],
            version: 3,
        };
        let json = serde_json::to_string(&meta).expect("serialize");
        let back: RingMeta = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.mailboxes.len(), 1);
        assert_eq!(back.mailboxes[0].engine, "fecs");
        assert_eq!(back.rings[0].last_fence, 42);
        assert_eq!(back.version, 3);
    }
}
