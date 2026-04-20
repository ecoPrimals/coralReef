// SPDX-License-Identifier: AGPL-3.0-or-later
mod deploy;
mod handlers_capture;
mod handlers_device;
mod handlers_diag;
mod oracle;
mod rpc;

use coral_glowplug::config::default_daemon_socket_path;
use coral_glowplug::error::ParseError;

#[cfg(test)]
mod tests;

use clap::{Parser, Subcommand};

/// Resolve the glowplug socket path from an optional env value.
///
/// `default_socket` passes `std::env::var("CORALREEF_GLOWPLUG_SOCKET").ok().as_deref()`.
fn resolve_glowplug_socket_path(env_value: Option<&str>) -> String {
    env_value
        .map(|s| s.to_owned())
        .unwrap_or_else(default_daemon_socket_path)
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
    version,
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
        /// Target driver (vfio, nouveau, amdgpu, nvidia, nvidia_oracle, xe, i915, unbound).
        target: String,
        /// Enable mmiotrace capture during the driver bind.
        /// Captures every MMIO write the driver performs during initialization.
        #[arg(long)]
        trace: bool,
    },

    /// Show mmiotrace status and list captured traces.
    TraceStatus,

    /// List all captured mmiotrace data.
    TraceList,

    /// Reset a PCI device to recover from corrupted GPU state.
    ///
    /// Methods:
    ///   auto (default)  — tries vendor-preferred chain (bridge-sbr → sbr → remove-rescan)
    ///   bridge-sbr      — SBR via parent PCI bridge (best for GV100 under VFIO)
    ///   sbr             — device-level sysfs reset (may fail under VFIO for FLR-less hw)
    ///   flr             — PCIe Function Level Reset via VFIO (requires FLR-capable hw)
    ///   remove-rescan   — PCI remove + bus rescan (nuclear option, invalidates VFIO fds)
    Reset {
        /// PCI BDF address (e.g. 0000:4a:00.0).
        bdf: String,
        /// Reset method: auto, bridge-sbr, sbr, flr, or remove-rescan. Default: auto.
        #[arg(long, default_value = "auto")]
        method: String,
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

    /// Warm FECS firmware via nouveau round-trip.
    ///
    /// Swaps the GPU to nouveau (which loads ACR → FECS/GPCCS firmware),
    /// waits for GR init, then swaps back to VFIO. Ember's NvidiaLifecycle
    /// disables `reset_method` so FECS IMEM persists across the swap.
    WarmFecs {
        /// PCI BDF address (e.g. 0000:03:00.0).
        bdf: String,
        /// Seconds to wait for nouveau GR init (default: 12).
        #[arg(long, default_value_t = 12)]
        settle: u64,
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

    /// Query the experiment journal for swap, reset, and boot observations.
    Journal {
        #[command(subcommand)]
        action: JournalAction,
    },

    /// Run automated experiments on a GPU — sweep personalities and compare.
    Experiment {
        #[command(subcommand)]
        action: ExperimentAction,
    },

    /// Training recipe capture — observe vendor driver init and distill a replay recipe.
    Capture {
        #[command(subcommand)]
        action: CaptureAction,
    },

    /// Run the full sovereign boot orchestration on a GPU.
    SovereignBoot {
        /// PCI BDF address (e.g. 0000:03:00.0).
        bdf: String,
    },
}

#[derive(Subcommand)]
enum CaptureAction {
    /// Capture a training recipe by observing an external driver's memory init.
    Training {
        /// PCI BDF address (e.g. 0000:03:00.0).
        bdf: String,
        /// Warm driver to use (nouveau, nvidia). Default: auto-detect.
        #[arg(long)]
        driver: Option<String>,
    },
    /// Compare two training recipe files.
    Compare {
        /// Left recipe file path.
        left: String,
        /// Right recipe file path.
        right: String,
    },
    /// Decode PLL/timing patterns from a training recipe.
    Decode {
        /// Recipe file path.
        file: String,
    },
    /// List all captured training recipes.
    List,
}

#[derive(Subcommand)]
enum ExperimentAction {
    /// Sweep through multiple personalities on a device, tracing each.
    ///
    /// For each personality: swap (with mmiotrace), capture timing and observer
    /// insights, then swap back to the return personality. All observations are
    /// recorded in the experiment journal.
    ///
    /// Use --repeat to run each personality multiple times for statistical
    /// significance. Use comma-separated BDFs to sweep multiple cards and
    /// get a cross-card comparison table.
    Sweep {
        /// PCI BDF address(es), comma-separated for cross-card comparison
        /// (e.g. "0000:03:00.0" or "0000:03:00.0,0000:4a:00.0").
        bdf: String,
        /// Comma-separated list of personalities to test. Default: nouveau,amdgpu,nvidia-open,xe,i915.
        #[arg(long)]
        personalities: Option<String>,
        /// Personality to return to after each test swap. Default: vfio.
        #[arg(long, default_value = "vfio")]
        return_to: String,
        /// Enable mmiotrace for each swap. Default: true.
        #[arg(long, default_value_t = true)]
        trace: bool,
        /// Number of times to repeat each personality swap (for statistical analysis).
        #[arg(long, default_value_t = 1)]
        repeat: u32,
    },
}

#[derive(Subcommand)]
enum JournalAction {
    /// Query journal entries with optional filters.
    Query {
        /// Filter by PCI BDF address.
        #[arg(long)]
        bdf: Option<String>,
        /// Filter by entry kind: Swap, Reset, or BootAttempt.
        #[arg(long)]
        kind: Option<String>,
        /// Filter by personality name.
        #[arg(long)]
        personality: Option<String>,
        /// Maximum entries to return (newest first).
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show aggregate statistics from journal entries.
    Stats {
        /// Filter stats by PCI BDF address.
        #[arg(long)]
        bdf: Option<String>,
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
        /// Output file path (default: `<BDF>_snapshot_<timestamp>.json`).
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

fn parse_hex_or_dec(s: &str) -> Result<u64, ParseError> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|source| ParseError::Hex {
            input: s.to_string(),
            source,
        })
    } else {
        s.parse::<u64>().map_err(|source| ParseError::Dec {
            input: s.to_string(),
            source,
        })
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Status => handlers_device::rpc_status(&cli.socket),
        Command::Swap { bdf, target, trace } => {
            handlers_device::rpc_swap(&cli.socket, &bdf, &target, trace)
        }
        Command::TraceStatus => {
            let resp = rpc::rpc_call(&cli.socket, "trace.status", serde_json::json!({}));
            rpc::check_rpc_error(&resp);
            if let Some(result) = resp.get("result") {
                println!(
                    "{}",
                    serde_json::to_string_pretty(result).unwrap_or_default()
                );
            }
        }
        Command::TraceList => {
            let resp = rpc::rpc_call(&cli.socket, "trace.list", serde_json::json!({}));
            rpc::check_rpc_error(&resp);
            if let Some(result) = resp.get("result") {
                println!(
                    "{}",
                    serde_json::to_string_pretty(result).unwrap_or_default()
                );
            }
        }
        Command::Reset { bdf, method } => handlers_device::rpc_reset(&cli.socket, &bdf, &method),
        Command::Health => handlers_device::rpc_health(&cli.socket),
        Command::Probe { bdf } => handlers_diag::rpc_probe(&cli.socket, &bdf),
        Command::VramProbe { bdf } => handlers_diag::rpc_vram_probe(&cli.socket, &bdf),
        Command::Mmio { action } => match action {
            MmioAction::Read { bdf, offset } => {
                let off = match parse_hex_or_dec(&offset) {
                    Ok(v) => v as usize,
                    Err(e) => {
                        tracing::error!(error = %e, "invalid offset");
                        std::process::exit(1);
                    }
                };
                handlers_diag::rpc_mmio_read(&cli.socket, &bdf, off);
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
                        tracing::error!(error = %e, "invalid offset");
                        std::process::exit(1);
                    }
                };
                let val = match parse_hex_or_dec(&value) {
                    Ok(v) => v as u32,
                    Err(e) => {
                        tracing::error!(error = %e, "invalid value");
                        std::process::exit(1);
                    }
                };
                handlers_diag::rpc_mmio_write(&cli.socket, &bdf, off, val, allow_dangerous);
            }
        },
        Command::Snapshot { action } => match action {
            SnapshotAction::Save { bdf, file } => {
                handlers_diag::rpc_snapshot_save(&cli.socket, &bdf, file);
            }
            SnapshotAction::Diff { bdf, file } => {
                handlers_diag::rpc_snapshot_diff(&cli.socket, &bdf, &file);
            }
        },
        Command::Oracle { action } => match action {
            OracleAction::Capture {
                bdf,
                output,
                max_channels,
                local,
            } => {
                if local {
                    oracle::oracle_capture_local(&bdf, output.as_deref(), max_channels);
                } else {
                    oracle::oracle_capture_rpc(&cli.socket, &bdf, output.as_deref(), max_channels);
                }
            }
            OracleAction::Diff { left, right } => oracle::oracle_diff(&left, &right),
        },
        Command::ComputeInfo { bdf } => handlers_device::rpc_compute_info(&cli.socket, &bdf),
        Command::ComputeQuota {
            bdf,
            power_limit,
            compute_mode,
            vram_budget,
        } => {
            if power_limit.is_some() || compute_mode.is_some() || vram_budget.is_some() {
                handlers_device::rpc_set_quota(
                    &cli.socket,
                    &bdf,
                    power_limit,
                    compute_mode.as_deref(),
                    vram_budget,
                );
            } else {
                handlers_device::rpc_get_quota(&cli.socket, &bdf);
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
            handlers_device::rpc_dispatch(
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
        Command::WarmFecs { bdf, settle } => {
            handlers_device::rpc_warm_fecs(&cli.socket, &bdf, settle)
        }
        Command::DeployUdev {
            config: config_path,
            output,
            dry_run,
            group,
        } => {
            deploy::deploy_udev(config_path, &output, dry_run, &group);
        }
        Command::Journal { action } => match action {
            JournalAction::Query {
                bdf,
                kind,
                personality,
                limit,
            } => {
                handlers_device::rpc_journal_query(&cli.socket, bdf, kind, personality, limit);
            }
            JournalAction::Stats { bdf } => {
                handlers_device::rpc_journal_stats(&cli.socket, bdf);
            }
        },
        Command::Experiment { action } => match action {
            ExperimentAction::Sweep {
                bdf,
                personalities,
                return_to,
                trace,
                repeat,
            } => {
                handlers_device::rpc_experiment_sweep(
                    &cli.socket,
                    &bdf,
                    personalities.as_deref(),
                    &return_to,
                    trace,
                    repeat,
                );
            }
        },
        Command::Capture { action } => match action {
            CaptureAction::Training { bdf, driver } => {
                handlers_capture::rpc_capture_training(&cli.socket, &bdf, driver.as_deref());
            }
            CaptureAction::Compare { left, right } => {
                handlers_capture::compare_recipes(&left, &right);
            }
            CaptureAction::Decode { file } => {
                handlers_capture::decode_recipe(&file);
            }
            CaptureAction::List => {
                handlers_capture::list_recipes();
            }
        },
        Command::SovereignBoot { bdf } => {
            let result = coral_glowplug::sovereign::sovereign_boot(&bdf);
            let json = serde_json::to_string_pretty(&result).unwrap_or_default();
            println!("{json}");
            if !result.success {
                std::process::exit(1);
            }
        }
    }
}
