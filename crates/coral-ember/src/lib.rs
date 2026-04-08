// SPDX-License-Identifier: AGPL-3.0-only
#![deny(unsafe_code)]
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
pub mod ecosystem;
pub(crate) mod error;
mod guarded_open;
mod hold;
#[allow(unsafe_code)]
pub mod isolation;
mod ipc;
pub mod journal;
pub mod observation;
pub mod pcie_armor;
mod swap;
mod sysfs;
pub mod trace;
pub(crate) mod vendor_lifecycle;

use std::collections::{HashMap, HashSet};
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use serde::Deserialize;

pub use hold::{DeviceHealth, HeldDevice, MailboxMeta, RingMeta, RingMetaEntry, all_faulted, check_voluntary_death};
pub use ipc::handlers_policy::{BootPolicy, PolicyStore, new_policy_store};
pub use ipc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, handle_client, send_with_fds};
pub use journal::{Journal, JournalEntry, JournalFilter, JournalStats};
pub use observation::{
    HealthResult, ResetObservation, SwapObservation, SwapTiming, epoch_ms,
};
pub use coral_driver::vfio::gpu_vendor::FirmwareState;
pub use swap::{
    handle_swap_device, handle_swap_device_with_journal, verify_drm_isolation_with_paths,
};
pub use vendor_lifecycle::{
    RebindStrategy, ResetMethod, VendorLifecycle, detect_lifecycle, detect_lifecycle_for_target,
};

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

impl EmberDeviceConfig {
    /// Returns `true` if this device has `role = "display"`, meaning it is a
    /// protected display GPU that ember must never touch, unbind, or manage.
    #[must_use]
    pub fn is_display(&self) -> bool {
        self.role.as_deref() == Some("display")
    }

    /// Returns `true` if this device has `role = "shared"` — serves both display and compute.
    #[must_use]
    pub fn is_shared(&self) -> bool {
        self.role.as_deref() == Some("shared")
    }

    /// Returns `true` if this device is protected from driver swaps (display or shared).
    #[must_use]
    pub fn is_protected(&self) -> bool {
        self.is_display() || self.is_shared()
    }
}

/// Environment variable for the optional TCP JSON-RPC listen port (set when `--port` is used).
pub const EMBER_LISTEN_PORT_ENV: &str = "CORALREEF_EMBER_PORT";

/// Options for [`run_with_options`] (UniBin `server` entry).
///
/// ```
/// use coral_ember::EmberRunOptions;
///
/// let opts = EmberRunOptions {
///     config_path: Some("/etc/coralreef/glowplug.toml".into()),
///     listen_port: Some(9000),
///     resurrect: false,
///     glowplug_socket: None,
/// };
/// assert_eq!(opts.listen_port, Some(9000));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmberRunOptions {
    /// Path to `glowplug.toml`; when `None`, uses [`find_config`] (XDG then system).
    pub config_path: Option<String>,
    /// When `Some`, also listens on `$CORALREEF_EMBER_TCP_HOST:port` (default `127.0.0.1`) for JSON-RPC over TCP.
    pub listen_port: Option<u16>,
    /// When `true`, ember does not open VFIO devices from sysfs. Instead it
    /// connects to glowplug's fd vault and receives VFIO fds via SCM_RIGHTS.
    /// This allows ember to restart without triggering PM reset.
    pub resurrect: bool,
    /// Glowplug socket path for resurrection. When `None`, uses the default
    /// glowplug socket path.
    pub glowplug_socket: Option<String>,
    /// When `Some`, filter the config to hold only this single BDF.
    /// Used by fleet mode: glowplug spawns one ember per device via
    /// `coral-ember server --bdf 0000:03:00.0`.
    pub single_bdf: Option<String>,
    /// When true, this ember starts with no devices and waits for an
    /// `ember.adopt_device` RPC to receive vault fds from glowplug.
    pub standby: bool,
}

