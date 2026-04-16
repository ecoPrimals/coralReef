// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]
//! coralReef — sovereign Rust NVIDIA shader compiler.
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
use std::path::Path;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use coral_reef::GpuArch;
use coralreef_core::commands;
use coralreef_core::or_exit::OrExit;
use tracing_subscriber::EnvFilter;

mod config {
    pub use coralreef_core::config::*;
}

mod capability {
    pub use coralreef_core::capability::*;
}

mod ipc;
mod service;

use ipc::default_tcp_bind;

#[derive(Debug, Parser)]
#[command(name = env!("CARGO_PKG_NAME"), version, about, long_about = None)]
struct Cli {
    /// Log level (trace, debug, info, warn, error).
    #[arg(long, default_value = "info", global = true)]
    log_level: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Start the IPC server (JSON-RPC 2.0 + tarpc).
    Server {
        /// Bind address for newline-delimited JSON-RPC over TCP.
        /// Respects `$CORALREEF_TCP_BIND` for deployment configuration.
        #[arg(long, default_value_t = default_tcp_bind())]
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
    /// Set by the panic hook via `abort()` — the OS maps this to exit code 3.
    InternalError = 3,
    Signal = 130,
}

const _: () = assert!(UniBinExit::InternalError as i32 == 3);

impl From<UniBinExit> for ExitCode {
    fn from(code: UniBinExit) -> Self {
        Self::from(code as u8)
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    install_panic_hook();

    let cli = match parse_cli() {
        Ok(c) => c,
        Err(e) => {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::new("info"))
                .try_init();
            tracing::error!(error = %e, "invalid command line");
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
    parse_cli_from(std::env::args_os())
}

/// Parse CLI from given args. Used by `main` and tests.
fn parse_cli_from<I, T>(args: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    Cli::try_parse_from(args)
}

/// Install panic hook that logs structurally and aborts.
/// Never prints raw panic messages to users per `UniBin` structured error requirements.
/// Uses `abort()` rather than `exit()` so destructors run; panics indicate unrecoverable state.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let payload = info.payload();
        let msg = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".to_string());
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::ERROR)
            .try_init();
        tracing::error!(
            message = %msg,
            location = ?location,
            "internal error: panic before normal logging bootstrap"
        );
        std::process::abort();
    }));
}

#[cfg(test)]
static TEST_SHUTDOWN_JOIN_TIMEOUT_MS_OVERRIDE: std::sync::Mutex<Option<u64>> =
    std::sync::Mutex::new(None);

/// Shutdown join timeout for `cmd_server` graceful teardown.
///
/// Unit tests use `TEST_SHUTDOWN_JOIN_TIMEOUT_MS_OVERRIDE`. Subprocess server tests (which run the
/// debug `coralreef` binary without `cfg(test)`) use `CORALREEF_TEST_SHUTDOWN_JOIN_TIMEOUT_MS`,
/// honored only in `cfg(test)` or `cfg(debug_assertions)` builds so release binaries ignore it.
fn shutdown_join_timeout() -> std::time::Duration {
    #[cfg(test)]
    if let Ok(g) = TEST_SHUTDOWN_JOIN_TIMEOUT_MS_OVERRIDE.lock() {
        if let Some(ms) = *g {
            return std::time::Duration::from_millis(ms);
        }
    }
    #[cfg(any(test, debug_assertions))]
    if let Ok(ms) = std::env::var("CORALREEF_TEST_SHUTDOWN_JOIN_TIMEOUT_MS") {
        if let Ok(ms) = ms.parse::<u64>() {
            return std::time::Duration::from_millis(ms);
        }
    }
    config::DEFAULT_SHUTDOWN_TIMEOUT
}

fn shutdown_join_timeout_elapsed_message(join_timeout: std::time::Duration) -> String {
    format!("shutdown timed out after {join_timeout:?}")
}

