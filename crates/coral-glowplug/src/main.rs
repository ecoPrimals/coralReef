// SPDX-License-Identifier: AGPL-3.0-only
//! coral-glowplug — Sovereign PCIe device lifecycle broker.
//!
//! Starts at boot, binds GPUs, holds VFIO fds open forever,
//! and exposes a Unix socket for toadStool to consume.
//!
//! Usage:
//!   coral-glowplug --config /etc/coralreef/glowplug.toml
//!   coral-glowplug --bdf 0000:4a:00.0              # single device, defaults
//!   coral-glowplug --bdf 0000:4a:00.0 --bdf 0000:03:00.0:nouveau

mod config;
mod device;
mod health;
mod socket;

use config::Config;
use device::DeviceSlot;
use std::sync::Arc;
use tokio::sync::Mutex;

fn parse_bdf_arg(arg: &str) -> config::DeviceConfig {
    // Handle BDF:personality format like "0000:03:00.0:nouveau"
    // BDF itself contains colons, so split on the last colon-word
    let (bdf, personality) = if arg.matches(':').count() > 2 {
        // More colons than a BDF has — last segment might be personality
        let last_colon = arg.rfind(':').unwrap();
        let candidate = &arg[last_colon + 1..];
        if candidate.chars().all(|c| c.is_ascii_alphabetic() || c == '-') && !candidate.is_empty() {
            (&arg[..last_colon], candidate)
        } else {
            (arg, "vfio")
        }
    } else {
        (arg, "vfio")
    };

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
    // Initialize tracing early so config loading can log
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    let config = if let Some(idx) = args.iter().position(|a| a == "--config") {
        let path = args.get(idx + 1).expect("--config requires a path");
        Config::load(path).expect("failed to load config")
    } else if args.iter().any(|a| a == "--auto") {
        tracing::info!("auto-discovering GPUs on PCI bus");
        Config::auto_discover()
    } else {
        // Build config from --bdf arguments
        let bdf_args: Vec<&String> = args.iter()
            .enumerate()
            .filter(|(_, a)| *a == "--bdf")
            .filter_map(|(i, _)| args.get(i + 1))
            .collect();

        if bdf_args.is_empty() {
            // Try standard config paths before giving up
            let candidates = [
                format!(
                    "{}/.config/coralreef/glowplug.toml",
                    std::env::var("HOME").unwrap_or_default()
                ),
                "/etc/coralreef/glowplug.toml".into(),
            ];
            let loaded = candidates.iter().find_map(|p| {
                Config::load(p).ok().map(|c| {
                    tracing::info!(path = %p, "loaded config");
                    c
                })
            });

            if let Some(c) = loaded {
                c
            } else {
                eprintln!("Usage:");
                eprintln!("  coral-glowplug --config /etc/coralreef/glowplug.toml");
                eprintln!("  coral-glowplug --auto                              # scan PCI bus");
                eprintln!("  coral-glowplug --bdf 0000:4a:00.0");
                eprintln!("  coral-glowplug --bdf 0000:4a:00.0 --bdf 0000:03:00.0:nouveau");
                eprintln!();
                eprintln!("Config search paths:");
                for c in &candidates {
                    eprintln!("  {c}");
                }
                std::process::exit(1);
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

    // Create device slots
    let mut slots: Vec<DeviceSlot> = config.device.iter()
        .map(|dc| DeviceSlot::new(dc.clone()))
        .collect();

    // Activate each device
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

    // Start health monitor
    let health_devices = devices.clone();
    let health_interval = config.daemon.health_interval_ms;
    tokio::spawn(async move {
        health::health_loop(health_devices, health_interval).await;
    });

    // Start socket server
    let server = socket::SocketServer::bind(&config.daemon.socket)
        .await
        .expect("failed to bind socket");

    // Print summary
    {
        let devs = devices.lock().await;
        eprintln!("╔══════════════════════════════════════════════════════════╗");
        eprintln!("║ coral-glowplug — Sovereign Device Broker                ║");
        eprintln!("╠══════════════════════════════════════════════════════════╣");
        for d in devs.iter() {
            let vram = if d.health.vram_alive { "VRAM ✓" } else { "VRAM ✗" };
            eprintln!("║ {} {} ({}) {} {}", d.bdf, d.chip_name, d.personality, vram, d.health.power);
        }
        eprintln!("╠══════════════════════════════════════════════════════════╣");
        eprintln!("║ Socket: {}", config.daemon.socket);
        eprintln!("║ Log level: {}", config.daemon.log_level);
        eprintln!("║ Health check: every {}ms", config.daemon.health_interval_ms);
        eprintln!("╚══════════════════════════════════════════════════════════╝");
    }

    // Notify systemd we're ready (if running under systemd)
    #[cfg(target_os = "linux")]
    {
        if std::env::var("NOTIFY_SOCKET").is_ok() {
            let _ = std::process::Command::new("systemd-notify")
                .arg("--ready")
                .status();
        }
    }

    // Accept connections forever
    // The VFIO fds in DeviceSlots stay open as long as the daemon runs
    server.accept_loop(devices).await;
}
