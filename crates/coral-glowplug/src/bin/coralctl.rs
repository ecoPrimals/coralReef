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
//!   probe         Dump all BAR0 registers for a device
//!   vram-probe    Check HBM2/VRAM accessibility via PRAMIN
//!   mmio          Read or write a single BAR0 register
//!   snapshot      Save or diff register snapshots
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

/// Default path for generated VFIO udev rules (`$CORALREEF_UDEV_RULES_PATH` overrides).
fn default_udev_rules_path() -> String {
    std::env::var("CORALREEF_UDEV_RULES_PATH")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/etc/udev/rules.d/70-coralreef-vfio.rules".to_string())
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

    /// Trigger a PCIe Function Level Reset (FLR) on a device via VFIO.
    ///
    /// Recovers from corrupted GPU state (e.g. wrong firmware applied)
    /// without a full system reboot. Requires the device to be VFIO-bound.
    Reset {
        /// PCI BDF address (e.g. 0000:4a:00.0).
        bdf: String,
    },

    /// Query health registers for all managed devices.
    Health,

    /// Dump all BAR0 registers for a device (comprehensive register probe).
    Probe {
        /// PCI BDF address (e.g. 0000:03:00.0).
        bdf: String,
    },

    /// Check HBM2/VRAM accessibility via the PRAMIN window.
    VramProbe {
        /// PCI BDF address (e.g. 0000:03:00.0).
        bdf: String,
    },

    /// Read or write a single BAR0 register.
    Mmio {
        #[command(subcommand)]
        action: MmioAction,
    },

    /// Save or diff register snapshots.
    Snapshot {
        #[command(subcommand)]
        action: SnapshotAction,
    },

    /// MMU page table oracle — capture full PT chain or diff two captures.
    Oracle {
        #[command(subcommand)]
        action: OracleAction,
    },

    /// Query compute capabilities for a GPU (NVML telemetry via nvidia-smi).
    ComputeInfo {
        /// PCI BDF address (e.g. 0000:21:00.0).
        bdf: String,
    },

    /// Query or set compute quota for a shared/display GPU.
    ComputeQuota {
        /// PCI BDF address (e.g. 0000:21:00.0).
        bdf: String,
        /// Set power limit (watts).
        #[arg(long)]
        power_limit: Option<u32>,
        /// Set compute mode (default, exclusive_process, prohibited).
        #[arg(long)]
        compute_mode: Option<String>,
        /// Set VRAM budget (MiB) — advisory.
        #[arg(long)]
        vram_budget: Option<u32>,
    },

    /// Submit compute work through the daemon pipeline (shader + buffers).
    Dispatch {
        /// PCI BDF address of the target GPU (e.g. 0000:21:00.0).
        bdf: String,
        /// Path to PTX shader file.
        #[arg(long)]
        shader: String,
        /// Input buffer files (raw binary, order = kernel arg order).
        #[arg(long)]
        input: Vec<String>,
        /// Output buffer sizes in bytes.
        #[arg(long)]
        output_size: Vec<u64>,
        /// Workgroup grid dimensions (X,Y,Z). Default: "256,1,1".
        #[arg(long, default_value = "256,1,1")]
        workgroups: String,
        /// Threads per workgroup (X,Y,Z). Default: "64,1,1".
        #[arg(long, default_value = "64,1,1")]
        threads: String,
        /// Write output buffers to files (output_0.bin, output_1.bin, ...).
        #[arg(long)]
        output_dir: Option<String>,
    },

    /// Generate udev rules for /dev/vfio/* from glowplug.toml.
    DeployUdev {
        #[arg(short, long)]
        config: Option<String>,
        #[arg(short, long, default_value_t = default_udev_rules_path())]
        output: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value = "coralreef")]
        group: String,
    },
}

#[derive(Subcommand)]
enum MmioAction {
    /// Read a single BAR0 register.
    Read {
        /// PCI BDF address.
        bdf: String,
        /// Register offset (hex: 0x1234 or decimal).
        offset: String,
    },
    /// Write a single BAR0 register.
    Write {
        /// PCI BDF address.
        bdf: String,
        /// Register offset (hex: 0x1234 or decimal).
        offset: String,
        /// Value to write (hex: 0xDEADBEEF or decimal).
        value: String,
        /// Allow writes to dangerous registers (e.g. PMC_ENABLE).
        #[arg(long)]
        allow_dangerous: bool,
    },
}

