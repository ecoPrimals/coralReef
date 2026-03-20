// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! coral-glowplug — Sovereign `PCIe` device lifecycle broker.
//!
//! Starts at boot, binds GPUs, holds VFIO fds open forever,
//! and exposes a Unix socket for ecosystem consumers (capability-based discovery).
//!
//! Usage:
//!   coral-glowplug --config `$XDG_CONFIG_HOME`/coralreef/glowplug.toml
//!   coral-glowplug --bdf 0000:4a:00.0              # single device, defaults
//!   coral-glowplug --bdf 0000:4a:00.0 --bdf 0000:03:00.0:nouveau

mod socket;

use clap::Parser;
use config::Config;
use coral_glowplug::{config, device, ember, health, pci_ids, sysfs};
use device::DeviceSlot;
use std::sync::Arc;
use tokio::sync::{Mutex, watch};

/// Sovereign `PCIe` device lifecycle broker — persistent GPU management daemon.
///
/// Binds GPUs at boot, holds VFIO fds open, and exposes a JSON-RPC
/// Unix socket for ecosystem consumers (capability-based discovery).
#[derive(Parser)]
#[command(name = "coral-glowplug", version, about)]
struct Cli {
    /// Path to TOML config file.
    #[arg(short, long)]
    config: Option<String>,

    /// PCI BDF address(es) to manage (e.g. 0000:4a:00.0 or 0000:4a:00.0:nouveau).
    #[arg(long)]
    bdf: Vec<String>,

    /// Auto-discover GPUs on the PCI bus.
    #[arg(long)]
    auto: bool,
}

fn parse_bdf_arg(arg: &str) -> config::DeviceConfig {
    let (bdf, personality) = arg
        .rfind(':')
        .filter(|_| arg.matches(':').count() > 2)
        .map(|pos| (&arg[..pos], &arg[pos + 1..]))
        .filter(|(_, tail)| {
            !tail.is_empty() && tail.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
        })
        .unwrap_or((arg, "vfio"));

    config::DeviceConfig {
        bdf: bdf.to_string(),
        name: None,
        boot_personality: personality.to_string(),
        power_policy: "always_on".into(),
        role: Some("compute".into()),
        oracle_dump: None,
    }
}

