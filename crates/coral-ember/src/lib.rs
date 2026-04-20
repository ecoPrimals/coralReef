// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![allow(
    clippy::redundant_pub_crate,
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::redundant_closure_for_method_calls,
    clippy::missing_const_for_fn,
    clippy::items_after_statements,
    clippy::manual_let_else,
    clippy::option_if_let_else,
    clippy::map_unwrap_or,
    clippy::single_match_else,
    clippy::unnecessary_wraps,
    clippy::significant_drop_tightening,
    clippy::implicit_hasher,
    clippy::needless_lifetimes,
    clippy::unnecessary_literal_bound,
    clippy::match_same_arms,
    clippy::missing_panics_doc
)]
//! coral-ember — Immortal VFIO fd holder for safe daemon restarts.
//!
//! Holds VFIO fds open and passes duplicates to coral-glowplug via
//! `SCM_RIGHTS`. Backend-agnostic: supports both legacy container/group
//! (kernel < 6.2) and iommufd/cdev (kernel 6.2+) paths. When glowplug
//! dies, ember's fds prevent the kernel from performing a PM reset.
//!
//! Usage:
//!   `coral-ember server` / `coral-ember server --port 9000`
//!   `coral-ember /etc/coralreef/glowplug.toml` (legacy: same as `server` with a config path)
//!   Auto-discovers config from XDG/system paths when omitted; override system path with
//!   `$CORALREEF_GLOWPLUG_CONFIG`.
//!
//! The full daemon and VFIO IPC surface are **Linux-only**. On other platforms this crate
//! exposes configuration, journal, observation, and ring metadata types for cross-compilation.

#[cfg(target_os = "linux")]
pub mod adaptive;
#[cfg(target_os = "linux")]
mod background;
pub mod drm_isolation;
pub mod error;
#[cfg(target_os = "linux")]
mod hold;
mod ipc;
pub mod journal;
pub mod observation;
pub mod ring_meta;
#[cfg(target_os = "linux")]
mod runtime;
#[cfg(target_os = "linux")]
mod swap;
#[cfg(target_os = "linux")]
mod sysfs;
#[cfg(target_os = "linux")]
pub mod trace;
#[cfg(target_os = "linux")]
pub(crate) mod vendor_lifecycle;

#[cfg(target_os = "linux")]
pub(crate) mod btsp;
mod config;

pub use config::{
    EMBER_LISTEN_PORT_ENV, EmberConfig, EmberDeviceConfig, EmberRunOptions, ember_socket_path,
    find_config, parse_glowplug_config, system_glowplug_config_path, validate_insecure_guard,
};
pub use error::{ConfigError, EmberIpcError};
#[cfg(target_os = "linux")]
pub use hold::HeldDevice;
pub use ipc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, send_with_fds};
#[cfg(target_os = "linux")]
pub use ipc::{handle_client, handle_client_tcp};
pub use journal::{Journal, JournalEntry, JournalFilter, JournalStats};
pub use observation::{HealthResult, ResetObservation, SwapObservation, SwapTiming, epoch_ms};
pub use ring_meta::{MailboxMeta, RingMeta, RingMetaEntry};
#[cfg(target_os = "linux")]
pub use runtime::{run, run_with_options};
#[cfg(target_os = "linux")]
pub use swap::{
    handle_swap_device, handle_swap_device_with_journal, verify_drm_isolation_with_paths,
};
#[cfg(target_os = "linux")]
pub use vendor_lifecycle::{
    RebindStrategy, ResetMethod, VendorLifecycle, detect_lifecycle, detect_lifecycle_for_target,
};

#[cfg(test)]
mod lib_tests;