#[derive(Subcommand)]
enum SnapshotAction {
    /// Save a register snapshot to a JSON file.
    Save {
        /// PCI BDF address.
        bdf: String,
        /// Output file path (default: <BDF>_snapshot_<timestamp>.json).
        file: Option<String>,
    },
    /// Compare current registers against a saved snapshot.
    Diff {
        /// PCI BDF address.
        bdf: String,
        /// Path to a previously saved snapshot JSON file.
        file: String,
    },
}

#[derive(Subcommand)]
enum OracleAction {
    /// Capture full MMU page table chain + engine registers from a GPU.
    Capture {
        /// PCI BDF address (e.g. 0000:03:00.0).
        bdf: String,
        /// Output JSON file path (default: stdout).
        #[arg(short, long)]
        output: Option<String>,
        /// Maximum channels to walk (0 = all found).
        #[arg(long, default_value_t = 0)]
        max_channels: usize,
        /// Bypass the daemon and capture directly (requires VFIO group access).
        #[arg(long)]
        local: bool,
    },
    /// Compare two oracle capture JSON files.
    Diff {
        /// Left (reference) capture file.
        left: String,
        /// Right (comparison) capture file.
        right: String,
    },
}

fn parse_hex_or_dec(s: &str) -> Result<u64, String> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|e| format!("invalid hex '{s}': {e}"))
    } else {
        s.parse::<u64>().map_err(|e| format!("invalid number '{s}': {e}"))
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Status => rpc_status(&cli.socket),
        Command::Swap { bdf, target } => rpc_swap(&cli.socket, &bdf, &target),
        Command::Reset { bdf } => rpc_reset(&cli.socket, &bdf),
        Command::Health => rpc_health(&cli.socket),
        Command::Probe { bdf } => rpc_probe(&cli.socket, &bdf),
        Command::VramProbe { bdf } => rpc_vram_probe(&cli.socket, &bdf),
        Command::Mmio { action } => match action {
            MmioAction::Read { bdf, offset } => {
                let off = match parse_hex_or_dec(&offset) {
                    Ok(v) => v as usize,
                    Err(e) => {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    }
                };
                rpc_mmio_read(&cli.socket, &bdf, off);
            }
            MmioAction::Write {
                bdf,
                offset,
                value,
                allow_dangerous,
            } => {
                let off = match parse_hex_or_dec(&offset) {
                    Ok(v) => v as usize,
                    Err(e) => {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    }
                };
                let val = match parse_hex_or_dec(&value) {
                    Ok(v) => v as u32,
                    Err(e) => {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    }
                };
                rpc_mmio_write(&cli.socket, &bdf, off, val, allow_dangerous);
            }
        },
        Command::Snapshot { action } => match action {
            SnapshotAction::Save { bdf, file } => rpc_snapshot_save(&cli.socket, &bdf, file),
            SnapshotAction::Diff { bdf, file } => rpc_snapshot_diff(&cli.socket, &bdf, &file),
        },
        Command::Oracle { action } => match action {
            OracleAction::Capture {
                bdf,
                output,
                max_channels,
                local,
            } => {
                if local {
                    oracle_capture_local(&bdf, output.as_deref(), max_channels);
                } else {
                    oracle_capture_rpc(&cli.socket, &bdf, output.as_deref(), max_channels);
                }
            }
            OracleAction::Diff { left, right } => oracle_diff(&left, &right),
        },
        Command::ComputeInfo { bdf } => rpc_compute_info(&cli.socket, &bdf),
        Command::ComputeQuota {
            bdf,
            power_limit,
            compute_mode,
            vram_budget,
        } => {
            if power_limit.is_some() || compute_mode.is_some() || vram_budget.is_some() {
                rpc_set_quota(&cli.socket, &bdf, power_limit, compute_mode.as_deref(), vram_budget);
            } else {
                rpc_get_quota(&cli.socket, &bdf);
            }
        }
        Command::Dispatch {
            bdf,
            shader,
            input,
            output_size,
            workgroups,
            threads,
            output_dir,
        } => {
            rpc_dispatch(
                &cli.socket,
                &bdf,
                &shader,
                &input,
                &output_size,
                &workgroups,
                &threads,
                output_dir.as_deref(),
            );
        }
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
                let protected = dev
                    .get("protected")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let suffix = if protected {
                    format!("{name} [PROTECTED]")
                } else {
                    name.to_string()
                };
                println!("{bdf:<16} {personality:<22} {power:<6} {vram:<6} {suffix}");
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

fn rpc_reset(socket: &str, bdf: &str) {
    println!("resetting {bdf} via VFIO FLR...");
    let response = rpc_call(
        socket,
        "device.reset",
        serde_json::json!({"bdf": bdf}),
    );
    check_rpc_error(&response);
    println!("ok: {bdf} reset complete");
}

fn rpc_compute_info(socket: &str, bdf: &str) {
    let response = rpc_call(
        socket,
        "device.compute_info",
        serde_json::json!({"bdf": bdf}),
    );
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let chip = result.get("chip").and_then(|v| v.as_str()).unwrap_or("?");
        let role = result
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let protected = result
            .get("protected")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let render = result
            .get("render_node")
            .and_then(|v| v.as_str())
            .unwrap_or("none");

        println!("{bdf}  {chip}  role={role}{}", if protected { " [PROTECTED]" } else { "" });
        println!("  Render Node: {render}");

        if let Some(c) = result.get("compute") {
            if let Some(err) = c.get("error") {
                println!("  Compute: unavailable ({})", err.as_str().unwrap_or("?"));
            } else {
                let name = c.get("gpu_name").and_then(|v| v.as_str()).unwrap_or("?");
                let mem_total = c.get("memory_total_mib").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let mem_free = c.get("memory_free_mib").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let mem_used = c.get("memory_used_mib").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let temp = c.get("temperature_c").and_then(|v| v.as_u64()).unwrap_or(0);
                let power = c.get("power_draw_w").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let power_limit = c.get("power_limit_w").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let sm = c.get("clock_sm_mhz").and_then(|v| v.as_u64()).unwrap_or(0);
                let mem_clk = c.get("clock_mem_mhz").and_then(|v| v.as_u64()).unwrap_or(0);
                let cc = c.get("compute_cap").and_then(|v| v.as_str()).unwrap_or("?");
                let pcie = c.get("pcie_width").and_then(|v| v.as_u64()).unwrap_or(0);

                println!("  GPU:         {name}");
                println!("  Compute Cap: {cc}");
                println!("  Memory:      {mem_used:.0} / {mem_total:.0} MiB ({mem_free:.0} MiB free)");
                println!("  Temperature: {temp}C");
                println!("  Power:       {power:.1}W / {power_limit:.0}W");
                println!("  Clocks:      SM {sm} MHz, Mem {mem_clk} MHz");
                println!("  PCIe Width:  x{pcie}");
            }
        }
    }
}

