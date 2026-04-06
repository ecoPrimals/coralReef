// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]
#![warn(missing_docs)]
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

pub mod adaptive;
pub mod drm_isolation;
pub mod error;
mod hold;
mod ipc;
pub mod journal;
pub mod observation;
mod swap;
mod sysfs;
pub mod trace;
pub(crate) mod vendor_lifecycle;

mod background;
mod config;
mod runtime;

pub(crate) use background::arm_req_irq;

pub use config::{
    EMBER_LISTEN_PORT_ENV, EmberConfig, EmberDeviceConfig, EmberRunOptions, ember_socket_path,
    find_config, parse_glowplug_config, system_glowplug_config_path,
};
pub use error::EmberIpcError;
pub use hold::{HeldDevice, MailboxMeta, RingMeta, RingMetaEntry};
pub use ipc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, handle_client, send_with_fds};
pub use journal::{Journal, JournalEntry, JournalFilter, JournalStats};
pub use observation::{HealthResult, ResetObservation, SwapObservation, SwapTiming, epoch_ms};
pub use runtime::{run, run_with_options};
pub use swap::{
    handle_swap_device, handle_swap_device_with_journal, verify_drm_isolation_with_paths,
};
pub use vendor_lifecycle::{
    RebindStrategy, ResetMethod, VendorLifecycle, detect_lifecycle, detect_lifecycle_for_target,
};

#[cfg(test)]
mod lib_tests;
