// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]
//! coral-glowplug — Sovereign PCIe device lifecycle broker.
//!
//! Starts at boot, binds GPUs, holds VFIO fds open forever,
//! and exposes a Unix socket for toadStool to consume.
//!
//! Usage:
//!   coral-glowplug --config $XDG_CONFIG_HOME/coralreef/glowplug.toml
//!   coral-glowplug --bdf 0000:4a:00.0              # single device, defaults
//!   coral-glowplug --bdf 0000:4a:00.0 --bdf 0000:03:00.0:nouveau

mod config;
mod device;
mod health;
mod personality;
mod socket;

use config::Config;
use device::DeviceSlot;
use std::sync::Arc;
use tokio::sync::Mutex;

fn parse_bdf_arg(arg: &str) -> config::DeviceConfig {
    // BDF format: "DDDD:BB:DD.F" (3 colons). If a 4th colon-word is present
    // and purely alphabetic, it's the personality suffix (e.g. ":nouveau").
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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    let config = if let Some(idx) = args.iter().position(|a| a == "--config") {
        let Some(path) = args.get(idx + 1) else {
            tracing::error!("--config requires a path argument");
            std::process::exit(2);
        };
        match Config::load(path) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(path, error = %e, "failed to load config");
                std::process::exit(1);
            }
        }
    } else if args.iter().any(|a| a == "--auto") {
        tracing::info!("auto-discovering GPUs on PCI bus");
        Config::auto_discover()
    } else {
        let bdf_args: Vec<&String> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "--bdf")
            .filter_map(|(i, _)| args.get(i + 1))
            .collect();

        if bdf_args.is_empty() {
            let xdg_config = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
                format!(
                    "{}/.config",
                    std::env::var("HOME").unwrap_or_else(|_| "/root".into()),
                )
            });
            let candidates = [
                format!("{xdg_config}/coralreef/glowplug.toml"),
                "/etc/coralreef/glowplug.toml".into(),
            ];
            let loaded = candidates.iter().find_map(|p| {
                Config::load(p)
                    .ok()
                    .inspect(|_| tracing::info!(path = %p, "loaded config"))
            });

            if let Some(c) = loaded {
                c
            } else {
                tracing::error!("no config found — provide --config, --auto, or --bdf arguments");
                tracing::info!("config search paths:");
                for c in &candidates {
                    tracing::info!("  {c}");
                }
                std::process::exit(2);
            }
        } else {
            Config {
                daemon: config::DaemonConfig::default(),
                device: bdf_args.iter().map(|a| parse_bdf_arg(a)).collect(),
            }
        }
    };

    tracing::info!(
        devices = config.device.len(),
        socket = %config.daemon.socket,
        "coral-glowplug starting"
    );

    let mut slots: Vec<DeviceSlot> = config
        .device
        .iter()
        .map(|dc| DeviceSlot::new(dc.clone()))
        .collect();

    for slot in &mut slots {
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

    let devices = Arc::new(Mutex::new(slots));

    let health_devices = devices.clone();
    let health_interval = config.daemon.health_interval_ms;
    tokio::spawn(async move {
        health::health_loop(health_devices, health_interval).await;
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
        let devs = devices.lock().await;
        tracing::info!("╔══════════════════════════════════════════════════════════╗");
        tracing::info!("║ coral-glowplug — Sovereign Device Broker                ║");
        tracing::info!("╠══════════════════════════════════════════════════════════╣");
        for d in devs.iter() {
            let vram = if d.health.vram_alive {
                "VRAM ✓"
            } else {
                "VRAM ✗"
            };
            tracing::info!(
                "║ {} {} ({}) {} {}",
                d.bdf,
                d.chip_name,
                d.personality,
                vram,
                d.health.power
            );
        }
        tracing::info!("╠══════════════════════════════════════════════════════════╣");
        tracing::info!("║ Socket: {}", config.daemon.socket);
        tracing::info!("║ Log level: {}", config.daemon.log_level);
        tracing::info!(
            "║ Health check: every {}ms",
            config.daemon.health_interval_ms
        );
        tracing::info!("╚══════════════════════════════════════════════════════════╝");
    }

    #[cfg(target_os = "linux")]
    {
        if std::env::var("NOTIFY_SOCKET").is_ok() {
            let _ = std::process::Command::new("systemd-notify")
                .arg("--ready")
                .status();
        }
    }

    let accept_devices = devices.clone();
    let accept_handle = tokio::spawn(async move {
        server.accept_loop(accept_devices).await;
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
        _ = sigterm.recv() => tracing::info!("received SIGTERM"),
        _ = sigint.recv() => tracing::info!("received SIGINT"),
    }

    tracing::info!("shutting down — disabling PCI resets and releasing devices");

    {
        let mut devs = devices.lock().await;
        for slot in devs.iter_mut() {
            device::sysfs_write(
                &format!("/sys/bus/pci/devices/{}/reset_method", slot.bdf),
                "",
            );
            let audio_bdf = format!("{}.1", &slot.bdf[..slot.bdf.len() - 1]);
            device::sysfs_write(
                &format!("/sys/bus/pci/devices/{audio_bdf}/reset_method"),
                "",
            );

            device::sysfs_write(
                &format!("/sys/bus/pci/devices/{}/power/control", slot.bdf),
                "on",
            );
            device::sysfs_write(
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

    accept_handle.abort();
    tracing::info!("coral-glowplug stopped cleanly");
}