async fn cmd_server(rpc_bind: &str, tarpc_bind: &str) -> UniBinExit {
    if let Err(e) = config::validate_insecure_guard() {
        tracing::error!(error = %e, "configuration rejected");
        return UniBinExit::ConfigError;
    }

    tracing::info!("{} server starting", env!("CARGO_PKG_NAME"));
    tracing::info!(rpc_bind, tarpc_bind, "binding addresses");

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());

    let (rpc_addr, rpc_handle) =
        match ipc::start_newline_tcp_jsonrpc(rpc_bind, shutdown_rx.clone()).await {
            Ok(x) => x,
            Err(e) => {
                tracing::error!(error = %e, "failed to start JSON-RPC server");
                return UniBinExit::GeneralError;
            }
        };

    let (tarpc_bound, tarpc_handle) =
        match ipc::start_tarpc_server(tarpc_bind, shutdown_rx.clone()).await {
            Ok(x) => x,
            Err(e) => {
                tracing::error!(error = %e, "failed to start tarpc server");
                rpc_handle.abort();
                return UniBinExit::GeneralError;
            }
        };

    #[cfg(unix)]
    let unix_jsonrpc_handle = {
        let sock_path = ipc::default_unix_socket_path();
        match ipc::start_unix_jsonrpc_server(&sock_path, shutdown_rx).await {
            Ok((_path, handle)) => {
                tracing::info!(path = %sock_path.display(), "Unix JSON-RPC server started");
                Some(handle)
            }
            Err(e) => {
                tracing::warn!(error = %e, "Unix JSON-RPC server failed to start (ecosystem primal discovery degraded)");
                None
            }
        }
    };

    let desc = coralreef_core::capability::self_description();
    let transports = vec![
        coralreef_core::capability::Transport {
            protocol: "jsonrpc".into(),
            address: rpc_addr.to_string().into(),
        },
        coralreef_core::capability::Transport {
            protocol: format!("tarpc+{}", tarpc_bound.protocol()).into(),
            address: tarpc_bound.to_string().into(),
        },
    ];
    let desc = coralreef_core::capability::with_transports(desc, transports);
    tracing::info!(
        rpc_addr = %rpc_addr,
        tarpc_addr = %tarpc_bound,
        provides = ?desc.provides.iter().map(|c| &c.id).collect::<Vec<_>>(),
        requires = ?desc.requires.iter().map(|c| &c.id).collect::<Vec<_>>(),
        "{} ready — capability advertisement prepared", env!("CARGO_PKG_NAME")
    );

    service::set_identity_from_self_description(&desc);

    if let Err(e) = write_discovery_file(&desc).await {
        tracing::warn!(error = %e, "failed to write discovery file (peers must use fallback discovery)");
    }

    coralreef_core::ecosystem::spawn_registration(desc);

    let signal_received = wait_for_shutdown_signal().await;
    tracing::info!(signal = ?signal_received, "received shutdown signal, stopping servers");

    let _ = shutdown_tx.send(());

    let join_timeout = shutdown_join_timeout();
    let shutdown_result = tokio::time::timeout(join_timeout, async move {
        if let Err(e) = rpc_handle.await {
            tracing::warn!(error = %e, "JSON-RPC task join failed during shutdown");
        }
        if let Err(e) = tarpc_handle.await {
            tracing::warn!(error = %e, "tarpc task join failed during shutdown");
        }
        #[cfg(unix)]
        if let Some(h) = unix_jsonrpc_handle {
            if let Err(e) = h.await {
                tracing::warn!(error = %e, "Unix JSON-RPC task join failed during shutdown");
            }
        }
    })
    .await;

    if shutdown_result.is_err() {
        tracing::warn!("{}", shutdown_join_timeout_elapsed_message(join_timeout));
    }

    remove_discovery_file().await;

    UniBinExit::Signal
}

/// Write a discovery file so peer primals can find this service.
///
/// File path: `{dir}/{CARGO_PKG_NAME}.json` where `dir` defaults to
/// `$XDG_RUNTIME_DIR/{ECOSYSTEM_NAMESPACE}`.
///
/// Format follows wateringHole Phase 10: `provides`, `transports` as
/// `{ "jsonrpc": { "bind": "..." }, "tarpc": { "bind": "..." } }`,
/// `primal`, `version`, `pid`.
async fn write_discovery_file(
    desc: &coralreef_core::capability::SelfDescription,
) -> io::Result<()> {
    write_discovery_file_to(&discovery_dir()?, desc).await
}

