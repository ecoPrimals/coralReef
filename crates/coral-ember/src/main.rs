// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use coral_ember::{EmberRunOptions, run_with_options};

/// Immortal VFIO fd holder — JSON-RPC server for coral-glowplug integration.
#[derive(Parser)]
#[command(name = "coral-ember", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    /// Path to `glowplug.toml` (when no subcommand is given; legacy).
    #[arg(value_name = "CONFIG")]
    legacy_config: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the ember JSON-RPC server (holds VFIO fds, Unix socket + optional TCP).
    Server {
        /// TCP port for JSON-RPC on `127.0.0.1` (Unix socket is always used; `ember.vfio_fds` needs Unix).
        #[arg(short, long)]
        port: Option<u16>,
        /// Path to `glowplug.toml` (optional; defaults to XDG/system discovery).
        #[arg(value_name = "CONFIG")]
        config: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    let opts = match cli.command {
        Some(Commands::Server { port, config }) => EmberRunOptions {
            config_path: config,
            listen_port: port,
        },
        None => EmberRunOptions {
            config_path: cli.legacy_config,
            listen_port: None,
        },
    };

    if let Err(code) = run_with_options(opts) {
        std::process::exit(code);
    }
}