/// Default socket path for ember IPC. Override with `$CORALREEF_EMBER_SOCKET`.
///
/// Follows the wateringHole IPC standard: `$XDG_RUNTIME_DIR/biomeos/coral-ember-<family>.sock`.
///
/// Family ID resolution: `$BIOMEOS_FAMILY_ID` (wateringHole canonical) with
/// fallback to `$CORALREEF_FAMILY_ID` / `$FAMILY_ID` for backward compatibility.
///
/// ```
/// let path = coral_ember::ember_socket_path();
/// assert!(path.ends_with("ember.sock"));
/// ```
#[must_use]
pub fn ember_socket_path() -> String {
    if let Ok(p) = std::env::var("CORALREEF_EMBER_SOCKET") {
        if !p.is_empty() {
            return p;
        }
    }
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let family = std::env::var("BIOMEOS_FAMILY_ID")
        .or_else(|_| std::env::var("CORALREEF_FAMILY_ID"))
        .or_else(|_| std::env::var("FAMILY_ID"))
        .unwrap_or_else(|_| "default".to_string());
    format!("{runtime_dir}/biomeos/coral-ember-{family}.sock")
}

/// Convert a BDF like `0000:03:00.0` into a filesystem-safe slug like `0000-03-00.0`.
#[must_use]
pub fn bdf_to_slug(bdf: &str) -> String {
    bdf.replace(':', "-")
}

/// Convert a slug like `0000-03-00.0` back to canonical BDF `0000:03:00.0`.
///
/// Accepts both formats — if already colon-separated, returns as-is.
#[must_use]
pub fn slug_to_bdf(slug: &str) -> String {
    if slug.contains(':') {
        return slug.to_string();
    }
    let mut out = slug.to_string();
    // PCI BDF has colons at positions 4 and 7: 0000:03:00.0
    if out.len() >= 10 {
        out.replace_range(4..5, ":");
        out.replace_range(7..8, ":");
    }
    out
}

/// Socket path for a per-device ember instance in fleet mode.
///
/// Returns `/run/coralreef/fleet/ember-{slug}.sock` where slug is the BDF with
/// colons replaced by hyphens. Uses a `fleet/` subdirectory to survive
/// glowplug restarts (the parent `/run/coralreef/` may be recreated).
#[must_use]
pub fn ember_instance_socket_path(bdf: &str) -> String {
    format!("/run/coralreef/fleet/ember-{}.sock", bdf_to_slug(bdf))
}

