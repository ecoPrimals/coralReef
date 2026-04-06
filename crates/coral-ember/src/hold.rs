// SPDX-License-Identifier: AGPL-3.0-or-later
//! VFIO device holder — keeps file descriptors alive across glowplug restarts.
//!
//! Backend-agnostic: holds either legacy (container/group/device) or
//! iommufd (iommufd/device) fds depending on the kernel and open path.

use serde::{Deserialize, Serialize};

/// A GPU (or other PCI device) held open by ember with an associated [`coral_driver::vfio::VfioDevice`].
pub struct HeldDevice {
    /// PCI address (`0000:01:00.0` style).
    pub bdf: String,
    /// Open VFIO device — backend (legacy or iommufd) determined at open time.
    pub device: coral_driver::vfio::VfioDevice,
    /// Ring/mailbox metadata persisted across glowplug restarts.
    /// Glowplug writes this before shutdown; reads it after reacquiring fds.
    pub ring_meta: RingMeta,
    /// Eventfd armed on `VFIO_PCI_REQ_ERR_IRQ` (index 4). The kernel
    /// signals this when a driver unbind is pending, giving ember a
    /// chance to close the VFIO fd before the unbind blocks in D-state.
    /// The [`super::spawn_req_watcher`] thread monitors all active eventfds.
    pub(crate) req_eventfd: Option<std::os::fd::OwnedFd>,
}

impl HeldDevice {
    /// Construct a `HeldDevice` without arming the REQ IRQ.
    ///
    /// Used in tests and non-standard init paths where the VFIO REQ IRQ
    /// watcher is not active.
    pub fn new_unmonitored(bdf: String, device: coral_driver::vfio::VfioDevice) -> Self {
        Self {
            bdf,
            device,
            ring_meta: RingMeta::default(),
            req_eventfd: None,
        }
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
