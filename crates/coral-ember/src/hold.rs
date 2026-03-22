// SPDX-License-Identifier: AGPL-3.0-only
//! VFIO device holder — keeps file descriptors alive across glowplug restarts.
//!
//! Backend-agnostic: holds either legacy (container/group/device) or
//! iommufd (iommufd/device) fds depending on the kernel and open path.

/// A GPU (or other PCI device) held open by ember with an associated [`coral_driver::vfio::VfioDevice`].
pub struct HeldDevice {
    /// PCI address (`0000:01:00.0` style).
    pub bdf: String,
    /// Open VFIO device — backend (legacy or iommufd) determined at open time.
    pub device: coral_driver::vfio::VfioDevice,
}
