// SPDX-License-Identifier: AGPL-3.0-only
#![warn(missing_docs)]
#![forbid(unsafe_code)]
//! coral-glowplug library — shared types for the sovereign PCIe device lifecycle broker.
//!
//! Re-exports [`DeviceSlot`](device::DeviceSlot), [`Personality`](personality::Personality),
//! [`DeviceError`](error::DeviceError), [`Config`](config::Config),
//! [`EmberClient`](ember::EmberClient), [`SysfsOps`],
//! and sysfs helpers for consumption by ecosystem crates.

pub mod config;
pub mod device;
pub mod ember;
pub mod error;
pub mod health;
pub mod mailbox;
pub mod observer;
pub mod pci_ids;
pub mod personality;
pub mod ring;
pub mod sysfs;
pub mod sysfs_ops;

pub use sysfs_ops::{RealSysfs, SysfsOps};

#[cfg(test)]
pub use sysfs_ops::MockSysfs;

#[doc(hidden)]
pub use ember::test_support_default_ember_socket;

#[doc(hidden)]
pub use health::test_support_notify_watchdog;
