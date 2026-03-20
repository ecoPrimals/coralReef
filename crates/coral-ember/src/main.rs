// SPDX-License-Identifier: AGPL-3.0-only
#![warn(missing_docs)]
//! coral-ember — Immortal VFIO fd holder for safe daemon restarts.
//!
//! Holds VFIO container/group/device fds open forever and passes
//! duplicates to coral-glowplug via `SCM_RIGHTS`. When glowplug dies,
//! ember's fds prevent the kernel from performing a PM reset on GV100.
//!
//! Usage:
//!   coral-ember /etc/coralreef/glowplug.toml
//!   coral-ember  (auto-discovers config from XDG/system paths)
#![deny(unsafe_code)]

mod hold;
mod ipc;
mod swap;
mod sysfs;
pub(crate) mod vendor_lifecycle;

use std::collections::HashMap;
use std::os::unix::net::UnixListener;

use serde::Deserialize;

use hold::HeldDevice;

const EMBER_SOCKET: &str = "/run/coralreef/ember.sock";

#[derive(Deserialize)]
struct EmberConfig {
    #[serde(default)]
    device: Vec<EmberDeviceConfig>,
}

#[derive(Deserialize)]
#[allow(
    dead_code,
    reason = "fields parsed from glowplug.toml but only bdf is used"
)]
struct EmberDeviceConfig {
    bdf: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    boot_personality: Option<String>,
    #[serde(default)]
    power_policy: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    oracle_dump: Option<String>,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config_path = std::env::args()
        .nth(1)
        .or_else(find_config)
        .unwrap_or_else(|| {
            tracing::error!("usage: coral-ember [config.toml]");
            tracing::error!("  no config found in XDG/system paths");
            std::process::exit(1);
        });

    let config_str = match std::fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(path = %config_path, error = %e, "failed to read config");
            std::process::exit(1);
        }
    };

    let config: EmberConfig = match toml::from_str(&config_str) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(path = %config_path, error = %e, "failed to parse config");
            std::process::exit(1);
        }
    };

    if config.device.is_empty() {
        tracing::error!("no devices configured — nothing to hold");
        std::process::exit(1);
    }

    let started_at = std::time::Instant::now();
    let mut held: HashMap<String, HeldDevice> = HashMap::new();

    // Pre-flight: disable dangerous reset methods for ALL managed devices.
    // This must happen at startup before any VFIO fds are opened, because
    // vfio-pci triggers a PCI reset when its last fd closes. If ember is
    // restarted (systemctl restart), the old process drops fds → reset.
    // Clearing reset_method now protects against that.
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
        std::process::exit(1);
    }

    if let Some(parent) = std::path::Path::new(EMBER_SOCKET).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::remove_file(EMBER_SOCKET);

    let listener = match UnixListener::bind(EMBER_SOCKET) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(path = EMBER_SOCKET, error = %e, "failed to bind ember socket");
            std::process::exit(1);
        }
    };

    let _ = std::fs::set_permissions(
        EMBER_SOCKET,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o660),
    );

    tracing::info!("╔══════════════════════════════════════════════════════════╗");
    tracing::info!("║ coral-ember — Immortal VFIO fd Holder                   ║");
    tracing::info!("╠══════════════════════════════════════════════════════════╣");
    for dev in held.values() {
        tracing::info!("║ {} (fd={})", dev.bdf, dev.device.device_fd());
    }
    tracing::info!("║ Socket: {EMBER_SOCKET}");
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
}

fn find_config() -> Option<String> {
    let xdg = std::env::var("XDG_CONFIG_HOME")
        .or_else(|_| std::env::var("HOME").map(|h| format!("{h}/.config")))
        .ok()
        .map(|base| format!("{base}/coralreef/glowplug.toml"));

    let candidates = ["/etc/coralreef/glowplug.toml"];

    for path in xdg
        .iter()
        .map(String::as_str)
        .chain(candidates.iter().copied())
    {
        if std::path::Path::new(path).exists() {
            tracing::info!(path, "found config");
            return Some(path.to_string());
        }
    }
    None
}