fn rpc_get_quota(socket: &str, bdf: &str) {
    let response = rpc_call(socket, "device.quota", serde_json::json!({"bdf": bdf}));
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let role = result.get("role").and_then(|v| v.as_str()).unwrap_or("?");
        let protected = result.get("protected").and_then(|v| v.as_bool()).unwrap_or(false);
        println!("{bdf}  role={role}{}", if protected { " [PROTECTED]" } else { "" });

        if let Some(q) = result.get("quota") {
            let pl = q.get("power_limit_w").and_then(|v| v.as_u64());
            let vb = q.get("vram_budget_mib").and_then(|v| v.as_u64());
            let cm = q.get("compute_mode").and_then(|v| v.as_str()).unwrap_or("default");
            let cp = q.get("compute_priority").and_then(|v| v.as_u64()).unwrap_or(0);
            println!("  Quota:");
            println!("    Power Limit:  {}", pl.map_or("default".to_string(), |w| format!("{w}W")));
            println!("    VRAM Budget:  {}", vb.map_or("unlimited".to_string(), |m| format!("{m} MiB")));
            println!("    Compute Mode: {cm}");
            println!("    Priority:     {cp}");
        }

        if let Some(c) = result.get("current") {
            if c.get("error").is_none() {
                let mem_used = c.get("memory_used_mib").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let mem_total = c.get("memory_total_mib").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let power = c.get("power_draw_w").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let power_limit = c.get("power_limit_w").and_then(|v| v.as_f64()).unwrap_or(0.0);
                println!("  Current:");
                println!("    Memory:       {mem_used:.0} / {mem_total:.0} MiB");
                println!("    Power:        {power:.1}W / {power_limit:.0}W");
            }
        }
    }
}

