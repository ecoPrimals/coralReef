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
use coralreef_core::env_keys;
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
    /// Start the IPC server (JSON-RPC 2.0, optionally tarpc).
    Server {
        /// TCP port for JSON-RPC listener (standard envelope).
        /// Binds to 127.0.0.1:PORT. Use --rpc-bind for full address control.
        #[arg(long, conflicts_with = "rpc_bind")]
        port: Option<u16>,

        /// Transport bind mode. Overrides `$PRIMAL_BIND_MODE` env var.
        /// Values: `tcp_only`, `fallback`, `auto`.
        #[arg(long)]
        bind_mode: Option<String>,

        /// Full bind address (host:port) for JSON-RPC TCP.
        /// Respects `$CORALREEF_TCP_BIND` env. Prefer --port for standard deployments.
        #[arg(long, hide = true)]
        rpc_bind: Option<String>,

        /// Unix domain socket path for JSON-RPC.
        /// Overrides the default `$XDG_RUNTIME_DIR/biomeos/<primal>.sock`.
        #[arg(long)]
        socket: Option<std::path::PathBuf>,

        /// Bind address for tarpc server.
        /// TCP: `127.0.0.1:0`; Unix socket: `unix:///path/to/socket`.
        /// Defaults to platform-native transport (Unix socket on Linux/macOS).
        #[cfg(feature = "tarpc-transport")]
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
    service::mark_startup();

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
            port,
            bind_mode,
            rpc_bind,
            socket,
            #[cfg(feature = "tarpc-transport")]
            tarpc_bind,
        } => {
            let effective_bind = resolve_effective_bind(port, rpc_bind.as_deref());
            #[cfg(feature = "tarpc-transport")]
            let tarpc_bind = tarpc_bind.unwrap_or_else(ipc::default_tarpc_bind);
            #[cfg(feature = "tarpc-transport")]
            {
                cmd_server(
                    &effective_bind,
                    &tarpc_bind,
                    socket.as_deref(),
                    bind_mode.as_deref(),
                )
                .await
            }
            #[cfg(not(feature = "tarpc-transport"))]
            {
                cmd_server(&effective_bind, socket.as_deref(), bind_mode.as_deref()).await
            }
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

/// Resolve effective TCP bind address from standard envelope flags.
///
/// Priority: `--rpc-bind` (deprecated full address) > `--port` (standard) >
/// `$CORALREEF_TCP_BIND` env > `127.0.0.1:0` (OS-assigned).
fn resolve_effective_bind(port: Option<u16>, rpc_bind: Option<&str>) -> String {
    if let Some(addr) = rpc_bind {
        return addr.to_owned();
    }
    if let Some(p) = port {
        return format!("127.0.0.1:{p}");
    }
    default_tcp_bind()
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
    if let Ok(ms) = std::env::var(env_keys::CORALREEF_TEST_SHUTDOWN_JOIN_TIMEOUT_MS) {
        if let Ok(ms) = ms.parse::<u64>() {
            return std::time::Duration::from_millis(ms);
        }
    }
    config::DEFAULT_SHUTDOWN_TIMEOUT
}

fn shutdown_join_timeout_elapsed_message(join_timeout: std::time::Duration) -> String {
    format!("shutdown timed out after {join_timeout:?}")
}

/// Log NUCLEUS composition environment variables at startup for diagnostics.
fn log_composition_env() {
    #[allow(deprecated)]
    let legacy_btsp = config::security_provider_socket_legacy().map(|p| p.display().to_string());
    let vars = [
        (
            "BTSP_PROVIDER_SOCKET",
            config::btsp_provider_socket().map(|p| p.display().to_string()),
        ),
        ("BEARDOG_SOCKET (deprecated)", legacy_btsp),
        (
            "DISCOVERY_SOCKET",
            config::discovery_socket().map(|p| p.display().to_string()),
        ),
        (
            "FAMILY_SEED",
            config::family_seed().map(|_| "<set>".to_owned()),
        ),
        (
            env_keys::BIOMEOS_SOCKET_DIR,
            std::env::var(env_keys::BIOMEOS_SOCKET_DIR).ok(),
        ),
        (
            env_keys::TRANSPORT_ENDPOINT,
            std::env::var(env_keys::TRANSPORT_ENDPOINT).ok(),
        ),
    ];
    for (name, val) in vars {
        if let Some(v) = val {
            tracing::info!(env = name, value = v, "composition env");
        } else {
            tracing::debug!(env = name, "composition env not set");
        }
    }
}

#[cfg(feature = "tarpc-transport")]
/// When composition passes `--tarpc-bind unix:///path/coralreef-{family}.sock`,
/// the ecosystem expects that socket to speak JSON-RPC 2.0 — not tarpc binary.
/// This function separates the two:
/// - JSON-RPC takes the composition-expected path (returned as the override)
/// - tarpc moves to a `-tarpc` suffixed socket
///
/// For TCP binds (or absent Unix prefix), both return the original bind string
/// and no Unix override.
fn resolve_uds_binds(tarpc_bind: &str) -> (String, Option<std::path::PathBuf>) {
    const UNIX_PREFIX: &str = "unix://";
    let Some(path_str) = tarpc_bind.strip_prefix(UNIX_PREFIX) else {
        return (tarpc_bind.to_owned(), None);
    };
    let path = std::path::PathBuf::from(path_str);
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    if stem.ends_with("-tarpc") {
        return (tarpc_bind.to_owned(), None);
    }
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let tarpc_path = path.extension().and_then(|e| e.to_str()).map_or_else(
        || parent.join(format!("{stem}-tarpc")),
        |ext| parent.join(format!("{stem}-tarpc.{ext}")),
    );
    tracing::info!(
        jsonrpc_uds = %path.display(),
        tarpc_uds = %tarpc_path.display(),
        "separated UDS binds: JSON-RPC on main socket, tarpc on dedicated socket"
    );
    (format!("{UNIX_PREFIX}{}", tarpc_path.display()), Some(path))
}

#[cfg(feature = "tarpc-transport")]
async fn cmd_server(
    rpc_bind: &str,
    tarpc_bind: &str,
    socket_override: Option<&std::path::Path>,
    bind_mode: Option<&str>,
) -> UniBinExit {
    use ipc::transport::{ResolvedBind, resolve_bind_with_mode};

    if let Err(e) = config::validate_insecure_guard() {
        tracing::error!(error = %e, "configuration rejected");
        return UniBinExit::ConfigError;
    }

    tracing::info!("{} server starting", env!("CARGO_PKG_NAME"));
    log_composition_env();

    let bind = match resolve_bind_with_mode(rpc_bind, socket_override, bind_mode) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = %e, "TRANSPORT_ENDPOINT resolution failed");
            return UniBinExit::ConfigError;
        }
    };

    let skip_tarpc = matches!(bind, ResolvedBind::TcpOnly { .. });
    let (tarpc_actual_bind, unix_jsonrpc_override) = resolve_uds_binds(tarpc_bind);

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());

    let mut rpc_handle: Option<tokio::task::JoinHandle<()>> = None;
    let mut rpc_addr: Option<std::net::SocketAddr> = None;
    #[cfg(unix)]
    let mut unix_jsonrpc_path: Option<std::path::PathBuf> = None;
    #[cfg(unix)]
    let mut unix_jsonrpc_handle: Option<tokio::task::JoinHandle<()>> = None;

    match &bind {
        ResolvedBind::TcpOnly { addr } => {
            tracing::info!(addr, tarpc_bind, "binding TCP (transport-injected) + tarpc");
            match ipc::start_newline_tcp_jsonrpc(addr, shutdown_rx.clone()).await {
                Ok((bound, handle)) => {
                    rpc_addr = Some(bound);
                    rpc_handle = Some(handle);
                }
                Err(e) => {
                    tracing::error!(error = %e, "failed to start TCP JSON-RPC server");
                    return UniBinExit::GeneralError;
                }
            }
        }
        #[cfg(unix)]
        ResolvedBind::UdsOnly { path } => {
            tracing::info!(path = %path.display(), tarpc_bind, "binding UDS (transport-injected) + tarpc");
            match ipc::start_unix_jsonrpc_server(path, shutdown_rx.clone()).await {
                Ok((_path, handle)) => {
                    unix_jsonrpc_path = Some(path.clone());
                    unix_jsonrpc_handle = Some(handle);
                }
                Err(e) => {
                    tracing::error!(error = %e, "failed to start Unix JSON-RPC server");
                    return UniBinExit::GeneralError;
                }
            }
        }
        #[cfg(not(unix))]
        ResolvedBind::UdsOnly { .. } => {
            tracing::error!("UDS transport injection not supported on this platform");
            return UniBinExit::ConfigError;
        }
        ResolvedBind::Both {
            tcp_bind,
            socket_override: sock_ovr,
        } => {
            tracing::info!(tcp_bind, tarpc_bind, "binding addresses (standalone mode)");
            match ipc::start_newline_tcp_jsonrpc(tcp_bind, shutdown_rx.clone()).await {
                Ok((bound, handle)) => {
                    rpc_addr = Some(bound);
                    rpc_handle = Some(handle);
                }
                Err(e) => {
                    tracing::error!(error = %e, "failed to start JSON-RPC server");
                    return UniBinExit::GeneralError;
                }
            }
            #[cfg(unix)]
            {
                let path = sock_ovr
                    .clone()
                    .or(unix_jsonrpc_override)
                    .unwrap_or_else(ipc::default_unix_socket_path);
                match ipc::start_unix_jsonrpc_server(&path, shutdown_rx.clone()).await {
                    Ok((_p, handle)) => {
                        tracing::info!(path = %path.display(), "Unix JSON-RPC server started");
                        unix_jsonrpc_path = Some(path);
                        unix_jsonrpc_handle = Some(handle);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Unix JSON-RPC server failed to start (ecosystem primal discovery degraded)");
                    }
                }
            }
        }
    }

    let mut tarpc_bound: Option<ipc::BoundAddr> = None;
    let mut tarpc_handle: Option<tokio::task::JoinHandle<()>> = None;

    if skip_tarpc {
        tracing::info!(
            "PRIMAL_BIND_MODE=tcp_only — skipping tarpc server (JSON-RPC TCP serves all methods)"
        );
    } else {
        match ipc::start_tarpc_server(&tarpc_actual_bind, shutdown_rx.clone()).await {
            Ok((bound, handle)) => {
                tarpc_bound = Some(bound);
                tarpc_handle = Some(handle);
            }
            Err(e) if tarpc_actual_bind.starts_with("unix://") => {
                tracing::warn!(
                    error = %e,
                    bind = %tarpc_actual_bind,
                    "tarpc Unix socket failed — falling back to TCP tarpc"
                );
                let tcp_fallback = ipc::FALLBACK_TCP_BIND;
                match ipc::start_tarpc_server(tcp_fallback, shutdown_rx.clone()).await {
                    Ok((bound, handle)) => {
                        tarpc_bound = Some(bound);
                        tarpc_handle = Some(handle);
                        tracing::info!("tarpc TCP fallback started");
                    }
                    Err(e2) => {
                        tracing::warn!(error = %e2, "tarpc TCP fallback also failed — continuing without tarpc");
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to start tarpc server");
                if let Some(h) = &rpc_handle {
                    h.abort();
                }
                return UniBinExit::GeneralError;
            }
        }
    }

    let mut transports = Vec::new();
    if let Some(addr) = rpc_addr {
        transports.push(coralreef_core::capability::Transport {
            protocol: "jsonrpc".into(),
            address: addr.to_string().into(),
        });
    }
    if let Some(ref bound) = tarpc_bound {
        transports.push(coralreef_core::capability::Transport {
            protocol: format!("tarpc+{}", bound.protocol()).into(),
            address: bound.to_string().into(),
        });
    }
    #[cfg(unix)]
    if let Some(ref path) = unix_jsonrpc_path {
        transports.push(coralreef_core::capability::Transport {
            protocol: "jsonrpc+unix".into(),
            address: format!("unix://{}", path.display()).into(),
        });
    }
    let desc = coralreef_core::capability::self_description();
    let desc = coralreef_core::capability::with_transports(desc, transports);
    tracing::info!(
        rpc_addr = ?rpc_addr,
        tarpc_addr = ?tarpc_bound,
        provides = ?desc.provides.iter().map(|c| &c.id).collect::<Vec<_>>(),
        requires = ?desc.requires.iter().map(|c| &c.id).collect::<Vec<_>>(),
        "{} ready — capability advertisement prepared", env!("CARGO_PKG_NAME")
    );

    service::set_identity_from_self_description(&desc);

    if let Err(e) = write_discovery_file(&desc).await {
        tracing::warn!(error = %e, "failed to write discovery file (peers must use fallback discovery)");
    }

    write_pid_file();

    coralreef_core::ecosystem::spawn_registration(desc);

    let signal_received = wait_for_shutdown_signal().await;
    tracing::info!(signal = ?signal_received, "received shutdown signal, stopping servers");

    let _ = shutdown_tx.send(());

    let join_timeout = shutdown_join_timeout();
    let shutdown_result = tokio::time::timeout(join_timeout, async move {
        if let Some(h) = rpc_handle {
            if let Err(e) = h.await {
                tracing::warn!(error = %e, "JSON-RPC task join failed during shutdown");
            }
        }
        if let Some(h) = tarpc_handle {
            if let Err(e) = h.await {
                tracing::warn!(error = %e, "tarpc task join failed during shutdown");
            }
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
    remove_pid_file();

    UniBinExit::Signal
}

#[cfg(not(feature = "tarpc-transport"))]
async fn cmd_server(
    rpc_bind: &str,
    socket_override: Option<&std::path::Path>,
    bind_mode: Option<&str>,
) -> UniBinExit {
    use ipc::transport::{ResolvedBind, resolve_bind_with_mode};

    if let Err(e) = config::validate_insecure_guard() {
        tracing::error!(error = %e, "configuration rejected");
        return UniBinExit::ConfigError;
    }

    tracing::info!("{} server starting (JSON-RPC only)", env!("CARGO_PKG_NAME"));
    log_composition_env();

    let bind = match resolve_bind_with_mode(rpc_bind, socket_override, bind_mode) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = %e, "TRANSPORT_ENDPOINT resolution failed");
            return UniBinExit::ConfigError;
        }
    };

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());

    let mut rpc_handle: Option<tokio::task::JoinHandle<()>> = None;
    let mut rpc_addr: Option<std::net::SocketAddr> = None;
    #[cfg(unix)]
    let mut unix_jsonrpc_path: Option<std::path::PathBuf> = None;
    #[cfg(unix)]
    let mut unix_jsonrpc_handle: Option<tokio::task::JoinHandle<()>> = None;

    match &bind {
        ResolvedBind::TcpOnly { addr } => {
            tracing::info!(addr, "binding TCP (transport-injected)");
            match ipc::start_newline_tcp_jsonrpc(addr, shutdown_rx.clone()).await {
                Ok((bound, handle)) => {
                    rpc_addr = Some(bound);
                    rpc_handle = Some(handle);
                }
                Err(e) => {
                    tracing::error!(error = %e, "failed to start TCP JSON-RPC server");
                    return UniBinExit::GeneralError;
                }
            }
        }
        #[cfg(unix)]
        ResolvedBind::UdsOnly { path } => {
            tracing::info!(path = %path.display(), "binding UDS (transport-injected)");
            match ipc::start_unix_jsonrpc_server(path, shutdown_rx.clone()).await {
                Ok((_path, handle)) => {
                    unix_jsonrpc_path = Some(path.clone());
                    unix_jsonrpc_handle = Some(handle);
                }
                Err(e) => {
                    tracing::error!(error = %e, "failed to start Unix JSON-RPC server");
                    return UniBinExit::GeneralError;
                }
            }
        }
        #[cfg(not(unix))]
        ResolvedBind::UdsOnly { .. } => {
            tracing::error!("UDS transport injection not supported on this platform");
            return UniBinExit::ConfigError;
        }
        ResolvedBind::Both {
            tcp_bind,
            socket_override: sock_ovr,
        } => {
            tracing::info!(tcp_bind, "binding addresses (standalone mode)");
            match ipc::start_newline_tcp_jsonrpc(tcp_bind, shutdown_rx.clone()).await {
                Ok((bound, handle)) => {
                    rpc_addr = Some(bound);
                    rpc_handle = Some(handle);
                }
                Err(e) => {
                    tracing::error!(error = %e, "failed to start JSON-RPC server");
                    return UniBinExit::GeneralError;
                }
            }
            #[cfg(unix)]
            {
                let path = sock_ovr
                    .clone()
                    .unwrap_or_else(ipc::default_unix_socket_path);
                match ipc::start_unix_jsonrpc_server(&path, shutdown_rx.clone()).await {
                    Ok((_p, handle)) => {
                        tracing::info!(path = %path.display(), "Unix JSON-RPC server started");
                        unix_jsonrpc_path = Some(path);
                        unix_jsonrpc_handle = Some(handle);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Unix JSON-RPC server failed to start (ecosystem primal discovery degraded)");
                    }
                }
            }
        }
    }

    let mut transports = Vec::new();
    if let Some(addr) = rpc_addr {
        transports.push(coralreef_core::capability::Transport {
            protocol: "jsonrpc".into(),
            address: addr.to_string().into(),
        });
    }
    #[cfg(unix)]
    if let Some(ref path) = unix_jsonrpc_path {
        transports.push(coralreef_core::capability::Transport {
            protocol: "jsonrpc+unix".into(),
            address: format!("unix://{}", path.display()).into(),
        });
    }
    let desc = coralreef_core::capability::self_description();
    let desc = coralreef_core::capability::with_transports(desc, transports);
    tracing::info!(
        rpc_addr = ?rpc_addr,
        provides = ?desc.provides.iter().map(|c| &c.id).collect::<Vec<_>>(),
        requires = ?desc.requires.iter().map(|c| &c.id).collect::<Vec<_>>(),
        "{} ready — capability advertisement prepared", env!("CARGO_PKG_NAME")
    );

    service::set_identity_from_self_description(&desc);

    if let Err(e) = write_discovery_file(&desc).await {
        tracing::warn!(error = %e, "failed to write discovery file (peers must use fallback discovery)");
    }

    write_pid_file();

    coralreef_core::ecosystem::spawn_registration(desc);

    let signal_received = wait_for_shutdown_signal().await;
    tracing::info!(signal = ?signal_received, "received shutdown signal, stopping servers");

    let _ = shutdown_tx.send(());

    let join_timeout = shutdown_join_timeout();
    let shutdown_result = tokio::time::timeout(join_timeout, async move {
        if let Some(h) = rpc_handle {
            if let Err(e) = h.await {
                tracing::warn!(error = %e, "JSON-RPC task join failed during shutdown");
            }
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
    remove_pid_file();

    UniBinExit::Signal
}

use coralreef_core::server_lifecycle::{
    self, remove_discovery_file, remove_pid_file, wait_for_shutdown_signal, write_pid_file,
};

async fn write_discovery_file(
    desc: &coralreef_core::capability::SelfDescription,
) -> io::Result<()> {
    server_lifecycle::write_discovery_file(desc).await
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
        Err(e) => {
            tracing::error!(error = %e, "compilation failed");
            match e.exit_status() {
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
