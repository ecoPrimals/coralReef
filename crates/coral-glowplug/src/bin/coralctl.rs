// SPDX-License-Identifier: AGPL-3.0-only
//! coralctl — CLI companion for coral-glowplug and coral-ember.
//!
//! All device management commands go through glowplug's JSON-RPC socket.
//! No privilege escalation needed — the user just needs to be in the
//! `coralreef` group (socket is `root:coralreef 0660`).
//!
//! Subcommands:
//!   status        List all managed devices
//!   swap          Hot-swap a device to a new driver personality
//!   health        Query device health registers
//!   deploy-udev   Generate /dev/vfio/* udev rules from glowplug.toml
#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use coral_glowplug::config;
use coral_glowplug::sysfs;

/// Resolve the glowplug socket path from an optional env value.
///
/// `default_socket` passes `std::env::var("CORALREEF_GLOWPLUG_SOCKET").ok().as_deref()`.
fn resolve_glowplug_socket_path(env_value: Option<&str>) -> String {
    env_value
        .map(|s| s.to_owned())
        .unwrap_or_else(|| "/run/coralreef/glowplug.sock".into())
}

/// Default socket path, overridable via `$CORALREEF_GLOWPLUG_SOCKET`.
fn default_socket() -> String {
    resolve_glowplug_socket_path(std::env::var("CORALREEF_GLOWPLUG_SOCKET").ok().as_deref())
}

#[derive(Parser)]
#[command(
    name = "coralctl",
    about = "CLI companion for the coralReef GPU lifecycle system"
)]
struct Cli {
    /// Path to glowplug socket (override: `$CORALREEF_GLOWPLUG_SOCKET`).
    #[arg(long, default_value_t = default_socket(), global = true)]
    socket: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List all managed devices and their current personalities.
    Status,

    /// Hot-swap a device to a new driver personality.
    Swap {
        /// PCI BDF address (e.g. 0000:03:00.0).
        bdf: String,
        /// Target driver (vfio, nouveau, amdgpu, nvidia, xe, i915, unbound).
        target: String,
    },

    /// Query health registers for all managed devices.
    Health,

    /// Generate udev rules for /dev/vfio/* from glowplug.toml.
    DeployUdev {
        #[arg(short, long)]
        config: Option<String>,
        #[arg(
            short,
            long,
            default_value = "/etc/udev/rules.d/70-coralreef-vfio.rules"
        )]
        output: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value = "coralreef")]
        group: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Status => rpc_status(&cli.socket),
        Command::Swap { bdf, target } => rpc_swap(&cli.socket, &bdf, &target),
        Command::Health => rpc_health(&cli.socket),
        Command::DeployUdev {
            config: config_path,
            output,
            dry_run,
            group,
        } => {
            deploy_udev(config_path, &output, dry_run, &group);
        }
    }
}

// ---------------------------------------------------------------------------
// JSON-RPC client (pure std, no dependencies)
// ---------------------------------------------------------------------------

fn rpc_call(socket_path: &str, method: &str, params: serde_json::Value) -> serde_json::Value {
    use std::io::{BufRead, Write};
    use std::os::unix::net::UnixStream;

    let mut stream = match UnixStream::connect(socket_path) {
        Ok(s) => s,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                eprintln!("error: permission denied connecting to {socket_path}");
                eprintln!("hint: add yourself to the coralreef group:");
                eprintln!("  sudo groupadd -r coralreef");
                eprintln!("  sudo usermod -aG coralreef $USER");
                eprintln!("  newgrp coralreef  # or log out and back in");
            } else if e.kind() == std::io::ErrorKind::NotFound {
                eprintln!("error: socket not found at {socket_path}");
                eprintln!("hint: is coral-glowplug running?  systemctl status coral-glowplug");
            } else {
                eprintln!("error: failed to connect to {socket_path}: {e}");
            }
            std::process::exit(1);
        }
    };

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });

    let mut payload =
        serde_json::to_string(&request).expect("JSON Value serialization is infallible");
    payload.push('\n');

    if let Err(e) = stream.write_all(payload.as_bytes()) {
        eprintln!("error: failed to send RPC: {e}");
        std::process::exit(1);
    }

    let mut reader = std::io::BufReader::new(&stream);
    let mut response_line = String::new();
    if let Err(e) = reader.read_line(&mut response_line) {
        eprintln!("error: failed to read RPC response: {e}");
        std::process::exit(1);
    }

    match serde_json::from_str::<serde_json::Value>(&response_line) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: invalid JSON response: {e}");
            eprintln!("raw: {response_line}");
            std::process::exit(1);
        }
    }
}

