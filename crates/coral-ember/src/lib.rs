// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! coral-ember — Immortal VFIO fd holder for safe daemon restarts.
//!
//! Holds VFIO container/group/device fds open forever and passes
//! duplicates to coral-glowplug via `SCM_RIGHTS`. When glowplug dies,
//! ember's fds prevent the kernel from performing a PM reset on GV100.
//!
//! Usage:
//!   coral-ember /etc/coralreef/glowplug.toml
//!   coral-ember  (auto-discovers config from XDG/system paths; override system path with `$CORALREEF_GLOWPLUG_CONFIG`)

mod hold;
mod ipc;
mod swap;
mod sysfs;
pub(crate) mod vendor_lifecycle;

use std::collections::HashMap;
use std::os::unix::net::UnixListener;

use serde::Deserialize;

pub use hold::HeldDevice;
pub use ipc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, handle_client, send_with_fds};
pub use swap::{handle_swap_device, verify_drm_isolation_with_paths};
pub use vendor_lifecycle::{RebindStrategy, VendorLifecycle, detect_lifecycle};

/// Parsed `glowplug.toml` top-level structure for ember.
#[derive(Deserialize)]
pub struct EmberConfig {
    #[serde(default)]
    /// Devices listed in the glowplug config (BDF and optional metadata).
    pub device: Vec<EmberDeviceConfig>,
}

/// One device entry from `glowplug.toml` (same schema as coral-glowplug).
#[derive(Deserialize)]
pub struct EmberDeviceConfig {
    /// PCI bus/device/function address (e.g. `0000:01:00.0`).
    pub bdf: String,
    #[serde(default)]
    /// Optional human-readable name.
    pub name: Option<String>,
    #[serde(default)]
    /// Boot personality hint.
    pub boot_personality: Option<String>,
    #[serde(default)]
    /// Power policy hint.
    pub power_policy: Option<String>,
    #[serde(default)]
    /// Role hint (e.g. compute).
    pub role: Option<String>,
    #[serde(default)]
    /// Oracle dump path.
    pub oracle_dump: Option<String>,
}

/// Default socket path for ember IPC. Override with `$CORALREEF_EMBER_SOCKET`.
#[must_use]
pub fn ember_socket_path() -> String {
    std::env::var("CORALREEF_EMBER_SOCKET")
        .unwrap_or_else(|_| "/run/coralreef/ember.sock".to_string())
}

/// System-wide glowplug config path (same default and `$CORALREEF_GLOWPLUG_CONFIG` as coral-glowplug).
#[must_use]
pub fn system_glowplug_config_path() -> String {
    std::env::var("CORALREEF_GLOWPLUG_CONFIG")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/etc/coralreef/glowplug.toml".to_string())
}

/// Parse `glowplug.toml` contents into [`EmberConfig`].
pub fn parse_glowplug_config(config_str: &str) -> Result<EmberConfig, toml::de::Error> {
    toml::from_str(config_str)
}

/// Resolve a glowplug config path: XDG config first, then [`system_glowplug_config_path`].
#[must_use]
pub fn find_config() -> Option<String> {
    let xdg = std::env::var("XDG_CONFIG_HOME")
        .or_else(|_| std::env::var("HOME").map(|h| format!("{h}/.config")))
        .ok()
        .map(|base| format!("{base}/coralreef/glowplug.toml"));

    let system = system_glowplug_config_path();

    let paths: Vec<String> = xdg.into_iter().chain(std::iter::once(system)).collect();

    for path in paths {
        if std::path::Path::new(&path).exists() {
            tracing::info!(path = %path, "found config");
            return Some(path);
        }
    }
    None
}

