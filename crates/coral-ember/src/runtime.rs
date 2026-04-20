// SPDX-License-Identifier: AGPL-3.0-or-later
//! Daemon entry: load config, acquire VFIO holds, JSON-RPC accept loops (Unix + optional TCP).

use std::collections::{HashMap, HashSet};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::sync::{Arc, RwLock};

use crate::background::{arm_req_irq, spawn_req_watcher, spawn_watchdog};
use crate::config::{EmberRunOptions, find_config, parse_glowplug_config};
use crate::drm_isolation;
use crate::hold::HeldDevice;
use crate::ipc;
use crate::journal;
use crate::ring_meta::RingMeta;
use crate::sysfs;
use crate::vendor_lifecycle;

/// Try to `chgrp <group> <path>` so members of the group can connect
/// without sudo. Falls back silently if the group doesn't exist.
fn set_socket_group(path: &str, group_name: &str) {
    match std::process::Command::new("chgrp")
        .args([group_name, path])
        .output()
    {
        Ok(out) if out.status.success() => {
            tracing::info!(path, group = group_name, "socket group set");
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            tracing::debug!(path, group = group_name, %stderr, "chgrp failed (group may not exist)");
        }
        Err(e) => {
            tracing::debug!(path, group = group_name, error = %e, "chgrp command failed");
        }
    }
}

/// Entry point for the coral-ember daemon: load config, hold VFIO fds, serve JSON-RPC on the Unix socket.
///
/// Equivalent to [`run_with_options`] with a legacy first positional config path from [`std::env::args`]
/// and no TCP listen port.
///
/// On startup failure, returns `Err(exit_code)` (`2` for insecure+family guard, else typically `1`).
/// On success, blocks in the accept loop until the process is terminated.
///
/// # Errors
///
/// Returns `Err(2)` when the insecure/family guard fails. Returns `Err(1)` when configuration is
/// missing, invalid, empty, or VFIO setup fails.
pub fn run() -> Result<(), i32> {
    run_with_options(crate::EmberRunOptions {
        config_path: std::env::args().nth(1),
        listen_port: None,
    })
}

