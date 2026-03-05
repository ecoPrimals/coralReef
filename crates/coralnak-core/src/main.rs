// SPDX-License-Identifier: AGPL-3.0-only
//! coralNak — sovereign Rust NVIDIA shader compiler.
//!
//! `UniBin` entry point: single binary, multiple modes via subcommands.
//!
//! Exit codes follow ecoPrimals `UniBin` standard:
//! - 0 = Success
//! - 1 = General error
//! - 2 = Configuration / input error
//! - 3 = Internal error (panic, OOM)
//! - 130 = SIGTERM/SIGINT (graceful shutdown)

use std::io;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use coral_nak::{CompileError, GpuArch};
use tracing_subscriber::EnvFilter;

mod ipc;
mod service;

use ipc::DEFAULT_TCP_BIND;

#[derive(Parser)]
#[command(name = env!("CARGO_PKG_NAME"), version, about, long_about = None)]
struct Cli {
    /// Log level (trace, debug, info, warn, error).
    #[arg(long, default_value = "info", global = true)]
    log_level: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the IPC server (JSON-RPC 2.0 + tarpc).
    Server {
        /// Bind address for JSON-RPC server (TCP only — HTTP transport).
        #[arg(long, default_value = DEFAULT_TCP_BIND)]
        rpc_bind: String,

        /// Bind address for tarpc server.
        /// TCP: `127.0.0.1:0`; Unix socket: `unix:///path/to/socket`.
        /// Defaults to platform-native transport (Unix socket on Linux/macOS).
        #[arg(long)]
        tarpc_bind: Option<String>,
    },

    /// Compile a shader file.
    Compile {
        /// Input file (SPIR-V binary or WGSL source).
        #[arg()]
        input: std::path::PathBuf,

        /// Output file for compiled binary.
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,

        /// Target GPU architecture (use `doctor` to list supported values).
        #[arg(long, default_value_t = GpuArch::default())]
        arch: GpuArch,

        /// Optimization level (0-3).
        #[arg(long, default_value = "2")]
        opt_level: u32,

        /// Enable f64 software transcendentals.
        #[arg(long, default_value = "true")]
        fp64_software: bool,
    },

    /// Health and diagnostic check.
    Doctor,
}

/// `UniBin` exit codes.
#[repr(i32)]
#[derive(Clone, Copy)]
enum UniBinExit {
    Success = 0,
    GeneralError = 1,
    ConfigError = 2,
    InternalError = 3,
    Signal = 130,
}

impl From<UniBinExit> for ExitCode {
    fn from(code: UniBinExit) -> Self {
        ExitCode::from(code as u8)
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    install_panic_hook();

    let cli = match parse_cli() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return UniBinExit::ConfigError.into();
        }
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cli.log_level)),
        )
        .init();

    let exit = match cli.command {
        Commands::Server {
            rpc_bind,
            tarpc_bind,
        } => {
            let tarpc_bind = tarpc_bind.unwrap_or_else(ipc::default_tarpc_bind);
            cmd_server(&rpc_bind, &tarpc_bind).await
        }
        Commands::Compile {
            input,
            output,
            arch,
            opt_level,
            fp64_software,
        } => cmd_compile(&input, output.as_deref(), arch, opt_level, fp64_software),
        Commands::Doctor => cmd_doctor().await,
    };

    exit.into()
}

fn parse_cli() -> Result<Cli, clap::Error> {
    Cli::try_parse()
}

/// Install panic hook that logs structurally and exits with code 3 (`UniBin` internal error).
/// Never prints raw panic messages to users per `UniBin` structured error requirements.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let payload = info.payload();
        let msg = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "panic".to_string()
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        // Log structurally; tracing may not be initialized yet, so also eprintln as fallback
        eprintln!("internal error: panic: message={msg}, location={location:?}");
        std::process::exit(UniBinExit::InternalError as i32);
    }));
}

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

async fn cmd_server(rpc_bind: &str, tarpc_bind: &str) -> UniBinExit {
    tracing::info!("{} server starting", env!("CARGO_PKG_NAME"));
    tracing::info!(rpc_bind, tarpc_bind, "binding addresses");

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());

    let (rpc_addr, rpc_handle) = match ipc::start_jsonrpc_server(rpc_bind).await {
        Ok(x) => x,
        Err(e) => {
            tracing::error!(error = %e, "failed to start JSON-RPC server");
            return UniBinExit::GeneralError;
        }
    };

    let (tarpc_bound, tarpc_handle) = match ipc::start_tarpc_server(tarpc_bind, shutdown_rx).await {
        Ok(x) => x,
        Err(e) => {
            tracing::error!(error = %e, "failed to start tarpc server");
            let _ = rpc_handle.stop();
            return UniBinExit::GeneralError;
        }
    };

    let desc = coralnak_core::capability::self_description();
    let desc = coralnak_core::capability::with_transports(
        desc,
        vec![
            coralnak_core::capability::Transport {
                protocol: "jsonrpc".to_owned(),
                address: rpc_addr.to_string(),
            },
            coralnak_core::capability::Transport {
                protocol: format!("tarpc+{}", tarpc_bound.protocol()),
                address: tarpc_bound.to_string(),
            },
        ],
    );
    tracing::info!(
        rpc_addr = %rpc_addr,
        tarpc_addr = %tarpc_bound,
        provides = ?desc.provides.iter().map(|c| &c.id).collect::<Vec<_>>(),
        requires = ?desc.requires.iter().map(|c| &c.id).collect::<Vec<_>>(),
        "{} ready — capability advertisement prepared", env!("CARGO_PKG_NAME")
    );

    // Wait for SIGTERM or SIGINT (graceful shutdown)
    let signal_received = wait_for_shutdown_signal().await;
    tracing::info!(signal = ?signal_received, "received shutdown signal, stopping servers");

    // 1. Stop accepting new connections
    let _ = shutdown_tx.send(());
    let _ = rpc_handle.stop();

    let rpc_stopped = rpc_handle.clone().stopped();
    let shutdown_result = tokio::time::timeout(SHUTDOWN_TIMEOUT, async move {
        rpc_stopped.await;
        tarpc_handle.await.ok();
    })
    .await;

    if shutdown_result.is_err() {
        tracing::warn!("shutdown timed out after {:?}", SHUTDOWN_TIMEOUT);
    }

    UniBinExit::Signal
}