fn rpc_set_quota(socket: &str, bdf: &str, power_limit: Option<u32>, compute_mode: Option<&str>, vram_budget: Option<u32>) {
    let mut params = serde_json::json!({"bdf": bdf});
    if let Some(pl) = power_limit {
        params["power_limit_w"] = serde_json::json!(pl);
    }
    if let Some(cm) = compute_mode {
        params["compute_mode"] = serde_json::json!(cm);
    }
    if let Some(vb) = vram_budget {
        params["vram_budget_mib"] = serde_json::json!(vb);
    }

    let response = rpc_call(socket, "device.set_quota", params);
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        println!("Quota updated for {bdf}");
        if let Some(q) = result.get("quota") {
            let pl = q.get("power_limit_w").and_then(|v| v.as_u64());
            let vb = q.get("vram_budget_mib").and_then(|v| v.as_u64());
            let cm = q.get("compute_mode").and_then(|v| v.as_str()).unwrap_or("default");
            println!("  Power Limit:  {}", pl.map_or("default".to_string(), |w| format!("{w}W")));
            println!("  VRAM Budget:  {}", vb.map_or("unlimited".to_string(), |m| format!("{m} MiB")));
            println!("  Compute Mode: {cm}");
        }
        if let Some(applied) = result.get("applied") {
            for (key, val) in applied.as_object().into_iter().flatten() {
                let ok = val.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
                let msg = val.get("message").and_then(|v| v.as_str()).unwrap_or("");
                let status = if ok { "OK" } else { "FAILED" };
                println!("  {key}: [{status}] {msg}");
            }
        }
    }
}

fn parse_triple(s: &str) -> [u32; 3] {
    let parts: Vec<u32> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
    [
        parts.first().copied().unwrap_or(1),
        parts.get(1).copied().unwrap_or(1),
        parts.get(2).copied().unwrap_or(1),
    ]
}

#[expect(clippy::too_many_arguments)]
fn rpc_dispatch(
    socket: &str,
    bdf: &str,
    shader_path: &str,
    input_paths: &[String],
    output_sizes: &[u64],
    workgroups: &str,
    threads: &str,
    output_dir: Option<&str>,
) {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;

    let shader_bytes = std::fs::read(shader_path).unwrap_or_else(|e| {
        eprintln!("error: cannot read shader {shader_path}: {e}");
        std::process::exit(1);
    });
    let shader_b64 = b64.encode(&shader_bytes);

    let inputs_b64: Vec<String> = input_paths
        .iter()
        .map(|p| {
            let data = std::fs::read(p).unwrap_or_else(|e| {
                eprintln!("error: cannot read input {p}: {e}");
                std::process::exit(1);
            });
            b64.encode(&data)
        })
        .collect();

    let dims = parse_triple(workgroups);
    let wg = parse_triple(threads);

    let params = serde_json::json!({
        "bdf": bdf,
        "shader": shader_b64,
        "inputs": inputs_b64,
        "output_sizes": output_sizes,
        "dims": dims,
        "workgroup": wg,
    });

    eprintln!(
        "dispatching on {bdf}: shader={shader_path} inputs={} outputs={} grid={}x{}x{} block={}x{}x{}",
        input_paths.len(),
        output_sizes.len(),
        dims[0], dims[1], dims[2],
        wg[0], wg[1], wg[2],
    );

    let response = rpc_call(socket, "device.dispatch", params);
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let outputs = result
            .get("outputs")
            .and_then(|v| v.as_array())
            .unwrap_or(&Vec::new())
            .clone();
        eprintln!("dispatch complete: {} output buffer(s)", outputs.len());

        for (i, out) in outputs.iter().enumerate() {
            if let Some(encoded) = out.as_str() {
                let data = b64.decode(encoded).unwrap_or_else(|e| {
                    eprintln!("error: base64 decode output {i}: {e}");
                    std::process::exit(1);
                });
                eprintln!("  output[{i}]: {} bytes", data.len());

                if let Some(dir) = output_dir {
                    let path = format!("{dir}/output_{i}.bin");
                    std::fs::write(&path, &data).unwrap_or_else(|e| {
                        eprintln!("error: write {path}: {e}");
                        std::process::exit(1);
                    });
                    eprintln!("  written to {path}");
                }
            }
        }
    }
}