/// Write a discovery file into an explicit directory.
///
/// Separated from [`write_discovery_file`] so tests can target an isolated
/// temp directory instead of the shared `$XDG_RUNTIME_DIR/biomeos/` path.
async fn write_discovery_file_to(
    dir: &std::path::Path,
    desc: &coralreef_core::capability::SelfDescription,
) -> io::Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    let path = dir.join(format!("{}.json", env!("CARGO_PKG_NAME")));

    let jsonrpc_addr = desc
        .transports
        .iter()
        .find(|t| t.protocol == "jsonrpc")
        .map_or("", |t| t.address.as_ref());
    let tarpc_addr = desc
        .transports
        .iter()
        .find(|t| t.protocol.starts_with("tarpc"))
        .map_or("", |t| t.address.as_ref());

    let discovery = serde_json::json!({
        "primal": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
        "pid": std::process::id(),
        "provides": desc.provides.iter().map(|c| &c.id).collect::<Vec<_>>(),
        "requires": desc.requires.iter().map(|c| &c.id).collect::<Vec<_>>(),
        "transports": {
            "jsonrpc": { "bind": jsonrpc_addr },
            "tarpc": { "bind": tarpc_addr },
        },
    });

    tokio::fs::write(
        &path,
        serde_json::to_string_pretty(&discovery).expect("JSON Value serialization is infallible"),
    )
    .await?;
    tracing::info!(path = %path.display(), "wrote discovery file");
    Ok(())
}

/// Remove the discovery file on shutdown.
async fn remove_discovery_file() {
    match discovery_dir() {
        Ok(dir) => remove_discovery_file_from(Some(dir.as_path())).await,
        Err(e) => tracing::debug!(error = %e, "discovery dir unavailable, skipping file removal"),
    }
}

/// Remove a discovery file from an explicit directory.
async fn remove_discovery_file_from(dir: Option<&std::path::Path>) {
    if let Some(dir) = dir {
        let path = dir.join(format!("{}.json", env!("CARGO_PKG_NAME")));
        let _ = tokio::fs::remove_file(&path).await;
    }
}

/// The shared discovery directory for all ecoPrimals.
fn discovery_dir() -> io::Result<std::path::PathBuf> {
    config::discovery_dir()
}

/// Wait for SIGTERM or SIGINT. Returns which signal was received.
///
/// # Panics
///
/// Panics if signal registration fails (e.g. tokio runtime or OS limits).
/// Failure is unrecoverable — the process cannot gracefully shut down.
async fn wait_for_shutdown_signal() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm = signal(SignalKind::terminate()).or_exit("failed to register SIGTERM");
        let mut sigint = signal(SignalKind::interrupt()).or_exit("failed to register SIGINT");

        tokio::select! {
            _ = sigterm.recv() => "SIGTERM",
            _ = sigint.recv() => "SIGINT",
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .or_exit("failed to register Ctrl+C");
        "SIGINT"
    }
}

fn cmd_compile(
    input: &Path,
    output: Option<&Path>,
    arch: GpuArch,
    opt_level: u32,
    fp64_software: bool,
) -> UniBinExit {
    match commands::compile_file(input, arch, opt_level, fp64_software) {
        Ok(binary) => {
            let out_path = output.map_or_else(|| input.with_extension("bin"), Path::to_path_buf);
            if let Err(e) = std::fs::write(&out_path, &binary) {
                tracing::error!(path = %out_path.display(), error = %e, "failed to write output");
                return UniBinExit::GeneralError;
            }
            tracing::info!(path = %out_path.display(), size = binary.len(), "compiled");
            UniBinExit::Success
        }
        Err((status, msg)) => {
            tracing::error!(error = %msg, "compilation failed");
            match status {
                commands::ExitStatus::ConfigError => UniBinExit::ConfigError,
                _ => UniBinExit::GeneralError,
            }
        }
    }
}

async fn cmd_doctor() -> UniBinExit {
    match commands::run_doctor().await {
        Ok(report) => {
            tracing::info!(report = %report, "doctor");
            UniBinExit::Success
        }
        Err(e) => {
            tracing::error!(error = %e, "doctor failed");
            UniBinExit::GeneralError
        }
    }
}

#[cfg(test)]
#[path = "main_tests/mod.rs"]
mod tests;
