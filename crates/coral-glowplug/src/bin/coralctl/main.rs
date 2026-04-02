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
//!   deploy-udev        Generate /dev/vfio/* udev rules from glowplug.toml
//!   deploy-boot-config modprobe.d + vfio-pci.ids snippet from glowplug.toml
#![forbid(unsafe_code)]

mod deploy;
mod handlers_device;
mod handlers_diag;
mod handlers_trace;
mod onboard;
mod oracle;
mod rpc;

#[cfg(test)]
mod tests;

use clap::{Parser, Subcommand};

/// Resolve the glowplug socket path from an optional env value.
///
/// Follows wateringHole IPC standard: `$XDG_RUNTIME_DIR/biomeos/coral-glowplug-<family>.sock`.
fn resolve_glowplug_socket_path(env_value: Option<&str>) -> String {
    if let Some(p) = env_value {
        return p.to_owned();
    }
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let family = std::env::var("CORALREEF_FAMILY_ID")
        .or_else(|_| std::env::var("FAMILY_ID"))
        .unwrap_or_else(|_| "default".to_string());
    format!("{runtime_dir}/biomeos/coral-glowplug-{family}.sock")
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

/// Default path for generated modprobe.d (`$CORALREEF_MODPROBE_CONF` overrides).
fn default_modprobe_conf_path() -> String {
    std::env::var("CORALREEF_MODPROBE_CONF")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/etc/modprobe.d/coralreef-glowplug.conf".to_string())
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
        /// Poll FECS CPUCTL via BAR0 sysfs and swap as soon as FECS is
        /// running (bit4=0). Overrides --settle with a minimum init wait
        /// of 2s followed by 50ms polling intervals.
        #[arg(long)]
        poll_fecs: bool,
        /// Keep FECS busy by spawning a GPU workload process that holds
        /// an active DRM channel during the swap. The process is killed
        /// after vfio-pci binds.
        #[arg(long)]
        keepalive: bool,
    },

    /// Warm FECS via nvidia proprietary driver round-trip.
    ///
    /// Like warm-fecs but uses the nvidia proprietary driver instead of nouveau.
    /// RM initializes FECS differently (no HS+ lockdown for host interface).
    /// Captures BAR0 register state before/after for diff analysis.
    WarmFecsNvidia {
        /// PCI BDF address (e.g. 0000:03:00.0).
        bdf: String,
        /// Seconds to wait for nvidia driver init (default: 5).
        #[arg(long, default_value_t = 5)]
        settle: u64,
    },

    /// Onboard a new GPU — run firmware census, recommend boot path, probe protocols.
    ///
    /// Produces a structured report suitable for feeding into the firmware
    /// learning matrix. Run on any VFIO-bound device to discover its
    /// firmware capabilities.
    Onboard {
        /// PCI BDF address (e.g. 0000:03:00.0).
        bdf: String,
        /// Write report to file (default: stdout).
        #[arg(short, long)]
        output: Option<String>,
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

    /// Generate modprobe.d config and vfio-pci.ids comma list from glowplug.toml.
    DeployBootConfig {
        #[arg(short, long)]
        config: Option<String>,
        #[arg(long, default_value_t = default_modprobe_conf_path())]
        modprobe_output: String,
        /// Write the comma-separated vfio-pci.ids value to this file (one line). Omit to only print the value on stderr after writing modprobe.d.
        #[arg(long)]
        vfio_ids_output: Option<String>,
        #[arg(long)]
        dry_run: bool,
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

    /// Parse an mmiotrace file into a domain-classified boot sequence summary.
    ///
    /// Uses coral-driver's BootTrace parser to extract all MMIO writes/reads,
    /// classify them by GPU domain, and optionally emit a replay recipe.
    TraceParse {
        /// Path to the mmiotrace file (text format from /sys/kernel/debug/tracing).
        file: String,
        /// Emit the recipe as JSON to stdout instead of a human-readable summary.
        #[arg(long)]
        recipe_json: bool,
    },

    /// Replay VBIOS devinit scripts on a cold GPU.
    ///
    /// Reads the GPU's PROM, locates VBIOS init scripts, and executes them
    /// via BAR0 writes to bring up clock domains and basic GPU state.
    /// Requires direct VFIO group access (runs locally, not via RPC).
    Devinit {
        #[command(subcommand)]
        action: DevinitAction,
    },

    /// Sovereign cold boot a Tesla K80 (GK210 Kepler) from fully powered-off state.
    ///
    /// Replays the BIOS init recipe (PLLs, clocks, engine domains) via BAR0,
    /// then PIO-boots FECS/GPCCS firmware. The GPU ends up in a state ready
    /// for compute channel creation — no nouveau or nvidia driver needed.
    /// Requires direct VFIO group access (runs locally, not via RPC).
    ColdBoot {
        /// PCI BDF address of the K80 (e.g. 0000:4c:00.0).
        bdf: String,
        /// Path to the BIOS init recipe JSON (captured from nvidia470 VM session).
        #[arg(long)]
        recipe: String,
        /// Directory containing FECS/GPCCS firmware blobs
        /// (fecs_inst.bin, fecs_data.bin, gpccs_inst.bin, gpccs_data.bin).
        /// Default: built-in data/firmware/nvidia/gk110/ relative to the binary.
        #[arg(long)]
        firmware_dir: Option<String>,
        /// Include PGRAPH registers in the recipe replay (needed for GR engine).
        #[arg(long, default_value_t = true)]
        pgraph: bool,
        /// Include PCCSR registers in the recipe replay (channel status).
        #[arg(long)]
        pccsr: bool,
        /// Include PRAMIN registers in the recipe replay (instance memory).
        #[arg(long)]
        pramin: bool,
        /// Skip FECS/GPCCS firmware upload (clock + devinit only).
        #[arg(long)]
        skip_firmware: bool,
    },
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
    /// Apply a recipe JSON to a GPU via BAR0 register writes.
    ///
    /// Reads a cold→warm diff JSON (or recipe JSON) and replays the register
    /// writes through the daemon. Requires the GPU to be on vfio-pci.
    Apply {
        /// PCI BDF address (e.g. 0000:4c:00.0).
        bdf: String,
        /// Path to the recipe JSON file.
        recipe: String,
        /// Apply directly via VFIO BAR0 (requires VFIO group access).
        #[arg(long)]
        local: bool,
    },
}

#[derive(Subcommand)]
enum DevinitAction {
    /// Replay VBIOS devinit scripts on a cold GPU.
    ///
    /// Reads PROM from BAR0, parses BIT tables, locates init scripts, and
    /// executes them to bring up clock domains. Requires direct VFIO access.
    Replay {
        /// PCI BDF address (e.g. 0000:4c:00.0).
        bdf: String,
        /// Run with enhanced diagnostics (slower but more detailed output).
        #[arg(long)]
        diagnostics: bool,
    },
}

fn parse_hex_or_dec(s: &str) -> Result<u64, String> {
    coral_driver::parse_hex_u64(s)
}

fn main() {
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
                        eprintln!("error: {e}");
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
            OracleAction::Apply { bdf, recipe, local } => {
                if local {
                    handlers_trace::oracle_apply_local(&bdf, &recipe);
                } else {
                    handlers_trace::oracle_apply_rpc(&cli.socket, &bdf, &recipe);
                }
            }
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
        Command::WarmFecs {
            bdf,
            settle,
            poll_fecs,
            keepalive,
        } => handlers_device::rpc_warm_fecs(&cli.socket, &bdf, settle, poll_fecs, keepalive),
        Command::WarmFecsNvidia { bdf, settle } => {
            handlers_device::rpc_warm_fecs_nvidia(&cli.socket, &bdf, settle)
        }
        Command::Onboard { bdf, output } => {
            onboard::run_onboard(&cli.socket, &bdf, output.as_deref())
        }
        Command::DeployUdev {
            config: config_path,
            output,
            dry_run,
            group,
        } => {
            deploy::deploy_udev(config_path, &output, dry_run, &group);
        }
        Command::DeployBootConfig {
            config: config_path,
            modprobe_output,
            vfio_ids_output,
            dry_run,
        } => {
            deploy::deploy_boot_config(
                config_path,
                &modprobe_output,
                vfio_ids_output.as_deref(),
                dry_run,
            );
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
        Command::TraceParse { file, recipe_json } => {
            handlers_trace::trace_parse(&file, recipe_json);
        }
        Command::Devinit { action } => match action {
            DevinitAction::Replay { bdf, diagnostics } => {
                handlers_trace::devinit_replay(&bdf, diagnostics);
            }
        },
        Command::ColdBoot {
            bdf,
            recipe,
            firmware_dir,
            pgraph,
            pccsr,
            pramin,
            skip_firmware,
        } => {
            handlers_trace::cold_boot_replay(
                &bdf,
                &recipe,
                firmware_dir.as_deref(),
                pgraph,
                pccsr,
                pramin,
                skip_firmware,
            );
        }
    }
}