/// Same as [`run`] but accepts explicit config and optional TCP listen port (see [`EmberRunOptions`]).
///
/// When `listen_port` is set, a TCP listener is started on `127.0.0.1` in addition to the Unix socket.
/// ([`crate::EMBER_LISTEN_PORT_ENV`] names the conventional env var for documenting the chosen port; it is
/// not written by this crate — Rust 2024 treats concurrent `set_var` as `unsafe`.)
///
/// # Errors
///
/// Returns `Err(2)` when `BIOMEOS_INSECURE` is set together with a non-default
/// `BIOMEOS_FAMILY_ID` (wateringHole `PRIMAL_SELF_KNOWLEDGE_STANDARD` v1.1).
/// Returns `Err(1)` when configuration is missing, invalid, empty, or VFIO setup fails.
pub fn run_with_options(opts: EmberRunOptions) -> Result<(), i32> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    if let Err(e) = crate::config::validate_insecure_guard() {
        tracing::error!(error = %e, "configuration rejected");
        return Err(2);
    }

    let config_path = match opts.config_path.or_else(find_config) {
        Some(p) => p,
        None => {
            tracing::error!("usage: coral-ember server [--port PORT] [CONFIG.toml]");
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

    let config: crate::EmberConfig = match parse_glowplug_config(&config_str) {
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

    let compute_devices: Vec<&crate::EmberDeviceConfig> =
        config.device.iter().filter(|d| !d.is_protected()).collect();
    let display_devices: Vec<&crate::EmberDeviceConfig> =
        config.device.iter().filter(|d| d.is_protected()).collect();

    for dd in &display_devices {
        tracing::info!(
            bdf = %dd.bdf,
            name = dd.name.as_deref().unwrap_or("?"),
            "display GPU — skipping VFIO hold, setting driver_override"
        );
        sysfs::set_driver_override(&dd.bdf, "nvidia");
    }

    drm_isolation::ensure_drm_isolation(&config.device);

    let started_at = std::time::Instant::now();

    for dev_config in &compute_devices {
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

    let mut held_init: HashMap<String, HeldDevice> = HashMap::new();

    for dev_config in &compute_devices {
        tracing::info!(bdf = %dev_config.bdf, "opening VFIO device for ember hold");

        let group_id = sysfs::read_iommu_group(&dev_config.bdf);
        if group_id != 0 {
            sysfs::bind_iommu_group_to_vfio(&dev_config.bdf, group_id);
        }

        sysfs::pin_power(&dev_config.bdf);

        match coral_driver::vfio::VfioDevice::open(&dev_config.bdf) {
            Ok(device) => {
                let req_eventfd = arm_req_irq(&device, &dev_config.bdf);
                tracing::info!(
                    bdf = %dev_config.bdf,
                    backend = ?device.backend_kind(),
                    device_fd = device.device_fd(),
                    num_fds = device.sendable_fds().len(),
                    req_armed = req_eventfd.is_some(),
                    "VFIO device held by ember"
                );
                held_init.insert(
                    dev_config.bdf.clone(),
                    HeldDevice {
                        bdf: dev_config.bdf.clone(),
                        device,
                        ring_meta: RingMeta::default(),
                        req_eventfd,
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

    if held_init.is_empty() {
        tracing::error!("no devices held — ember cannot provide fd keepalive");
        return Err(1);
    }

    let held: Arc<RwLock<HashMap<String, HeldDevice>>> = Arc::new(RwLock::new(held_init));
    let managed_bdfs: Arc<HashSet<String>> = Arc::new(
        config
            .device
            .iter()
            .filter(|d| !d.is_protected())
            .map(|d| d.bdf.clone())
            .collect(),
    );

    let journal = Arc::new(journal::Journal::open_default());
    tracing::info!(path = %journal.path().display(), "experiment journal opened");

    let socket_path = crate::ember_socket_path();

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

    let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o660));
    let socket_group =
        std::env::var("CORALREEF_SOCKET_GROUP").unwrap_or_else(|_| "coralreef".into());
    set_socket_group(&socket_path, &socket_group);

    tracing::info!("╔══════════════════════════════════════════════════════════╗");
    tracing::info!("║ coral-ember — Immortal VFIO fd Holder (threaded)        ║");
    tracing::info!("╠══════════════════════════════════════════════════════════╣");
    if let Ok(map) = held.read() {
        for dev in map.values() {
            tracing::info!("║ {} (fd={})", dev.bdf, dev.device.device_fd());
        }
    }
    tracing::info!("║ Socket: {socket_path}");
    if let Some(port) = opts.listen_port {
        tracing::info!("║ TCP JSON-RPC: 127.0.0.1:{port} (vfio_fds unavailable over TCP)");
    }
    tracing::info!("╚══════════════════════════════════════════════════════════╝");

    if let Ok(ref path) = std::env::var("NOTIFY_SOCKET") {
        let _ = std::os::unix::net::UnixDatagram::unbound()
            .and_then(|sock| sock.send_to(b"READY=1", path));
    }

    spawn_watchdog(Arc::clone(&held));
    spawn_req_watcher(Arc::clone(&held));

    if let Some(port) = opts.listen_port {
        let tcp_host =
            std::env::var("CORALREEF_EMBER_TCP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let tcp_addr = format!("{tcp_host}:{port}");
        let tcp_listener = match TcpListener::bind(&tcp_addr) {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(addr = %tcp_addr, error = %e, "failed to bind ember TCP listen");
                return Err(1);
            }
        };

        let held_tcp = Arc::clone(&held);
        let managed_tcp = Arc::clone(&managed_bdfs);
        let journal_tcp = Arc::clone(&journal);
        let started_tcp = started_at;
        std::thread::Builder::new()
            .name("ember-tcp-accept".into())
            .spawn(move || {
                for stream in tcp_listener.incoming() {
                    match stream {
                        Ok(mut stream) => {
                            let first_byte = {
                                let mut buf = [0u8; 1];
                                stream
                                    .peek(&mut buf)
                                    .ok()
                                    .filter(|&n| n > 0)
                                    .map(|_| buf[0])
                            };
                            let outcome = crate::btsp::guard_from_first_byte(first_byte);
                            if !outcome.should_accept() {
                                tracing::warn!(?outcome, "BTSP rejected TCP connection");
                                drop(stream);
                                continue;
                            }
                            let held = Arc::clone(&held_tcp);
                            let managed = Arc::clone(&managed_tcp);
                            let journal = Arc::clone(&journal_tcp);
                            std::thread::spawn(move || {
                                if let Err(e) = ipc::handle_client_tcp(
                                    &mut stream,
                                    &held,
                                    managed.as_ref(),
                                    started_tcp,
                                    Some(&journal),
                                ) {
                                    tracing::warn!(error = %e, "ember TCP client handler error");
                                }
                            });
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "ember TCP accept error");
                        }
                    }
                }
            })
            .expect("spawn ember TCP accept thread");
    }

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let held = Arc::clone(&held);
                let managed = Arc::clone(&managed_bdfs);
                let journal = Arc::clone(&journal);
                std::thread::spawn(move || {
                    if let Err(e) = ipc::handle_client(
                        &mut stream,
                        &held,
                        managed.as_ref(),
                        started_at,
                        Some(&journal),
                    ) {
                        tracing::warn!(error = %e, "client handler error");
                    }
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, "accept error");
            }
        }
    }

    tracing::error!("ember accept loop ended unexpectedly");
    Err(1)
}