/// Check that the system boot configuration is safe for GPU management.
///
/// Warns at startup if kernel cmdline is missing `vfio-pci.ids`, or if
/// the nvidia module probed any of our managed devices (which corrupts
/// GV100 hardware state).
fn validate_boot_safety(config: &Config) {
    let cmdline = std::fs::read_to_string("/proc/cmdline").unwrap_or_else(|e| {
        tracing::warn!(error = %e, "failed to read /proc/cmdline — boot safety checks may be incomplete");
        String::new()
    });

    if !cmdline.contains("vfio-pci.ids") {
        tracing::warn!(
            "BOOT SAFETY: kernel cmdline is missing '{}'. \
             Without this, nvidia may probe Titan V GPUs before vfio-pci binds, \
             corrupting hardware state. Run: sudo kernelstub -a '{}'",
            pci_ids::TITAN_V_VFIO_IDS_CMDLINE,
            pci_ids::TITAN_V_VFIO_IDS_CMDLINE
        );
    }

    if std::path::Path::new("/sys/module/nvidia").exists() {
        for dev in &config.device {
            let driver_path = format!("/sys/bus/pci/devices/{}/driver", dev.bdf);
            if let Ok(link) = std::fs::read_link(&driver_path) {
                let driver_name = link.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if driver_name == "nvidia" {
                    tracing::error!(
                        bdf = %dev.bdf,
                        "BOOT SAFETY VIOLATION: nvidia module is bound to a managed device. \
                         The open nvidia.ko corrupts GV100 state (no GSP support). \
                         Ensure vfio-pci claims this device before nvidia loads."
                    );
                }
            }
        }

        let nvidia_probed_managed = config.device.iter().any(|dev| {
            let override_path = format!("/sys/bus/pci/devices/{}/driver_override", dev.bdf);
            let current_override = std::fs::read_to_string(&override_path).unwrap_or_default();
            current_override.trim() != "vfio-pci"
        });

        if nvidia_probed_managed {
            tracing::warn!(
                "BOOT SAFETY: nvidia module is loaded and not all managed devices have \
                 driver_override=vfio-pci. Ensure /etc/modprobe.d/coralreef-dual-titanv.conf \
                 contains 'softdep nvidia pre: vfio-pci' and 'options vfio-pci ids={}'",
                pci_ids::TITAN_V_VFIO_IDS
            );
        }
    }

    let vfio_ids_in_cmdline = cmdline.contains(pci_ids::TITAN_V_VFIO_IDS_CMDLINE)
        || cmdline.contains(pci_ids::TITAN_V_VFIO_IDS_CMDLINE_ALT);
    let nvidia_loaded = std::path::Path::new("/sys/module/nvidia").exists();
    let all_on_vfio = config.device.iter().all(|dev| {
        let driver_path = format!("/sys/bus/pci/devices/{}/driver", dev.bdf);
        std::fs::read_link(&driver_path)
            .ok()
            .and_then(|l| l.file_name().map(|n| n.to_string_lossy().into_owned()))
            .as_deref()
            == Some("vfio-pci")
    });

    if vfio_ids_in_cmdline && all_on_vfio {
        tracing::info!(
            "boot safety: OK — vfio-pci.ids in cmdline, all managed devices on vfio-pci"
        );
    } else if all_on_vfio {
        tracing::info!(
            "boot safety: devices on vfio-pci (cmdline param recommended for belt-and-suspenders)"
        );
    }

    if nvidia_loaded {
        tracing::debug!(
            "nvidia module loaded (for RTX display) — swap/resurrect operations \
             blocked on managed Volta devices as safety precaution"
        );
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    let config = if let Some(ref path) = cli.config {
        match Config::load(path) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(path, error = %e, "failed to load config");
                std::process::exit(1);
            }
        }
    } else if cli.auto {
        tracing::info!("auto-discovering GPUs on PCI bus");
        Config::auto_discover()
    } else if !cli.bdf.is_empty() {
        Config {
            daemon: config::DaemonConfig::default(),
            device: cli.bdf.iter().map(|a| parse_bdf_arg(a)).collect(),
        }
    } else {
        let candidates = config::config_search_paths();
        let loaded = candidates.iter().find_map(|p| {
            Config::load(p)
                .ok()
                .inspect(|_| tracing::info!(path = %p, "loaded config"))
        });

        loaded.unwrap_or_else(|| {
            tracing::error!("no config found — provide --config, --auto, or --bdf arguments");
            tracing::info!("config search paths:");
            for c in &candidates {
                tracing::info!("  {c}");
            }
            std::process::exit(2);
        })
    };

    tracing::info!(
        devices = config.device.len(),
        socket = %config.daemon.socket,
        "coral-glowplug starting"
    );

    validate_boot_safety(&config);

    let mut slots: Vec<DeviceSlot> = config
        .device
        .iter()
        .map(|dc| DeviceSlot::new(dc.clone()))
        .collect();

    // Try to connect to coral-ember for safe fd keepalive
    let ember_client = ember::EmberClient::connect();
    if let Some(ref client) = ember_client {
        match client.list_devices() {
            Ok(ember_devices) => {
                tracing::info!(
                    devices = ?ember_devices,
                    "ember is holding VFIO fds — daemon restarts are safe"
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, "ember reachable but list failed");
            }
        }
    } else {
        tracing::info!(
            "ember not available — VFIO fds will be opened directly \
             (daemon restart may trigger PM reset on GV100)"
        );
    }

    for slot in &mut slots {
        // Try ember first for safe fd management
        let ember_ok = if let Some(ref client) = ember_client {
            if slot.config.boot_personality == "vfio" {
                match client.request_fds(&slot.bdf) {
                    Ok(fds) => {
                        match slot.activate_from_ember(fds.container, fds.group, fds.device) {
                            Ok(()) => {
                                tracing::info!(
                                    bdf = %slot.bdf,
                                    chip = %slot.chip_name,
                                    "device ready (ember fds)"
                                );
                                true
                            }
                            Err(e) => {
                                tracing::warn!(
                                    bdf = %slot.bdf,
                                    error = %e,
                                    "ember fd activation failed — falling back to direct open"
                                );
                                false
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            bdf = %slot.bdf,
                            error = %e,
                            "ember fd request failed — falling back to direct open"
                        );
                        false
                    }
                }
            } else {
                false
            }
        } else {
            false
        };

        if !ember_ok {
            match slot.activate() {
                Ok(()) => tracing::info!(
                    bdf = %slot.bdf,
                    chip = %slot.chip_name,
                    personality = %slot.personality,
                    vram = slot.health.vram_alive,
                    "device ready"
                ),
                Err(e) => tracing::error!(
                    bdf = %slot.bdf,
                    error = %e,
                    "device activation failed"
                ),
            }
        }
    }

    let devices = Arc::new(Mutex::new(slots));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let health_devices = devices.clone();
    let health_interval = config.daemon.health_interval_ms;
    let mut health_shutdown = shutdown_rx.clone();
    tokio::spawn(async move {
        health::health_loop(health_devices, health_interval, &mut health_shutdown).await;
    });

    let server = match socket::SocketServer::bind(&config.daemon.socket).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(
                socket = %config.daemon.socket,
                error = %e,
                "failed to bind socket"
            );
            std::process::exit(1);
        }
    };

    {
        let device_lines: Vec<String> = {
            let devs = devices.lock().await;
            devs.iter()
                .map(|d| {
                    let vram = if d.health.vram_alive {
                        "VRAM ✓"
                    } else {
                        "VRAM ✗"
                    };
                    format!(
                        "║ {} {} ({}) {} {}",
                        d.bdf, d.chip_name, d.personality, vram, d.health.power
                    )
                })
                .collect()
        };
        tracing::info!("╔══════════════════════════════════════════════════════════╗");
        tracing::info!("║ coral-glowplug — Sovereign Device Broker                ║");
        tracing::info!("╠══════════════════════════════════════════════════════════╣");
        for line in &device_lines {
            tracing::info!("{line}");
        }
        tracing::info!("╠══════════════════════════════════════════════════════════╣");
        tracing::info!("║ Socket: {}", server.bound_addr());
        tracing::info!("║ Log level: {}", config.daemon.log_level);
        tracing::info!(
            "║ Health check: every {}ms",
            config.daemon.health_interval_ms
        );
        tracing::info!("╚══════════════════════════════════════════════════════════╝");
    }

    #[cfg(target_os = "linux")]
    if let Ok(ref path) = std::env::var("NOTIFY_SOCKET") {
        let _ = std::os::unix::net::UnixDatagram::unbound()
            .and_then(|sock| sock.send_to(b"READY=1", path));
    }

    let accept_devices = devices.clone();
    let mut accept_shutdown = shutdown_rx.clone();
    let accept_handle = tokio::spawn(async move {
        server
            .accept_loop(accept_devices, &mut accept_shutdown)
            .await;
    });

    let Ok(mut sigterm) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        tracing::error!("failed to register SIGTERM handler");
        std::process::exit(1);
    };
    let Ok(mut sigint) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
    else {
        tracing::error!("failed to register SIGINT handler");
        std::process::exit(1);
    };

    tokio::select! {
        Some(()) = sigterm.recv() => tracing::info!("received SIGTERM"),
        Some(()) = sigint.recv() => tracing::info!("received SIGINT"),
    }

    tracing::info!("shutting down — signalling background tasks to stop");
    let _ = shutdown_tx.send(true);

    // Give background tasks up to 2s to release the mutex gracefully
    accept_handle.abort();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let shutdown_timeout = std::time::Duration::from_secs(5);
    match tokio::time::timeout(shutdown_timeout, devices.lock()).await {
        Ok(mut devs) => {
            tracing::info!("disabling PCI resets and releasing devices");
            for slot in devs.iter_mut() {
                let _ = sysfs::sysfs_write(
                    &format!("/sys/bus/pci/devices/{}/reset_method", slot.bdf),
                    "",
                );
                let audio_bdf = format!("{}.1", &slot.bdf[..slot.bdf.len() - 1]);
                let _ = sysfs::sysfs_write(
                    &format!("/sys/bus/pci/devices/{audio_bdf}/reset_method"),
                    "",
                );

                let _ = sysfs::sysfs_write(
                    &format!("/sys/bus/pci/devices/{}/power/control", slot.bdf),
                    "on",
                );
                let _ = sysfs::sysfs_write(
                    &format!("/sys/bus/pci/devices/{}/d3cold_allowed", slot.bdf),
                    "0",
                );

                if slot.has_vfio() {
                    slot.snapshot_registers();
                    tracing::info!(bdf = %slot.bdf, "reset disabled, snapshot saved");
                }
            }
            devs.clear();
        }
        Err(_) => {
            tracing::error!("timed out acquiring device mutex during shutdown — forcing exit");
        }
    }

    tracing::info!("coral-glowplug stopped cleanly");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bdf_arg_simple() {
        let cfg = parse_bdf_arg("0000:01:00.0");
        assert_eq!(cfg.bdf, "0000:01:00.0");
        assert_eq!(cfg.boot_personality, "vfio");
        assert!(cfg.name.is_none());
    }

    #[test]
    fn test_parse_bdf_arg_with_personality() {
        let cfg = parse_bdf_arg("0000:4a:00.0:nouveau");
        assert_eq!(cfg.bdf, "0000:4a:00.0");
        assert_eq!(cfg.boot_personality, "nouveau");
    }

    #[test]
    fn test_parse_bdf_arg_with_amdgpu() {
        let cfg = parse_bdf_arg("0000:03:00.0:amdgpu");
        assert_eq!(cfg.bdf, "0000:03:00.0");
        assert_eq!(cfg.boot_personality, "amdgpu");
    }

    #[test]
    fn test_parse_bdf_arg_colon_in_bdf_no_personality() {
        // "0000:01:00.0" has 3 colons - no 4th segment, so default vfio
        let cfg = parse_bdf_arg("0000:01:00.0");
        assert_eq!(cfg.bdf, "0000:01:00.0");
        assert_eq!(cfg.boot_personality, "vfio");
    }

    #[test]
    fn test_parse_bdf_arg_numeric_suffix_ignored() {
        // If 4th segment has digits, it's not a personality (must be alphabetic).
        // Filter fails, so full arg is used as bdf with default vfio.
        let cfg = parse_bdf_arg("0000:01:00.0:1");
        assert_eq!(cfg.bdf, "0000:01:00.0:1");
        assert_eq!(cfg.boot_personality, "vfio");
    }
}