fn check_rpc_error(response: &serde_json::Value) {
    if let Some(error) = response.get("error") {
        let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        let message = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        eprintln!("error [{code}]: {message}");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

fn rpc_status(socket: &str) {
    let response = rpc_call(socket, "device.list", serde_json::json!({}));
    check_rpc_error(&response);

    let result = match response.get("result") {
        Some(r) => r,
        None => {
            eprintln!("error: no result in response");
            std::process::exit(1);
        }
    };

    let devices = if result.is_array() {
        result.as_array()
    } else {
        result.get("devices").and_then(|d| d.as_array())
    };

    match devices {
        Some(devs) if !devs.is_empty() => {
            println!(
                "{:<16} {:<22} {:<6} {:<6} NAME",
                "BDF", "PERSONALITY", "POWER", "VRAM",
            );
            println!("{}", "-".repeat(70));
            for dev in devs {
                let bdf = dev.get("bdf").and_then(|v| v.as_str()).unwrap_or("?");
                let personality = dev
                    .get("personality")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let power = dev.get("power").and_then(|v| v.as_str()).unwrap_or("?");
                let vram = if dev
                    .get("vram_alive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    "ok"
                } else {
                    "-"
                };
                let name = dev.get("name").and_then(|v| v.as_str()).unwrap_or("");
                println!("{bdf:<16} {personality:<22} {power:<6} {vram:<6} {name}");
            }
        }
        _ => {
            println!("no devices managed");
        }
    }
}

fn rpc_swap(socket: &str, bdf: &str, target: &str) {
    println!("swapping {bdf} -> {target}...");

    let response = rpc_call(
        socket,
        "device.swap",
        serde_json::json!({
            "bdf": bdf,
            "target": target,
        }),
    );
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let personality = result
            .get("personality")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let vram = result
            .get("vram_alive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        println!("ok: {bdf} now on {personality} (vram_alive={vram})");
    }
}

fn rpc_health(socket: &str) {
    let response = rpc_call(socket, "health.check", serde_json::json!({}));
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let healthy = result
            .get("healthy")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let status = if healthy { "HEALTHY" } else { "DEGRADED" };
        println!("system: {status}");

        let devices = if result.is_array() {
            result.as_array()
        } else {
            result.get("devices").and_then(|d| d.as_array())
        };

        if let Some(devs) = devices {
            for dev in devs {
                let bdf = dev.get("bdf").and_then(|v| v.as_str()).unwrap_or("?");
                let vram = dev
                    .get("vram_alive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let power = dev.get("power").and_then(|v| v.as_str()).unwrap_or("?");
                let domains_alive = dev
                    .get("domains_alive")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let domains_faulted = dev
                    .get("domains_faulted")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let total = domains_alive + domains_faulted;
                let dev_status = if vram && domains_faulted == 0 {
                    "ok"
                } else {
                    "degraded"
                };
                println!(
                    "  {bdf}: {dev_status} (power={power}, vram={vram}, domains={domains_alive}/{total})"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// deploy-udev (existing)
// ---------------------------------------------------------------------------

fn deploy_udev(config_path: Option<String>, output: &str, dry_run: bool, group: &str) {
    let cfg = load_config(config_path);

    if cfg.device.is_empty() {
        eprintln!("error: no devices configured in glowplug.toml");
        std::process::exit(1);
    }

    let mut rules = String::new();
    rules.push_str("# Generated by coralctl deploy-udev — do not edit manually.\n");
    rules.push_str("# Re-run: coralctl deploy-udev\n\n");

    rules.push_str("# /dev/vfio/vfio (container fd) — needed by all VFIO users\n");
    rules.push_str(&format!(
        "SUBSYSTEM==\"vfio\", KERNEL==\"vfio\", GROUP=\"{group}\", MODE=\"0660\"\n\n"
    ));

    let mut seen_groups = std::collections::BTreeSet::new();

    for dev in &cfg.device {
        let group_id = sysfs::read_iommu_group(&dev.bdf);
        if group_id == 0 {
            eprintln!(
                "warning: {}: no IOMMU group found (device missing or IOMMU disabled), skipping",
                dev.bdf
            );
            continue;
        }

        if !seen_groups.insert(group_id) {
            continue;
        }

        let (vendor_id, device_id) = sysfs::read_pci_ids(&dev.bdf);
        let chip = sysfs::identify_chip(vendor_id, device_id);
        let name = dev.name.as_deref().unwrap_or(&chip);

        rules.push_str(&format!(
            "# {name} ({}) — IOMMU group {group_id}\n",
            dev.bdf
        ));
        rules.push_str(&format!(
            "SUBSYSTEM==\"vfio\", KERNEL==\"{group_id}\", GROUP=\"{group}\", MODE=\"0660\"\n\n"
        ));
    }

    if seen_groups.is_empty() {
        eprintln!("error: no IOMMU groups resolved — are devices present and IOMMU enabled?");
        std::process::exit(1);
    }

    if dry_run {
        print!("{rules}");
    } else {
        if let Some(parent) = std::path::Path::new(output).parent()
            && !parent.exists()
        {
            eprintln!(
                "error: parent directory {} does not exist",
                parent.display()
            );
            std::process::exit(1);
        }
        match std::fs::write(output, &rules) {
            Ok(()) => {
                eprintln!(
                    "wrote {} rules for {} IOMMU group(s) to {output}",
                    seen_groups.len(),
                    seen_groups.len()
                );
                eprintln!(
                    "reload udev: sudo udevadm control --reload-rules && sudo udevadm trigger"
                );
            }
            Err(e) => {
                eprintln!("error: failed to write {output}: {e}");
                eprintln!("hint: run with sudo or use --dry-run to preview");
                std::process::exit(1);
            }
        }
    }
}

fn try_load_config(config_path: Option<String>) -> Result<config::Config, Vec<String>> {
    let paths = if let Some(path) = config_path {
        vec![path]
    } else {
        config::config_search_paths()
    };

    for path in &paths {
        match config::Config::load(path) {
            Ok(cfg) => return Ok(cfg),
            Err(e) => {
                eprintln!("note: skipping {path}: {e}");
            }
        }
    }

    Err(paths)
}

fn load_config(config_path: Option<String>) -> config::Config {
    match try_load_config(config_path) {
        Ok(cfg) => cfg,
        Err(paths) => {
            eprintln!("error: no valid config found. Tried:");
            for path in &paths {
                eprintln!("  - {path}");
            }
            eprintln!("hint: pass --config <path> or create /etc/coralreef/glowplug.toml");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_glowplug_socket_path_matches_env_set_semantics() {
        const CUSTOM_SOCKET: &str = "/tmp/coralctl-test-glowplug.sock";
        assert_eq!(
            resolve_glowplug_socket_path(Some(CUSTOM_SOCKET)),
            CUSTOM_SOCKET
        );
    }

    #[test]
    fn resolve_glowplug_socket_path_matches_env_unset_semantics() {
        const FALLBACK_SOCKET: &str = "/run/coralreef/glowplug.sock";
        assert_eq!(resolve_glowplug_socket_path(None), FALLBACK_SOCKET);
    }

    #[test]
    fn cli_parses_custom_socket_for_status() {
        let cli = Cli::try_parse_from(["coralctl", "--socket", "/custom/glowplug.sock", "status"])
            .unwrap();
        assert_eq!(cli.socket, "/custom/glowplug.sock");
    }

    #[test]
    fn try_load_config_nonexistent_path_errors() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("definitely-missing-glowplug.toml");
        let err = try_load_config(Some(missing.to_string_lossy().into_owned())).unwrap_err();
        assert!(
            err.iter().any(|p| p.contains("definitely-missing")),
            "expected missing path in error list: {err:?}"
        );
    }
}