fn rpc_health(socket: &str) {
    let response = rpc_call(socket, "health.check", serde_json::json!({}));
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let alive = result
            .get("alive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let device_count = result
            .get("device_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let healthy_count = result
            .get("healthy_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let status = if alive && healthy_count == device_count {
            "HEALTHY"
        } else if alive {
            "DEGRADED"
        } else {
            "DOWN"
        };
        println!("system: {status}  ({healthy_count}/{device_count} devices healthy)");

        if !alive {
            println!("  daemon reports not alive");
        }
    }
}

// ---------------------------------------------------------------------------
// Diagnostic subcommand implementations
// ---------------------------------------------------------------------------

fn rpc_probe(socket: &str, bdf: &str) {
    let response = rpc_call(
        socket,
        "device.register_dump",
        serde_json::json!({ "bdf": bdf }),
    );
    check_rpc_error(&response);

    let result = match response.get("result") {
        Some(r) => r,
        None => {
            eprintln!("error: no result in response");
            std::process::exit(1);
        }
    };

    let regs = result
        .get("registers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let count = result
        .get("register_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    println!("=== Register Probe: {bdf} ({count} registers) ===");
    for reg in &regs {
        let offset = reg
            .get("offset")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let value = reg.get("value").and_then(|v| v.as_str()).unwrap_or("?");
        println!("  {offset} = {value}");
    }
}

fn rpc_vram_probe(socket: &str, bdf: &str) {
    println!("=== VRAM Probe: {bdf} ===");

    let regions: &[(u64, &str)] = &[
        (0x0000, "VRAM base"),
        (0x0100, "VRAM +0x100"),
        (0x1_0000, "VRAM +64K"),
    ];

    let mut alive = true;
    for &(offset, label) in regions {
        let response = rpc_call(
            socket,
            "device.pramin_read",
            serde_json::json!({
                "bdf": bdf,
                "vram_offset": offset,
                "count": 8,
            }),
        );
        check_rpc_error(&response);

        let result = response.get("result").unwrap_or(&serde_json::Value::Null);
        let values: Vec<u32> = result
            .get("values")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u32))
                    .collect()
            })
            .unwrap_or_default();

        let bad_count = values
            .iter()
            .filter(|&&v| (v >> 16) == 0xBAD0 || v == 0xDEAD_DEAD || v == 0xFFFF_FFFF)
            .count();

        if bad_count > values.len() / 2 {
            alive = false;
            print!("  {label} ({offset:#06x}): DEAD");
        } else {
            print!("  {label} ({offset:#06x}): ok  ");
        }
        for (i, val) in values.iter().enumerate() {
            if i < 4 {
                print!(" {val:#010x}");
            }
        }
        println!();
    }

    // Write-readback test at VRAM +0x100
    let test_val: u64 = 0xDEAD_BEEF;
    let read_before = rpc_call(
        socket,
        "device.pramin_read",
        serde_json::json!({ "bdf": bdf, "vram_offset": 0x100_u64, "count": 1 }),
    );
    check_rpc_error(&read_before);
    let before_val = read_before
        .get("result")
        .and_then(|r| r.get("values"))
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let write_resp = rpc_call(
        socket,
        "device.pramin_write",
        serde_json::json!({ "bdf": bdf, "vram_offset": 0x100_u64, "values": [test_val] }),
    );
    check_rpc_error(&write_resp);

    let read_after = rpc_call(
        socket,
        "device.pramin_read",
        serde_json::json!({ "bdf": bdf, "vram_offset": 0x100_u64, "count": 1 }),
    );
    check_rpc_error(&read_after);
    let after_val = read_after
        .get("result")
        .and_then(|r| r.get("values"))
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let readback_ok = after_val == test_val as u32;
    println!(
        "\n  Write-readback: before={before_val:#010x} wrote={test_val:#010x} read={after_val:#010x} {}",
        if readback_ok { "OK" } else { "FAILED" }
    );
    if !readback_ok {
        alive = false;
    }

    let status = if alive { "ALIVE" } else { "DEAD (0xbad0acXX)" };
    println!("\n=== HBM2: {status} ===");
}

fn rpc_mmio_read(socket: &str, bdf: &str, offset: usize) {
    let response = rpc_call(
        socket,
        "device.register_dump",
        serde_json::json!({ "bdf": bdf, "offsets": [offset] }),
    );
    check_rpc_error(&response);

    let result = response.get("result").unwrap_or(&serde_json::Value::Null);
    let regs = result
        .get("registers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if let Some(reg) = regs.first() {
        let off_str = reg
            .get("offset")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let val_str = reg.get("value").and_then(|v| v.as_str()).unwrap_or("?");
        println!("{off_str} = {val_str}");
    } else {
        eprintln!("error: no value returned for offset {offset:#010x}");
        std::process::exit(1);
    }
}

fn rpc_mmio_write(socket: &str, bdf: &str, offset: usize, value: u32, allow_dangerous: bool) {
    let response = rpc_call(
        socket,
        "device.write_register",
        serde_json::json!({
            "bdf": bdf,
            "offset": offset,
            "value": value as u64,
            "allow_dangerous": allow_dangerous,
        }),
    );
    check_rpc_error(&response);
    println!(
        "{:#010x} <- {:#010x}  ok",
        offset, value
    );
}