/// Socket path for a hot-standby ember instance.
#[must_use]
pub fn ember_standby_socket_path(index: usize) -> String {
    format!("/run/coralreef/fleet/ember-standby-{index}.sock")
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
/// On startup failure, returns `Err(exit_code)` (typically `1`). On success, blocks in the accept
/// loop until the process is terminated.
///
/// # Errors
///
/// Returns `Err(1)` when configuration is missing, invalid, empty, or VFIO setup fails.
pub fn run() -> Result<(), i32> {
    run_with_options(EmberRunOptions {
        config_path: std::env::args().nth(1),
        ..Default::default()
    })
}

/// Same as [`run`] but accepts explicit config and optional TCP listen port (see [`EmberRunOptions`]).
///
/// When `listen_port` is set, a TCP listener is started on `$CORALREEF_EMBER_TCP_HOST` (default `127.0.0.1`) in addition to the Unix socket.
/// ([`EMBER_LISTEN_PORT_ENV`] names the conventional env var for documenting the chosen port; it is
/// not written by this crate — Rust 2024 treats concurrent `set_var` as `unsafe`.)
///
/// # Errors
///
/// Returns `Err(1)` when configuration is missing, invalid, empty, or VFIO setup fails.
pub fn run_with_options(opts: EmberRunOptions) -> Result<(), i32> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

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

    let mut config: EmberConfig = match parse_glowplug_config(&config_str) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(path = %config_path, error = %e, "failed to parse config");
            return Err(1);
        }
    };

    if opts.single_bdf.is_some() {
        let raw_bdf = opts.single_bdf.as_deref().unwrap();
        let bdf = slug_to_bdf(raw_bdf);
        config.device.retain(|d| d.bdf == bdf);
        if config.device.is_empty() {
            tracing::error!(%bdf, raw = raw_bdf, "BDF not found in config — nothing to hold");
            return Err(1);
        }
        tracing::info!(%bdf, "fleet mode: single-device ember");
    }

    if opts.standby {
        tracing::info!("HOT-STANDBY MODE — no devices, waiting for ember.adopt_device");
        config.device.clear();
    } else if config.device.is_empty() {
        tracing::error!("no devices configured — nothing to hold");
        return Err(1);
    }

    let compute_devices: Vec<&EmberDeviceConfig> =
        config.device.iter().filter(|d| !d.is_protected()).collect();
    let display_devices: Vec<&EmberDeviceConfig> =
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
    let mut held_init: HashMap<String, HeldDevice> = HashMap::new();
    let mut deferred_bdfs: Vec<String> = Vec::new();

    if opts.resurrect {
        // ── RESURRECTION PATH ──
        // Receive VFIO fds from glowplug's fd vault instead of opening from sysfs.
        tracing::info!("RESURRECT MODE — requesting VFIO fds from glowplug vault");

        let glowplug_socket = opts.glowplug_socket.clone().unwrap_or_else(default_glowplug_socket);

        match resurrect_from_vault(&glowplug_socket, &compute_devices) {
            Ok(devices) => {
                for (bdf, device) in devices {
                    let armor = pcie_armor::PcieArmor::arm(&bdf);
                    let req_eventfd = arm_req_irq(&device, &bdf);
                    tracing::info!(
                        bdf = %bdf,
                        backend = ?device.backend_kind(),
                        "RESURRECTED device from vault (PCIe armor active)"
                    );
                    held_init.insert(
                        bdf.clone(),
                        HeldDevice {
                            bdf,
                            device,
                            bar0: None,
                            ring_meta: hold::RingMeta::default(),
                            req_eventfd,
                            experiment_dirty: false,
                            needs_warm_cycle: false,
                            dma_prepare_state: None,
                            mmio_fault_count: 0,
                            health: hold::DeviceHealth::Alive,
                            pcie_armor: Some(armor),
                            teardown_policy: Default::default(),
                        },
                    );
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "resurrection failed — falling back to sysfs acquisition");
                // Fall through to normal acquisition
            }
        }
    }

    if !opts.resurrect || held_init.is_empty() {
        // ── NORMAL SYSFS ACQUISITION PATH ──
        for dev_config in &compute_devices {
            if held_init.contains_key(&dev_config.bdf) {
                continue;
            }

            tracing::info!(bdf = %dev_config.bdf, "acquiring VFIO device for ember hold");

            let bdf = dev_config.bdf.clone();
            let (tx, rx) = std::sync::mpsc::channel();

            std::thread::Builder::new()
                .name(format!("ember-acquire-{bdf}"))
                .spawn({
                    let bdf = bdf.clone();
                    move || {
                        let lifecycle = vendor_lifecycle::detect_lifecycle(&bdf);
                        let cold_sensitive = lifecycle.is_cold_sensitive();

                        let current = sysfs::read_current_driver(&bdf);
                        if let Some(ref drv) = current {
                            if let Err(e) = lifecycle.prepare_for_unbind(&bdf, drv) {
                                tracing::warn!(
                                    bdf = %bdf, error = %e,
                                    "startup: prepare_for_unbind failed (non-fatal)"
                                );
                            }
                        } else {
                            sysfs::pin_power(&bdf);
                        }

                        let group_id = sysfs::read_iommu_group(&bdf);
                        if group_id != 0 {
                            sysfs::bind_iommu_group_to_vfio(&bdf, group_id);
                        }

                        sysfs::pin_power(&bdf);

                        let armor = pcie_armor::PcieArmor::arm(&bdf);

                        let open_result =
                            coral_driver::vfio::VfioDevice::open(&bdf)
                                .map_err(|e| e.to_string());

                        let _ = tx.send((open_result, cold_sensitive, armor));
                    }
                })
                .expect("failed to spawn device acquire thread");

            match rx.recv_timeout(guarded_open::GUARDED_OPEN_TIMEOUT) {
                Ok((Ok(device), _cold_sensitive, armor)) => {
                    match device.map_bar(0) {
                        Ok(bar0) => {
                            let ptr = bar0.base_ptr() as usize;
                            let sz = bar0.size();
                            let bdf_q = dev_config.bdf.clone();
                            let qr = isolation::fork_isolated_mmio(
                                &bdf_q,
                                std::time::Duration::from_secs(2),
                                |_pipe_fd| {
                                    #[allow(unsafe_code)]
                                    let b = unsafe {
                                        coral_driver::vfio::device::MappedBar::from_raw(
                                            ptr as *mut u8, sz,
                                        )
                                    };
                                    coral_driver::vfio::device::dma_safety::post_swap_quiesce(&b);
                                    std::mem::forget(b);
                                },
                            );
                            if matches!(qr, isolation::ForkResult::Timeout) {
                                tracing::error!(bdf = %dev_config.bdf, "startup: post_swap_quiesce TIMED OUT — device may be degraded");
                            }
                        }
                        Err(e) => {
                            tracing::warn!(bdf = %dev_config.bdf, error = %e, "startup: BAR0 map failed for post-swap quiesce");
                        }
                    }

                    let req_eventfd = arm_req_irq(&device, &dev_config.bdf);
                    tracing::info!(
                        bdf = %dev_config.bdf,
                        backend = ?device.backend_kind(),
                        device_fd = device.device_fd(),
                        num_fds = device.sendable_fds().len(),
                        req_armed = req_eventfd.is_some(),
                        "VFIO device held by ember (post-swap quiesce applied, PCIe armor active)"
                    );
                    held_init.insert(
                        dev_config.bdf.clone(),
                        HeldDevice {
                            bdf: dev_config.bdf.clone(),
                            device,
                            bar0: None,
                            ring_meta: hold::RingMeta::default(),
                            req_eventfd,
                            experiment_dirty: false,
                            needs_warm_cycle: false,
                            dma_prepare_state: None,
                            mmio_fault_count: 0,
                            health: hold::DeviceHealth::Alive,
                            pcie_armor: Some(armor),
                            teardown_policy: Default::default(),
                        },
                    );
                }
                Ok((Err(e), cold_sensitive, _armor)) => {
                    tracing::warn!(
                        bdf = %dev_config.bdf,
                        cold_sensitive,
                        error = %e,
                        "VFIO open failed — device deferred (use ember.open_device to retry)"
                    );
                    deferred_bdfs.push(dev_config.bdf.clone());
                }
                Err(_timeout) => {
                    tracing::error!(
                        bdf = %dev_config.bdf,
                        timeout_secs = guarded_open::GUARDED_OPEN_TIMEOUT.as_secs(),
                        "device acquire TIMED OUT — sysfs or VFIO open stuck (D-state). \
                         Thread leaked. Device deferred."
                    );
                    deferred_bdfs.push(dev_config.bdf.clone());
                }
            }
        }
    }

    if !deferred_bdfs.is_empty() {
        tracing::info!(
            deferred = ?deferred_bdfs,
            "devices deferred at startup (timeout/cold/busy — retry via ember.open_device)"
        );
    }

    if held_init.is_empty() && deferred_bdfs.is_empty() {
        tracing::error!("no devices held or deferred — ember cannot provide fd keepalive");
        return Err(1);
    }
    if held_init.is_empty() {
        tracing::warn!(
            "no devices held at startup (all deferred) — \
             ember is running but cannot serve fds until devices become available"
        );
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

    let policies = ipc::handlers_policy::new_policy_store();

    let socket_path = if let Some(ref bdf) = opts.single_bdf {
        ember_instance_socket_path(bdf)
    } else if opts.standby {
        let idx = std::env::var("CORALREEF_STANDBY_INDEX")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        ember_standby_socket_path(idx)
    } else {
        ember_socket_path()
    };

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
    set_socket_group(&socket_path, "coralreef");

    let device_count = held.read().map(|m| m.len()).unwrap_or(0);
    let tcp_bind_str = opts.listen_port.map(|port| {
        let host =
            std::env::var("CORALREEF_EMBER_TCP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        format!("{host}:{port}")
    });
    ecosystem::write_discovery_file(
        &socket_path,
        tcp_bind_str.as_deref(),
        device_count,
    );

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
        let tcp_host =
            std::env::var("CORALREEF_EMBER_TCP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        tracing::info!("║ TCP JSON-RPC: {tcp_host}:{port} (vfio_fds unavailable over TCP)");
    }
    tracing::info!("╚══════════════════════════════════════════════════════════╝");

    if let Ok(ref path) = std::env::var("NOTIFY_SOCKET") {
        let _ = std::os::unix::net::UnixDatagram::unbound()
            .and_then(|sock| sock.send_to(b"READY=1", path));
    }

    isolation::install_sigterm_handler();
    spawn_watchdog(Arc::clone(&held), socket_path.clone());

    let warm_cycling: Arc<std::sync::Mutex<std::collections::HashSet<String>>> =
        Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    spawn_req_watcher(Arc::clone(&held), Arc::clone(&warm_cycling));

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
        let policies_tcp = Arc::clone(&policies);
        let warm_cycling_tcp = Arc::clone(&warm_cycling);
        let started_tcp = started_at;
        std::thread::Builder::new()
            .name("ember-tcp-accept".into())
            .spawn(move || {
                for stream in tcp_listener.incoming() {
                    match stream {
                        Ok(mut stream) => {
                            let held = Arc::clone(&held_tcp);
                            let managed = Arc::clone(&managed_tcp);
                            let journal = Arc::clone(&journal_tcp);
                            let policies = Arc::clone(&policies_tcp);
                            let warm_cycling = Arc::clone(&warm_cycling_tcp);
                            std::thread::spawn(move || {
                                if let Err(e) = ipc::handle_client_tcp(
                                    &mut stream,
                                    &held,
                                    managed.as_ref(),
                                    started_tcp,
                                    Some(&journal),
                                    &policies,
                                    &warm_cycling,
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

    let active_connections = Arc::new(AtomicUsize::new(0));

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let current = active_connections.load(Ordering::Relaxed);
                if current >= MAX_CONCURRENT_CLIENTS {
                    tracing::warn!(
                        active = current,
                        max = MAX_CONCURRENT_CLIENTS,
                        "overloaded — rejecting connection"
                    );
                    let overload = serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32000,
                            "message": "ember overloaded — try again later"
                        },
                        "id": null
                    });
                    let _ = std::io::Write::write_all(
                        &mut stream,
                        format!("{overload}\n").as_bytes(),
                    );
                    continue;
                }
                active_connections.fetch_add(1, Ordering::Relaxed);

                let held = Arc::clone(&held);
                let managed = Arc::clone(&managed_bdfs);
                let journal = Arc::clone(&journal);
                let policies = Arc::clone(&policies);
                let warm_cycling = Arc::clone(&warm_cycling);
                let conn_counter = Arc::clone(&active_connections);
                std::thread::spawn(move || {
                    if let Err(e) = ipc::handle_client(
                        &mut stream,
                        &held,
                        managed.as_ref(),
                        started_at,
                        Some(&journal),
                        &policies,
                        &warm_cycling,
                    ) {
                        tracing::warn!(error = %e, "client handler error");
                    }
                    conn_counter.fetch_sub(1, Ordering::Relaxed);
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

/// Arm `VFIO_PCI_REQ_ERR_IRQ` (index 4) on a VFIO device.
///
/// When armed, the kernel signals this eventfd instead of printing
/// "No device request channel registered, blocked until released by user".
/// The [`spawn_req_watcher`] thread monitors all active eventfds and
/// auto-releases the VFIO fd before the kernel enters D-state.
fn arm_req_irq(device: &coral_driver::vfio::VfioDevice, bdf: &str) -> Option<std::os::fd::OwnedFd> {
    use coral_driver::vfio::irq::{VfioIrqIndex, arm_irq_eventfd};

    match arm_irq_eventfd(device.device_as_fd(), VfioIrqIndex::Req, 0) {
        Ok(fd) => {
            tracing::info!(bdf, "VFIO REQ IRQ armed — kernel can signal device release");
            Some(fd)
        }
        Err(e) => {
            tracing::warn!(
                bdf,
                error = %e,
                "failed to arm VFIO REQ IRQ (non-fatal — external unbind may D-state)"
            );
            None
        }
    }
}

/// Spawn a background thread that monitors all VFIO device request eventfds.
///
/// When the kernel signals a REQ IRQ (because someone wrote to `driver/unbind`
/// while ember holds the fd), this thread auto-releases the VFIO device from
/// the `held` map. This prevents the kernel from blocking indefinitely in
/// `wait_for_completion()` inside `vfio_unregister_group_dev()`, which is the
/// root cause of D-state cascades on Kepler and other FLR-lacking GPUs.
///
/// The thread rebuilds its poll set each cycle from `try_clone()`'d eventfds,
/// so it remains correct as devices are added/removed from the held map.
fn spawn_req_watcher(
    held: Arc<RwLock<HashMap<String, HeldDevice>>>,
    warm_cycling: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
) {
    use rustix::event::{PollFd, PollFlags, poll};
    use rustix::time::Timespec;

    std::thread::Builder::new()
        .name("ember-req-watcher".into())
        .spawn(move || {
            loop {
                let (cloned_fds, bdfs): (Vec<std::os::fd::OwnedFd>, Vec<String>) = {
                    let map = match held.read() {
                        Ok(m) => m,
                        Err(_) => break,
                    };
                    let mut fds = Vec::new();
                    let mut names = Vec::new();
                    for (bdf, dev) in map.iter() {
                        if let Some(ref req_fd) = dev.req_eventfd {
                            if let Ok(cloned) = req_fd.try_clone() {
                                fds.push(cloned);
                                names.push(bdf.clone());
                            }
                        }
                    }
                    (fds, names)
                };

                if cloned_fds.is_empty() {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    continue;
                }

                let mut poll_fds: Vec<PollFd<'_>> = cloned_fds
                    .iter()
                    .map(|fd| PollFd::new(fd, PollFlags::IN))
                    .collect();

                let timeout = Timespec {
                    tv_sec: 1,
                    tv_nsec: 0,
                };
                match poll(&mut poll_fds, Some(&timeout)) {
                    Ok(n) if n > 0 => {
                        for (i, pfd) in poll_fds.iter().enumerate() {
                            let revents = pfd.revents();
                            if revents.contains(PollFlags::IN) {
                                let bdf = &bdfs[i];

                                let mut buf = [0u8; 8];
                                let _ = rustix::io::read(&cloned_fds[i], &mut buf);

                                if warm_cycling.lock().map(|s| s.contains(bdf.as_str())).unwrap_or(false) {
                                    tracing::info!(
                                        bdf,
                                        "VFIO REQ IRQ during warm cycle — suppressed (expected)"
                                    );
                                    continue;
                                }

                                tracing::warn!(
                                    bdf,
                                    "VFIO device-release request from kernel — \
                                     auto-releasing VFIO fds to prevent D-state"
                                );

                                match held.try_write() {
                                    Ok(mut map) => {
                                        if let Some(held) = map.remove(bdf) {
                                            let bdf_owned = bdf.to_string();
                                            drop(map);
                                            guarded_open::guarded_vfio_close(held.device, &bdf_owned);
                                            tracing::info!(
                                                bdf = %bdf_owned,
                                                "device auto-released (kernel REQ IRQ)"
                                            );
                                        }
                                    }
                                    Err(_) => {
                                        tracing::warn!(
                                            bdf,
                                            "held lock busy — will retry auto-release \
                                             on next poll cycle"
                                        );
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            tracing::info!("req-watcher thread exiting");
        })
        .expect("spawn device request watcher thread");
}

/// Default watchdog interval in seconds (half a typical `WatchdogSec=30`).
const WATCHDOG_INTERVAL_SECS: u64 = 15;

/// Maximum concurrent client connections before overload response.
///
/// Protects ember from RPC floods that would exhaust threads/fds.
/// Excess connections receive a JSON-RPC error and are closed immediately.
const MAX_CONCURRENT_CLIENTS: usize = 32;

/// Spawn a background thread that periodically:
/// 1. Sends `WATCHDOG=1` to systemd (if `NOTIFY_SOCKET` is set).
/// 2. Verifies held VFIO fds are still valid (ring-keeper liveness).
/// 3. Checks the SIGTERM shutdown flag and performs graceful cleanup.
///
/// The thread is daemonic — it dies when the main process exits.
fn spawn_watchdog(held: Arc<RwLock<HashMap<String, HeldDevice>>>, socket_path: String) {
    let interval = std::env::var("CORALREEF_EMBER_WATCHDOG_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(WATCHDOG_INTERVAL_SECS);

    let notify_path = std::env::var("NOTIFY_SOCKET").ok();

    std::thread::Builder::new()
        .name("ember-watchdog".into())
        .spawn(move || {
            let interval = std::time::Duration::from_secs(interval);
            loop {
                std::thread::sleep(interval);

                if isolation::shutdown_requested() {
                    tracing::info!("watchdog: SIGTERM received — initiating graceful shutdown");
                    // Zero-IO shutdown: remove socket only (local fs op, safe).
                    // Bus master disable is skipped — if the GPU is wedged,
                    // sysfs writes stall the watchdog thread and cascade to
                    // a full system lockup. Glowplug's resurrection cycle
                    // handles bus master cleanup after ember exits.
                    let _ = std::fs::remove_file(&socket_path);
                    tracing::info!("watchdog: socket removed — aborting (zero device I/O)");
                    std::process::abort();
                }

                let device_count = held.read().map(|map| map.len()).unwrap_or(0);

                if device_count == 0 {
                    tracing::warn!("watchdog: no devices held — ring-keeper degraded");
                }

                if let Some(ref path) = notify_path {
                    let _ = std::os::unix::net::UnixDatagram::unbound()
                        .and_then(|sock| sock.send_to(b"WATCHDOG=1", path));
                }

                tracing::trace!(devices = device_count, "watchdog: heartbeat");
            }
        })
        .expect("spawn ember watchdog thread");
}

/// Default glowplug socket path for resurrection fd retrieval.
fn default_glowplug_socket() -> String {
    if let Ok(p) = std::env::var("CORALREEF_GLOWPLUG_SOCKET") {
        if !p.is_empty() {
            return p;
        }
    }
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let family = std::env::var("BIOMEOS_FAMILY_ID")
        .or_else(|_| std::env::var("CORALREEF_FAMILY_ID"))
        .or_else(|_| std::env::var("FAMILY_ID"))
        .unwrap_or_else(|_| "default".to_string());
    format!("{runtime_dir}/biomeos/coral-glowplug-{family}.sock")
}

/// Receive VFIO fds from glowplug's fd vault to resurrect held devices.
///
/// Connects to glowplug's socket, calls `vault.restore_fds`, and
/// reconstructs `VfioDevice` instances from the received fds.
fn resurrect_from_vault(
    glowplug_socket: &str,
    expected_devices: &[&EmberDeviceConfig],
) -> Result<Vec<(String, coral_driver::vfio::VfioDevice)>, String> {
    use std::mem::MaybeUninit;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;

    let stream = UnixStream::connect(glowplug_socket)
        .map_err(|e| format!("connect to glowplug at {glowplug_socket}: {e}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "vault.restore_fds",
        "params": {
            "bdfs": expected_devices.iter().map(|d| &d.bdf).collect::<Vec<_>>(),
        },
        "id": 1,
    });
    std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())
        .map_err(|e| format!("write request: {e}"))?;

    const MAX_FDS: usize = 32;
    let mut buf = [0u8; 8192];
    let mut iov = [rustix::io::IoSliceMut::new(&mut buf)];
    let mut recv_space =
        [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(MAX_FDS))];
    let mut control = rustix::net::RecvAncillaryBuffer::new(&mut recv_space);
    let msg = rustix::net::recvmsg(&stream, &mut iov, &mut control, rustix::net::RecvFlags::empty())
        .map_err(|e| format!("recvmsg: {e}"))?;

    let mut fds: Vec<OwnedFd> = Vec::new();
    for ancillary in control.drain() {
        if let rustix::net::RecvAncillaryMessage::ScmRights(iter) = ancillary {
            fds.extend(iter);
        }
    }

    let resp: serde_json::Value = serde_json::from_slice(&buf[..msg.bytes])
        .map_err(|e| format!("parse response: {e}"))?;

    if let Some(err) = resp.get("error") {
        return Err(format!(
            "glowplug vault error: {}",
            err.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
        ));
    }

    let result = resp.get("result").cloned().unwrap_or_default();
    let manifest: Vec<serde_json::Value> = result
        .get("devices")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();

    let mut devices = Vec::new();
    let mut fd_iter = fds.into_iter();

    for dev in &manifest {
        let bdf = dev.get("bdf").and_then(|b| b.as_str()).unwrap_or("");
        let backend = dev
            .get("backend")
            .and_then(|b| b.as_str())
            .unwrap_or("legacy");
        let num_fds = dev.get("num_fds").and_then(|n| n.as_u64()).unwrap_or(3) as usize;

        let received = match backend {
            "iommufd" => {
                let iommufd = fd_iter.next().ok_or("not enough fds for iommufd")?;
                let device = fd_iter.next().ok_or("not enough fds for iommufd")?;
                let ioas_id = dev.get("ioas_id").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                coral_driver::vfio::ReceivedVfioFds::Iommufd {
                    iommufd,
                    device,
                    ioas_id,
                }
            }
            _ => {
                let container = fd_iter.next().ok_or("not enough fds for legacy")?;
                let group = fd_iter.next().ok_or("not enough fds for legacy")?;
                let device = fd_iter.next().ok_or("not enough fds for legacy")?;
                coral_driver::vfio::ReceivedVfioFds::Legacy {
                    container,
                    group,
                    device,
                }
            }
        };

        let vfio_device = coral_driver::vfio::VfioDevice::from_received(bdf, received)
            .map_err(|e| format!("reconstruct VfioDevice for {bdf}: {e}"))?;

        tracing::info!(bdf, backend, num_fds, "resurrected device from vault");
        devices.push((bdf.to_string(), vfio_device));
    }

    Ok(devices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_device(role: Option<&str>) -> EmberDeviceConfig {
        EmberDeviceConfig {
            bdf: "0000:01:00.0".to_string(),
            name: None,
            boot_personality: None,
            power_policy: None,
            role: role.map(|s| s.to_string()),
            oracle_dump: None,
        }
    }

    #[test]
    fn ember_device_config_is_display_only_for_display_role() {
        let mut d = sample_device(None);
        assert!(!d.is_display());
        d.role = Some("compute".to_string());
        assert!(!d.is_display());
        d.role = Some("display".to_string());
        assert!(d.is_display());
    }

    #[test]
    fn ember_device_config_is_shared_only_for_shared_role() {
        let mut d = sample_device(None);
        assert!(!d.is_shared());
        d.role = Some("shared".to_string());
        assert!(d.is_shared());
        d.role = Some("display".to_string());
        assert!(!d.is_shared());
    }

    #[test]
    fn ember_device_config_is_protected_for_display_or_shared() {
        let mut d = sample_device(None);
        assert!(!d.is_protected());
        d.role = Some("compute".to_string());
        assert!(!d.is_protected());
        d.role = Some("display".to_string());
        assert!(d.is_protected());
        d.role = Some("shared".to_string());
        assert!(d.is_protected());
    }

    #[test]
    fn parse_glowplug_config_roles_roundtrip() {
        let toml = r#"
            [[device]]
            bdf = "0000:01:00.0"
            role = "display"

            [[device]]
            bdf = "0000:02:00.0"
            role = "shared"
        "#;
        let cfg = parse_glowplug_config(toml).expect("valid glowplug TOML");
        assert_eq!(cfg.device.len(), 2);
        assert!(cfg.device[0].is_display());
        assert!(cfg.device[1].is_shared());
        assert!(!cfg.device[1].is_display());
    }

    #[test]
    fn parse_glowplug_config_invalid_returns_error() {
        assert!(
            parse_glowplug_config("[[device]]\n bdf =").is_err(),
            "truncated device table must not parse"
        );
    }

    #[test]
    fn parse_glowplug_config_empty_device_list() {
        let cfg = parse_glowplug_config("device = []").expect("valid empty device list");
        assert!(cfg.device.is_empty());
    }
}
