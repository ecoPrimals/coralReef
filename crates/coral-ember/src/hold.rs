// SPDX-License-Identifier: AGPL-3.0-only
//! VFIO device holder — keeps file descriptors alive across glowplug restarts.

/// A GPU (or other PCI device) held open by ember with an associated [`coral_driver::vfio::VfioDevice`].
pub struct HeldDevice {
    /// PCI address (`0000:01:00.0` style).
    pub bdf: String,
    /// Open VFIO container/group/device fds.
    pub device: coral_driver::vfio::VfioDevice,
}