/// Entry point for the coral-ember daemon: load config, hold VFIO fds, serve JSON-RPC on the Unix socket.
///
/// On startup failure, returns `Err(exit_code)` (typically `1`). On success, blocks in the accept
/// loop until the process is terminated.
///
/// # Errors
///
/// Returns `Err(1)` when configuration is missing, invalid, empty, or VFIO setup fails.
pub fn run() -> Result<(), i32> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config_path = match std::env::args().nth(1).or_else(find_config) {
        Some(p) => p,
        None => {
            tracing::error!("usage: coral-ember [config.toml]");
            tracing::error!("  no config found in XDG/system paths");
            return Err(1);
        }
    };

    let config_str = match std::fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(path = %config_path, error = %e, "failed to read config");
            return Err(1);
        }
    };

    let config: EmberConfig = match parse_glowplug_config(&config_str) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(path = %config_path, error = %e, "failed to parse config");
            return Err(1);
        }
    };

    if config.device.is_empty() {
        tracing::error!("no devices configured — nothing to hold");
        return Err(1);
    }

    let started_at = std::time::Instant::now();
    let mut held: HashMap<String, HeldDevice> = HashMap::new();

    for dev_config in &config.device {
        let lifecycle = vendor_lifecycle::detect_lifecycle(&dev_config.bdf);
        let current = sysfs::read_current_driver(&dev_config.bdf);
        if let Some(ref drv) = current {
            if let Err(e) = lifecycle.prepare_for_unbind(&dev_config.bdf, drv) {
                tracing::warn!(
                    bdf = %dev_config.bdf, error = %e,
                    "startup: prepare_for_unbind failed (non-fatal)"
                );
            }
        } else {
            sysfs::pin_power(&dev_config.bdf);
        }
    }

    for dev_config in &config.device {
        tracing::info!(bdf = %dev_config.bdf, "opening VFIO device for ember hold");

        let group_id = sysfs::read_iommu_group(&dev_config.bdf);
        if group_id != 0 {
            sysfs::bind_iommu_group_to_vfio(&dev_config.bdf, group_id);
        }

        sysfs::pin_power(&dev_config.bdf);

        match coral_driver::vfio::VfioDevice::open(&dev_config.bdf) {
            Ok(device) => {
                tracing::info!(
                    bdf = %dev_config.bdf,
                    container_fd = device.container_fd(),
                    group_fd = device.group_fd(),
                    device_fd = device.device_fd(),
                    "VFIO device held by ember"
                );
                held.insert(
                    dev_config.bdf.clone(),
                    HeldDevice {
                        bdf: dev_config.bdf.clone(),
                        device,
                    },
                );
            }
            Err(e) => {
                tracing::error!(
                    bdf = %dev_config.bdf,
                    error = %e,
                    "failed to open VFIO device — ember will not hold this device"
                );
            }
        }
    }

    if held.is_empty() {
        tracing::error!("no devices held — ember cannot provide fd keepalive");
        return Err(1);
    }

    let socket_path = ember_socket_path();

    if let Some(parent) = std::path::Path::new(&socket_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::remove_file(&socket_path);

    let listener = match UnixListener::bind(&socket_path) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(path = %socket_path, error = %e, "failed to bind ember socket");
            return Err(1);
        }
    };

    let _ = std::fs::set_permissions(
        &socket_path,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o660),
    );

    tracing::info!("╔══════════════════════════════════════════════════════════╗");
    tracing::info!("║ coral-ember — Immortal VFIO fd Holder                   ║");
    tracing::info!("╠══════════════════════════════════════════════════════════╣");
    for dev in held.values() {
        tracing::info!("║ {} (fd={})", dev.bdf, dev.device.device_fd());
    }
    tracing::info!("║ Socket: {socket_path}");
    tracing::info!("╚══════════════════════════════════════════════════════════╝");

    if let Ok(ref path) = std::env::var("NOTIFY_SOCKET") {
        let _ = std::os::unix::net::UnixDatagram::unbound()
            .and_then(|sock| sock.send_to(b"READY=1", path));
    }

    for stream in listener.incoming() {
        match stream {
            Ok(ref stream) => {
                if let Err(e) = ipc::handle_client(stream, &mut held, started_at) {
                    tracing::warn!(error = %e, "client handler error");
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "accept error");
            }
        }
    }

    tracing::error!("ember accept loop ended unexpectedly");
    Err(1)
}
