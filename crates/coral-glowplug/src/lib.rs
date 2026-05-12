// SPDX-License-Identifier: AGPL-3.0-or-later
#![warn(missing_docs)]
#![forbid(unsafe_code)]
#![allow(deprecated, reason = "crate is soft-deprecated — absorbed into toadStool")]
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
    clippy::items_after_statements,
    reason = "PCIe lifecycle broker: sysfs/VFIO device management patterns require casts and kernel-mirrored naming"
)]
//! **Deprecated**: This crate is absorbed into toadStool (Phase B). Use
//! `toadstool-glowplug` for new development. Bug fixes only until toadStool
//! Phase C confirms full coverage, then this crate will be removed.
//!
//! ---
//!
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

#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
#[cfg(target_os = "linux")]
pub mod capture;
#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
pub mod config;
#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
#[cfg(target_os = "linux")]
pub mod device;
#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
#[cfg(target_os = "linux")]
pub mod ember;
#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
pub mod error;
#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
#[cfg(unix)]
pub mod group_unix;
#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
#[cfg(target_os = "linux")]
pub mod health;
#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
pub mod mailbox;
#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
pub mod observer;
#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
pub mod pci_ids;
#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
pub mod personality;
#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
pub mod power_state;
#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
pub mod ring;
#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
#[cfg(target_os = "linux")]
pub mod sec2_bridge;
#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
#[cfg(target_os = "linux")]
pub mod sovereign;
#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
pub mod sysfs;
#[deprecated(since = "0.2.0", note = "Absorbed into toadStool Phase B — use toadstool-glowplug")]
pub mod sysfs_ops;

#[allow(deprecated, reason = "re-exporting deprecated items for backward compatibility")]
pub use sysfs_ops::{RealSysfs, SysfsOps};

#[allow(deprecated, reason = "re-exporting deprecated items for backward compatibility")]
#[cfg(all(test, target_os = "linux"))]
pub use sysfs_ops::MockSysfs;

#[allow(deprecated, reason = "re-exporting deprecated items for backward compatibility")]
#[cfg(target_os = "linux")]
#[doc(hidden)]
pub use ember::test_support_default_ember_socket;

#[allow(deprecated, reason = "re-exporting deprecated items for backward compatibility")]
#[cfg(target_os = "linux")]
#[doc(hidden)]
pub use health::test_support_notify_watchdog;
