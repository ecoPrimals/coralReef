// SPDX-License-Identifier: AGPL-3.0-only
//! VFIO device holder — keeps file descriptors alive across glowplug restarts.

pub struct HeldDevice {
    pub bdf: String,
    pub device: coral_driver::vfio::VfioDevice,
}
