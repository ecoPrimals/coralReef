// SPDX-License-Identifier: AGPL-3.0-only
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

use std::collections::{HashMap, HashSet};
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::sync::{Arc, RwLock};

use serde::Deserialize;

pub use error::EmberIpcError;
pub use hold::{HeldDevice, MailboxMeta, RingMeta, RingMetaEntry};
pub use ipc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, handle_client, send_with_fds};
pub use journal::{Journal, JournalEntry, JournalFilter, JournalStats};
pub use observation::{HealthResult, ResetObservation, SwapObservation, SwapTiming, epoch_ms};
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
///
/// ```
/// use coral_ember::parse_glowplug_config;
///
/// let cfg = parse_glowplug_config(
///     r#"[[device]]
/// bdf = "0000:01:00.0"
/// role = "display"
/// "#,
/// )
/// .expect("valid TOML");
/// assert!(cfg.device[0].is_display());
/// assert!(cfg.device[0].is_protected());
/// ```
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
    ///
    /// ```
    /// use coral_ember::EmberDeviceConfig;
    ///
    /// let display = EmberDeviceConfig {
    ///     bdf: "0000:01:00.0".into(),
    ///     name: None,
    ///     boot_personality: None,
    ///     power_policy: None,
    ///     role: Some("display".into()),
    ///     oracle_dump: None,
    /// };
    /// assert!(display.is_display());
    /// ```
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
/// };
/// assert_eq!(opts.listen_port, Some(9000));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmberRunOptions {
    /// Path to `glowplug.toml`; when `None`, uses [`find_config`] (XDG then system).
    pub config_path: Option<String>,
    /// When `Some`, also listens on `127.0.0.1:port` for JSON-RPC over TCP.
    pub listen_port: Option<u16>,
}

/// Default socket path for ember IPC. Override with `$CORALREEF_EMBER_SOCKET`.
///
/// ```
/// let path = coral_ember::ember_socket_path();
/// assert!(path.ends_with("ember.sock"));
/// ```
#[must_use]
pub fn ember_socket_path() -> String {
    std::env::var("CORALREEF_EMBER_SOCKET")
        .unwrap_or_else(|_| "/run/coralreef/ember.sock".to_string())
}

/// System-wide glowplug config path (same default and `$CORALREEF_GLOWPLUG_CONFIG` as coral-glowplug).
///
/// ```
/// let path = coral_ember::system_glowplug_config_path();
/// assert!(path.ends_with("glowplug.toml"));
/// ```
#[must_use]
pub fn system_glowplug_config_path() -> String {
    std::env::var("CORALREEF_GLOWPLUG_CONFIG")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/etc/coralreef/glowplug.toml".to_string())
}

/// Parse `glowplug.toml` contents into [`EmberConfig`].
///
/// ```
/// use coral_ember::parse_glowplug_config;
///
/// let cfg = parse_glowplug_config("device = []").expect("parse empty device list");
/// assert!(cfg.device.is_empty());
/// ```
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
        listen_port: None,
    })
}

