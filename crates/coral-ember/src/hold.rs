// SPDX-License-Identifier: AGPL-3.0-or-later
//! VFIO device holder — keeps file descriptors alive across glowplug restarts.
//!
//! Backend-agnostic: holds either legacy (container/group/device) or
//! iommufd (iommufd/device) fds depending on the kernel and open path.

use crate::ring_meta::RingMeta;

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
