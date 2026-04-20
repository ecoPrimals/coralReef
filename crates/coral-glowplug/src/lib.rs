// SPDX-License-Identifier: AGPL-3.0-or-later
#![warn(missing_docs)]
#![forbid(unsafe_code)]
#![allow(
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::unreadable_literal,
    clippy::redundant_closure_for_method_calls,
    clippy::missing_const_for_fn,
    clippy::option_if_let_else,
    clippy::map_unwrap_or,
    clippy::single_match_else,
    clippy::manual_let_else,
    clippy::bool_to_int_with_if,
    clippy::needless_pass_by_value,
    clippy::match_same_arms,
    clippy::redundant_pub_crate,
    clippy::branches_sharing_code,
    clippy::uninlined_format_args,
    clippy::significant_drop_tightening,
    clippy::or_fun_call,
    clippy::semicolon_if_nothing_returned,
    clippy::items_after_statements
)]
//! coral-glowplug library — shared types for the sovereign PCIe device lifecycle broker.
//!
//! The full daemon, VFIO device stack, ember bridge, and capture pipelines are **Linux-only**
//! (`target_os = "linux"`). On other targets this crate exposes configuration, sysfs helpers,
//! and related types for cross-compilation.
//!
//! Re-exports [`Personality`](personality::Personality),
//! [`DeviceError`](error::DeviceError), [`Config`](config::Config),
//! [`SysfsOps`], and sysfs helpers for consumption by ecosystem crates.
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
//! Build a [`DeviceSlot`](device::DeviceSlot) with the real sysfs backend (Linux only; touches `/sys`):
//!
//! ```ignore
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
//! Probe for [`EmberClient`](ember::EmberClient) (Linux only; returns `None` if the ember socket is absent):
//!
//! ```ignore
//! use coral_glowplug::ember::EmberClient;
//!
//! let _maybe = EmberClient::connect();
//! ```

#[cfg(target_os = "linux")]
pub mod capture;
pub mod config;
#[cfg(target_os = "linux")]
pub mod device;
#[cfg(target_os = "linux")]
pub mod ember;
pub mod error;
#[cfg(unix)]
pub mod group_unix;
#[cfg(target_os = "linux")]
pub mod health;
pub mod mailbox;
pub mod observer;
pub mod pci_ids;
pub mod personality;
pub mod power_state;
pub mod ring;
#[cfg(target_os = "linux")]
pub mod sec2_bridge;
#[cfg(target_os = "linux")]
pub mod sovereign;
pub mod sysfs;
pub mod sysfs_ops;

pub use sysfs_ops::{RealSysfs, SysfsOps};

#[cfg(all(test, target_os = "linux"))]
pub use sysfs_ops::MockSysfs;

#[cfg(target_os = "linux")]
#[doc(hidden)]
pub use ember::test_support_default_ember_socket;

#[cfg(target_os = "linux")]
#[doc(hidden)]
pub use health::test_support_notify_watchdog;