fn rpc_snapshot_save(socket: &str, bdf: &str, file: Option<String>) {
    let response = rpc_call(
        socket,
        "device.register_dump",
        serde_json::json!({ "bdf": bdf }),
    );
    check_rpc_error(&response);

    let result = match response.get("result") {
        Some(r) => r,
        None => {
            eprintln!("error: no result in response");
            std::process::exit(1);
        }
    };

    let filename = file.unwrap_or_else(|| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let safe_bdf = bdf.replace(':', "-");
        format!("{safe_bdf}_snapshot_{ts}.json")
    });

    let snapshot = serde_json::json!({
        "bdf": bdf,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        "registers": result.get("registers"),
    });

    let json = serde_json::to_string_pretty(&snapshot).expect("serialization");
    match std::fs::write(&filename, &json) {
        Ok(()) => {
            let count = result
                .get("register_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            println!("saved {count} registers to {filename}");
        }
        Err(e) => {
            eprintln!("error: failed to write {filename}: {e}");
            std::process::exit(1);
        }
    }
}

fn rpc_snapshot_diff(socket: &str, bdf: &str, file: &str) {
    let saved_json = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: failed to read {file}: {e}");
            std::process::exit(1);
        }
    };
    let saved: serde_json::Value = match serde_json::from_str(&saved_json) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: invalid JSON in {file}: {e}");
            std::process::exit(1);
        }
    };

    let saved_regs = saved
        .get("registers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let offsets: Vec<u64> = saved_regs
        .iter()
        .filter_map(|r| r.get("raw_offset").and_then(|v| v.as_u64()))
        .collect();

    let response = rpc_call(
        socket,
        "device.register_dump",
        serde_json::json!({ "bdf": bdf, "offsets": offsets }),
    );
    check_rpc_error(&response);

    let current_regs = response
        .get("result")
        .and_then(|r| r.get("registers"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let current_map: std::collections::HashMap<u64, u64> = current_regs
        .iter()
        .filter_map(|r| {
            let off = r.get("raw_offset").and_then(|v| v.as_u64())?;
            let val = r.get("raw_value").and_then(|v| v.as_u64())?;
            Some((off, val))
        })
        .collect();

    let mut changed = 0;
    let mut total = 0;
    println!("=== Snapshot Diff: {bdf} vs {file} ===");
    println!(
        "{:<14} {:<14} {:<14} {}",
        "OFFSET", "SAVED", "CURRENT", "STATUS"
    );
    println!("{}", "-".repeat(56));

    for reg in &saved_regs {
        let off = match reg.get("raw_offset").and_then(|v| v.as_u64()) {
            Some(o) => o,
            None => continue,
        };
        let saved_val = reg.get("raw_value").and_then(|v| v.as_u64()).unwrap_or(0);
        let current_val = current_map.get(&off).copied().unwrap_or(0xDEAD_DEAD);

        total += 1;
        let status = if saved_val == current_val {
            "="
        } else {
            changed += 1;
            "CHANGED"
        };
        if saved_val != current_val {
            println!(
                "{:#012x}   {:#012x}   {:#012x}   {status}",
                off, saved_val, current_val
            );
        }
    }
    println!("\n{changed}/{total} registers changed");
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
            eprintln!(
                "hint: pass --config <path> or create {}",
                config::system_config_path()
            );
            std::process::exit(1);
        }
    }
}

fn oracle_capture_rpc(socket: &str, bdf: &str, output: Option<&str>, max_channels: usize) {
    let response = rpc_call(
        socket,
        "device.oracle_capture",
        serde_json::json!({"bdf": bdf, "max_channels": max_channels}),
    );
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let channel_count = result
            .get("channels")
            .and_then(|c| c.as_array())
            .map_or(0, |a| a.len());
        let total_pts: usize = result
            .get("channels")
            .and_then(|c| c.as_array())
            .map(|chs| {
                chs.iter()
                    .filter_map(|ch| ch.get("page_tables").and_then(|p| p.as_array()))
                    .map(|pts| pts.len())
                    .sum()
            })
            .unwrap_or(0);
        let driver = result
            .get("driver")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        eprintln!("Captured {channel_count} channels, {total_pts} page tables (driver: {driver})");

        let json = serde_json::to_string_pretty(result).expect("serialize");
        match output {
            Some(path) => {
                std::fs::write(path, &json).expect("write output");
                eprintln!("Written to {path}");
            }
            None => println!("{json}"),
        }
    }
}