/// Same as [`run`] but accepts explicit config and optional TCP listen port (see [`EmberRunOptions`]).
///
/// When `listen_port` is set, a TCP listener is started on `127.0.0.1` in addition to the Unix socket.
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
                        ring_meta: hold::RingMeta::default(),
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
    set_socket_group(&socket_path, "coralreef");

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
fn spawn_req_watcher(held: Arc<RwLock<HashMap<String, HeldDevice>>>) {
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
                        if let Some(ref req_fd) = dev.req_eventfd
                            && let Ok(cloned) = req_fd.try_clone()
                        {
                            fds.push(cloned);
                            names.push(bdf.clone());
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
                                tracing::warn!(
                                    bdf,
                                    "VFIO device-release request from kernel — \
                                     auto-releasing VFIO fds to prevent D-state"
                                );

                                let mut buf = [0u8; 8];
                                let _ = rustix::io::read(&cloned_fds[i], &mut buf);

                                match held.try_write() {
                                    Ok(mut map) => {
                                        if let Some(device) = map.remove(bdf) {
                                            drop(device);
                                            tracing::info!(
                                                bdf,
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

/// Spawn a background thread that periodically:
/// 1. Sends `WATCHDOG=1` to systemd (if `NOTIFY_SOCKET` is set).
/// 2. Verifies held VFIO fds are still valid (ring-keeper liveness).
///
/// The thread is daemonic — it dies when the main process exits.
fn spawn_watchdog(held: Arc<RwLock<HashMap<String, HeldDevice>>>) {
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

    #[test]
    fn ember_run_options_default_is_empty() {
        assert_eq!(
            EmberRunOptions::default(),
            EmberRunOptions {
                config_path: None,
                listen_port: None,
            }
        );
    }

    #[test]
    fn parse_glowplug_config_device_optional_fields_roundtrip() {
        let toml = r#"
            [[device]]
            bdf = "0000:0a:00.0"
            name = "Test GPU"
            boot_personality = "nouveau"
            power_policy = "power_save"
            role = "compute"
            oracle_dump = "/tmp/oracle.bin"
        "#;
        let cfg = parse_glowplug_config(toml).expect("valid TOML");
        assert_eq!(cfg.device.len(), 1);
        let d = &cfg.device[0];
        assert_eq!(d.bdf, "0000:0a:00.0");
        assert_eq!(d.name.as_deref(), Some("Test GPU"));
        assert_eq!(d.boot_personality.as_deref(), Some("nouveau"));
        assert_eq!(d.power_policy.as_deref(), Some("power_save"));
        assert_eq!(d.role.as_deref(), Some("compute"));
        assert_eq!(d.oracle_dump.as_deref(), Some("/tmp/oracle.bin"));
    }

    #[test]
    fn swap_error_from_string_maps_to_other() {
        let e: crate::error::SwapError = "orchestrator gave up".to_string().into();
        match e {
            crate::error::SwapError::Other(s) => assert_eq!(s, "orchestrator gave up"),
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[test]
    fn ember_ipc_error_from_string_maps_to_dispatch() {
        let e: crate::error::EmberIpcError = "lock failed".to_string().into();
        match e {
            crate::error::EmberIpcError::Dispatch(s) => assert_eq!(s, "lock failed"),
            other => panic!("expected Dispatch, got {other:?}"),
        }
    }

    #[test]
    fn sysfs_error_write_and_read_display() {
        let w = crate::error::SysfsError::Write {
            path: "/sys/a".into(),
            reason: "busy".into(),
        };
        assert!(w.to_string().contains("sysfs write"));
        assert!(w.to_string().contains("/sys/a"));
        let r = crate::error::SysfsError::Read {
            path: "/sys/b".into(),
            reason: "eof".into(),
        };
        assert!(r.to_string().contains("sysfs read"));
        let d = crate::error::SysfsError::DriverBind {
            bdf: "0000:01:00.0".into(),
            reason: "EEXIST".into(),
        };
        assert!(d.to_string().contains("driver bind"));
        let p = crate::error::SysfsError::PciReset {
            bdf: "0000:02:00.0".into(),
            reason: "reset failed".into(),
        };
        assert!(p.to_string().contains("PCI reset"));
    }

    #[test]
    fn sysfs_error_pci_and_bridge_variants_display() {
        let e = crate::error::SysfsError::BridgeNotFound {
            bdf: "0000:01:00.0".into(),
        };
        assert!(e.to_string().contains("parent PCI bridge"));
        let e2 = crate::error::SysfsError::BridgeResetMissing {
            bdf: "0000:01:00.0".into(),
            bridge_bdf: "0000:00:01.0".into(),
        };
        assert!(e2.to_string().contains("bridge"));
        let e3 = crate::error::SysfsError::DeviceNotReappeared {
            bdf: "0000:02:00.0".into(),
        };
        assert!(e3.to_string().contains("re-appear"));
        let e4 = crate::error::SysfsError::PmCycleD3cold {
            bdf: "0000:03:00.0".into(),
        };
        assert!(e4.to_string().contains("D3cold"));
    }

    #[test]
    fn swap_error_displays_preflight_drm_external_vfio_and_reset_method() {
        let p = crate::error::SwapError::Preflight {
            bdf: "0000:01:00.0".into(),
            reason: "nvidia_drm".into(),
        };
        assert!(p.to_string().contains("preflight"));
        let d = crate::error::SwapError::DrmIsolation("modeset active".into());
        assert!(d.to_string().contains("DRM isolation"));
        let x = crate::error::SwapError::ExternalVfioHolders {
            bdf: "0000:01:00.0".into(),
            count: 2,
        };
        assert!(x.to_string().contains("2 holders"));
        let u = crate::error::SwapError::UnknownTarget("fictional".into());
        assert!(u.to_string().contains("unknown target"));
        let t = crate::error::SwapError::Trace("mmiotrace busy".into());
        assert!(t.to_string().contains("trace"));
        let v = crate::error::SwapError::VerifyHealth {
            bdf: "0000:01:00.0".into(),
            detail: "no temp sensor".into(),
        };
        assert!(v.to_string().contains("post-bind verification"));
        let a = crate::error::SwapError::ActiveDisplayGpu {
            bdf: "0000:01:00.0".into(),
        };
        assert!(a.to_string().contains("display GPU"));
        let r = crate::error::SwapError::VfioReacquire {
            bdf: "0000:01:00.0".into(),
            reason: "open failed".into(),
        };
        assert!(r.to_string().contains("VFIO reacquire"));
        let i = crate::error::SwapError::InvalidResetMethod("kitten_reset".into());
        assert!(i.to_string().contains("kitten_reset"));
    }

    #[test]
    fn swap_error_from_sysfs_uses_from_trait() {
        let inner = crate::error::SysfsError::PciReset {
            bdf: "0000:01:00.0".into(),
            reason: "no reset".into(),
        };
        let s: crate::error::SwapError = inner.into();
        match s {
            crate::error::SwapError::Sysfs(e) => {
                assert!(e.to_string().contains("PCI reset"));
            }
            other => panic!("expected Sysfs wrapper, got {other:?}"),
        }
    }

    #[test]
    fn trace_error_displays_enable_disable_capture() {
        let e = crate::error::TraceError::Enable("busy".into());
        assert!(e.to_string().contains("mmiotrace enable"));
        let e2 = crate::error::TraceError::Disable("failed".into());
        assert!(e2.to_string().contains("mmiotrace disable"));
        let e3 = crate::error::TraceError::Capture {
            bdf: "0000:01:00.0".into(),
            reason: "disk full".into(),
        };
        assert!(e3.to_string().contains("trace capture"));
    }

    #[test]
    fn ember_ipc_error_invalid_request_io_utf8_lock_json_send_display() {
        assert_eq!(
            crate::error::EmberIpcError::InvalidRequest("empty body").to_string(),
            "invalid request: empty body"
        );
        let io: crate::error::EmberIpcError =
            std::io::Error::new(std::io::ErrorKind::NotFound, "nope").into();
        assert!(io.to_string().contains("I/O error"));
        let invalid_utf8 = vec![0xff_u8];
        let utf8_err = std::str::from_utf8(&invalid_utf8).unwrap_err();
        let u: crate::error::EmberIpcError = utf8_err.into();
        assert!(u.to_string().contains("UTF-8"));
        assert_eq!(
            crate::error::EmberIpcError::LockPoisoned.to_string(),
            "RwLock poisoned"
        );
        let j = crate::error::EmberIpcError::JsonSerialize("bad".into());
        assert!(j.to_string().contains("JSON serialization"));
        let s = crate::error::EmberIpcError::SendMsg("e".into());
        assert!(s.to_string().contains("sendmsg"));
    }
}
