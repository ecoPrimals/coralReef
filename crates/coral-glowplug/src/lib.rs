// SPDX-License-Identifier: AGPL-3.0-or-later
#![warn(missing_docs)]
#![forbid(unsafe_code)]
//! coral-glowplug library — shared types for the sovereign PCIe device lifecycle broker.
//!
//! Re-exports [`DeviceSlot`](device::DeviceSlot), [`Personality`](personality::Personality),
//! [`DeviceError`](error::DeviceError), [`Config`](config::Config),
//! [`EmberClient`](ember::EmberClient), [`SysfsOps`],
//! and sysfs helpers for consumption by ecosystem crates.
//!
//! # Examples
//!
//! Parse a minimal [`Config`](config::Config) and inspect search paths (no hardware I/O):
//!
//! ```
//! use coral_glowplug::config::{config_search_paths, Config};
//!
//! let toml = r#"[[device]]
//! bdf = "0000:01:00.0"
//! "#;
//! let cfg: Config = toml::from_str(toml).expect("deserialize config");
//! assert_eq!(cfg.device.len(), 1);
//! let _paths = config_search_paths();
//! ```
//!
//! Build a [`DeviceSlot`](device::DeviceSlot) with the real sysfs backend (touches `/sys`; not run in docs):
//!
//! ```no_run
//! use coral_glowplug::config::Config;
//! use coral_glowplug::device::DeviceSlot;
//! use coral_glowplug::RealSysfs;
//!
//! let toml = r#"[[device]]
//! bdf = "0000:01:00.0"
//! "#;
//! let cfg: Config = toml::from_str(toml).expect("deserialize config");
//! let _slot = DeviceSlot::with_sysfs(cfg.device[0].clone(), RealSysfs::default());
//! ```
//!
//! Probe for [`EmberClient`](ember::EmberClient) (returns `None` if the ember socket is absent):
//!
//! ```
//! use coral_glowplug::ember::EmberClient;
//!
//! let _maybe = EmberClient::connect();
//! ```

pub mod config;
pub mod device;
pub mod ember;
pub mod error;
#[cfg(unix)]
pub mod group_unix;
pub mod health;
pub mod mailbox;
pub mod observer;
pub mod pci_ids;
pub mod personality;
pub mod ring;
pub mod sec2_bridge;
pub mod sysfs;
pub mod sysfs_ops;

pub use sysfs_ops::{RealSysfs, SysfsOps};

#[cfg(test)]
pub use sysfs_ops::MockSysfs;

#[doc(hidden)]
pub use ember::test_support_default_ember_socket;

#[doc(hidden)]
pub use health::test_support_notify_watchdog;