fn oracle_capture_local(bdf: &str, output: Option<&str>, max_channels: usize) {
    use coral_driver::vfio::channel::mmu_oracle;

    let driver = mmu_oracle::detect_driver(bdf);
    eprintln!("Capturing MMU state from {bdf} (driver: {driver})...");

    let dump = match mmu_oracle::capture_page_tables(bdf, max_channels) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Capture failed: {e}");
            std::process::exit(1);
        }
    };

    let channel_count = dump.channels.len();
    let total_pts: usize = dump.channels.iter().map(|c| c.page_tables.len()).sum();
    let total_ptes: usize = dump
        .channels
        .iter()
        .flat_map(|c| c.page_tables.iter())
        .map(|pt| pt.entries.len())
        .sum();
    eprintln!("Captured {channel_count} channels, {total_pts} page tables, {total_ptes} PTEs");

    let er = &dump.engine_registers;
    eprintln!(
        "PMU CPUCTL={:#010x} FECS CPUCTL={:#010x} SEC2 CPUCTL={:#010x}",
        er.pmu.get("PMU_FALCON_CPUCTL").unwrap_or(&0),
        er.fecs.get("FECS_FALCON_CPUCTL").unwrap_or(&0),
        er.sec2.get("SEC2_FALCON_CPUCTL").unwrap_or(&0),
    );

    let json = serde_json::to_string_pretty(&dump).expect("serialize");

    match output {
        Some(path) => {
            std::fs::write(path, &json).expect("write output");
            eprintln!("Written to {path}");
        }
        None => println!("{json}"),
    }
}