/// Wait for SIGTERM or SIGINT. Returns which signal was received.
async fn wait_for_shutdown_signal() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm = signal(SignalKind::terminate()).expect("failed to register SIGTERM");
        let mut sigint = signal(SignalKind::interrupt()).expect("failed to register SIGINT");

        tokio::select! {
            _ = sigterm.recv() => "SIGTERM",
            _ = sigint.recv() => "SIGINT",
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to register Ctrl+C");
        "SIGINT"
    }
}

fn cmd_compile(
    input: &std::path::Path,
    output: Option<&std::path::Path>,
    arch: GpuArch,
    opt_level: u32,
    fp64_software: bool,
) -> UniBinExit {
    let options = coral_nak::CompileOptions {
        arch,
        opt_level,
        debug_info: false,
        fp64_software,
    };

    let input_bytes = match std::fs::read(input) {
        Ok(b) => b,
        Err(e) => {
            let code = if e.kind() == io::ErrorKind::NotFound {
                UniBinExit::ConfigError
            } else {
                UniBinExit::GeneralError
            };
            tracing::error!(path = %input.display(), error = %e, "failed to read input");
            return code;
        }
    };

    let result = if input.extension().is_some_and(|e| e == "wgsl") {
        let source = match String::from_utf8(input_bytes) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, "invalid UTF-8 in WGSL source");
                return UniBinExit::ConfigError;
            }
        };
        coral_nak::compile_wgsl(&source, &options)
    } else {
        let words: Vec<u32> = input_bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        coral_nak::compile(&words, &options)
    };

    match result {
        Ok(binary) => {
            let out_path =
                output.map_or_else(|| input.with_extension("bin"), std::path::Path::to_path_buf);
            if let Err(e) = std::fs::write(&out_path, &binary) {
                tracing::error!(path = %out_path.display(), error = %e, "failed to write output");
                return UniBinExit::GeneralError;
            }
            tracing::info!(path = %out_path.display(), size = binary.len(), "compiled");
            UniBinExit::Success
        }
        Err(e) => {
            let code = error_to_exit_code(&e);
            tracing::error!(error = %e, "compilation failed");
            code
        }
    }
}

fn error_to_exit_code(e: &CompileError) -> UniBinExit {
    match e {
        CompileError::InvalidInput(_) | CompileError::UnsupportedArch(_) => UniBinExit::ConfigError,
        _ => UniBinExit::GeneralError,
    }
}

async fn cmd_doctor() -> UniBinExit {
    use coralnak_core::CoralNakPrimal;
    use coralnak_core::health::PrimalHealth;
    use coralnak_core::lifecycle::PrimalLifecycle;

    println!("{} doctor — diagnostic check\n", env!("CARGO_PKG_NAME"));

    let desc = coralnak_core::capability::self_description();
    println!("[OK] Capabilities (provides):");
    for cap in &desc.provides {
        println!("     - {} v{}", cap.id, cap.version);
    }
    println!("[OK] Capabilities (requires):");
    for cap in &desc.requires {
        println!("     - {} v{}", cap.id, cap.version);
    }
    println!("[OK] Supported architectures:");
    for arch in GpuArch::ALL {
        println!("     - {arch}");
    }

    let mut primal = CoralNakPrimal::new();
    println!("[OK] Primal created (state: {:?})", primal.state());

    if let Err(e) = primal.start().await {
        tracing::error!(error = %e, "primal start failed");
        return UniBinExit::GeneralError;
    }
    println!("[OK] Primal started (state: {:?})", primal.state());

    let report = match primal.health_check().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "health check failed");
            let _ = primal.stop().await;
            return UniBinExit::GeneralError;
        }
    };
    println!("[OK] Health: {:?}", report.status);

    let test_opts = coral_nak::CompileOptions::default();
    match coral_nak::compile(&[0x0723_0203], &test_opts) {
        Ok(_) => println!("[OK] Compile pipeline operational"),
        Err(coral_nak::CompileError::NotImplemented(_)) => {
            println!("[WARN] Compile pipeline: not yet implemented");
        }
        Err(e) => {
            tracing::error!(error = %e, "compile pipeline failed");
            let _ = primal.stop().await;
            return UniBinExit::GeneralError;
        }
    }

    if let Err(e) = primal.stop().await {
        tracing::error!(error = %e, "primal stop failed");
        return UniBinExit::GeneralError;
    }
    println!("[OK] Primal stopped cleanly");
    println!("\nDiagnostic complete.");
    UniBinExit::Success
}