fn oracle_diff(left_path: &str, right_path: &str) {
    use coral_driver::vfio::channel::mmu_oracle;

    let left_json = std::fs::read_to_string(left_path).expect("read left");
    let right_json = std::fs::read_to_string(right_path).expect("read right");

    let left: mmu_oracle::PageTableDump = serde_json::from_str(&left_json).expect("parse left");
    let right: mmu_oracle::PageTableDump =
        serde_json::from_str(&right_json).expect("parse right");

    let diff = mmu_oracle::diff_page_tables(&left, &right);
    mmu_oracle::print_diff_report(&diff);

    let diff_json = serde_json::to_string_pretty(&diff).expect("serialize diff");
    println!("\n{diff_json}");
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

    #[test]
    fn try_load_config_reads_minimal_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("glowplug.toml");
        std::fs::write(
            &path,
            r#"
[[device]]
bdf = "0000:01:00.0"
"#,
        )
        .expect("write");
        let cfg = try_load_config(Some(path.to_string_lossy().into_owned())).expect("load");
        assert_eq!(cfg.device.len(), 1);
        assert_eq!(cfg.device[0].bdf, "0000:01:00.0");
    }

    #[test]
    fn deploy_udev_cli_accepts_config_and_dry_run() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gp.toml");
        std::fs::write(
            &path,
            r#"
[[device]]
bdf = "0000:ff:00.0"
"#,
        )
        .expect("write");
        let path_str = path.to_str().expect("utf8 path");
        let cli = Cli::try_parse_from([
            "coralctl",
            "--socket",
            "/tmp/x.sock",
            "deploy-udev",
            "--config",
            path_str,
            "--dry-run",
            "--group",
            "coralreef",
        ])
        .expect("parse");
        let Command::DeployUdev {
            config,
            dry_run,
            group,
            ..
        } = cli.command
        else {
            panic!("expected DeployUdev");
        };
        assert!(dry_run);
        assert_eq!(group, "coralreef");
        assert_eq!(config.as_deref(), Some(path_str));
    }

    #[test]
    fn cli_parses_probe_subcommand() {
        let cli =
            Cli::try_parse_from(["coralctl", "probe", "0000:03:00.0"]).expect("parse probe");
        let Command::Probe { bdf } = cli.command else {
            panic!("expected Probe");
        };
        assert_eq!(bdf, "0000:03:00.0");
    }

    #[test]
    fn cli_parses_vram_probe_subcommand() {
        let cli = Cli::try_parse_from(["coralctl", "vram-probe", "0000:4b:00.0"])
            .expect("parse vram-probe");
        let Command::VramProbe { bdf } = cli.command else {
            panic!("expected VramProbe");
        };
        assert_eq!(bdf, "0000:4b:00.0");
    }

    #[test]
    fn cli_parses_mmio_read_subcommand() {
        let cli = Cli::try_parse_from(["coralctl", "mmio", "read", "0000:03:00.0", "0x200"])
            .expect("parse mmio read");
        let Command::Mmio {
            action: MmioAction::Read { bdf, offset },
        } = cli.command
        else {
            panic!("expected Mmio Read");
        };
        assert_eq!(bdf, "0000:03:00.0");
        assert_eq!(offset, "0x200");
    }

    #[test]
    fn cli_parses_mmio_write_with_dangerous_flag() {
        let cli = Cli::try_parse_from([
            "coralctl",
            "mmio",
            "write",
            "0000:03:00.0",
            "0x200",
            "0xFFFFFFFF",
            "--allow-dangerous",
        ])
        .expect("parse mmio write");
        let Command::Mmio {
            action:
                MmioAction::Write {
                    bdf,
                    offset,
                    value,
                    allow_dangerous,
                },
        } = cli.command
        else {
            panic!("expected Mmio Write");
        };
        assert_eq!(bdf, "0000:03:00.0");
        assert_eq!(offset, "0x200");
        assert_eq!(value, "0xFFFFFFFF");
        assert!(allow_dangerous);
    }

    #[test]
    fn cli_parses_snapshot_save_subcommand() {
        let cli = Cli::try_parse_from([
            "coralctl",
            "snapshot",
            "save",
            "0000:03:00.0",
            "out.json",
        ])
        .expect("parse snapshot save");
        let Command::Snapshot {
            action: SnapshotAction::Save { bdf, file },
        } = cli.command
        else {
            panic!("expected Snapshot Save");
        };
        assert_eq!(bdf, "0000:03:00.0");
        assert_eq!(file.as_deref(), Some("out.json"));
    }

    #[test]
    fn cli_parses_snapshot_diff_subcommand() {
        let cli = Cli::try_parse_from([
            "coralctl",
            "snapshot",
            "diff",
            "0000:03:00.0",
            "saved.json",
        ])
        .expect("parse snapshot diff");
        let Command::Snapshot {
            action: SnapshotAction::Diff { bdf, file },
        } = cli.command
        else {
            panic!("expected Snapshot Diff");
        };
        assert_eq!(bdf, "0000:03:00.0");
        assert_eq!(file, "saved.json");
    }

    #[test]
    fn parse_hex_or_dec_works() {
        assert_eq!(parse_hex_or_dec("0x200").unwrap(), 0x200);
        assert_eq!(parse_hex_or_dec("0XDEAD").unwrap(), 0xDEAD);
        assert_eq!(parse_hex_or_dec("512").unwrap(), 512);
        assert!(parse_hex_or_dec("garbage").is_err());
    }

    #[test]
    fn snapshot_save_default_file_omits_optional() {
        let cli = Cli::try_parse_from(["coralctl", "snapshot", "save", "0000:03:00.0"])
            .expect("parse snapshot save no file");
        let Command::Snapshot {
            action: SnapshotAction::Save { bdf, file },
        } = cli.command
        else {
            panic!("expected Snapshot Save");
        };
        assert_eq!(bdf, "0000:03:00.0");
        assert!(file.is_none());
    }

    #[test]
    fn cli_parses_reset_subcommand() {
        let cli = Cli::try_parse_from(["coralctl", "reset", "0000:4a:00.0"]).expect("parse reset");
        let Command::Reset { bdf } = cli.command else {
            panic!("expected Reset");
        };
        assert_eq!(bdf, "0000:4a:00.0");
    }

    #[test]
    fn cli_parses_oracle_capture_subcommand() {
        let cli = Cli::try_parse_from([
            "coralctl",
            "oracle",
            "capture",
            "0000:03:00.0",
            "--output",
            "nvidia.json",
        ])
        .expect("parse oracle capture");
        let Command::Oracle {
            action:
                OracleAction::Capture {
                    bdf,
                    output,
                    max_channels,
                    local,
                },
        } = cli.command
        else {
            panic!("expected Oracle Capture");
        };
        assert_eq!(bdf, "0000:03:00.0");
        assert_eq!(output.as_deref(), Some("nvidia.json"));
        assert!(!local);
        assert_eq!(max_channels, 0);
    }

    #[test]
    fn cli_parses_oracle_diff_subcommand() {
        let cli =
            Cli::try_parse_from(["coralctl", "oracle", "diff", "left.json", "right.json"])
                .expect("parse oracle diff");
        let Command::Oracle {
            action: OracleAction::Diff { left, right },
        } = cli.command
        else {
            panic!("expected Oracle Diff");
        };
        assert_eq!(left, "left.json");
        assert_eq!(right, "right.json");
    }
}
