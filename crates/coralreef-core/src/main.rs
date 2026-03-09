// SPDX-License-Identifier: AGPL-3.0-only
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
use tracing_subscriber::EnvFilter;

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
        /// Bind address for JSON-RPC server (TCP only — HTTP transport).
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
    /// Used by the panic hook via `abort()` — the OS maps this to exit code 3.
    #[allow(
        dead_code,
        reason = "used by panic hook via abort(); OS maps to exit code 3"
    )]
    InternalError = 3,
    Signal = 130,
}

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
        // Log structurally; tracing may not be initialized yet, so also eprintln as fallback
        eprintln!("internal error: panic: message={msg}, location={location:?}");
        std::process::abort();
    }));
}

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

    let (tarpc_bound, tarpc_handle) =
        match ipc::start_tarpc_server(tarpc_bind, shutdown_rx.clone()).await {
            Ok(x) => x,
            Err(e) => {
                tracing::error!(error = %e, "failed to start tarpc server");
                let _ = rpc_handle.stop();
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
                tracing::warn!(error = %e, "Unix JSON-RPC server failed to start (toadStool integration degraded)");
                None
            }
        }
    };

    let desc = coralreef_core::capability::self_description();
    let desc = coralreef_core::capability::with_transports(
        desc,
        vec![
            coralreef_core::capability::Transport {
                protocol: "jsonrpc".into(),
                address: rpc_addr.to_string().into(),
            },
            coralreef_core::capability::Transport {
                protocol: format!("tarpc+{}", tarpc_bound.protocol()).into(),
                address: tarpc_bound.to_string().into(),
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

    // File-based discovery: write transport info so peer primals can find us.
    if let Err(e) = write_discovery_file(&desc) {
        tracing::warn!(error = %e, "failed to write discovery file (peers must use fallback discovery)");
    }

    // Wait for SIGTERM or SIGINT (graceful shutdown)
    let signal_received = wait_for_shutdown_signal().await;
    tracing::info!(signal = ?signal_received, "received shutdown signal, stopping servers");

    // 1. Stop accepting new connections
    let _ = shutdown_tx.send(());
    let _ = rpc_handle.stop();

    let rpc_stopped = rpc_handle.clone().stopped();
    let shutdown_result = tokio::time::timeout(
        coralreef_core::config::DEFAULT_SHUTDOWN_TIMEOUT,
        async move {
            rpc_stopped.await;
            tarpc_handle.await.ok();
            #[cfg(unix)]
            if let Some(h) = unix_jsonrpc_handle {
                h.await.ok();
            }
        },
    )
    .await;

    if shutdown_result.is_err() {
        tracing::warn!(
            "shutdown timed out after {:?}",
            coralreef_core::config::DEFAULT_SHUTDOWN_TIMEOUT
        );
    }

    remove_discovery_file();

    UniBinExit::Signal
}

/// Write a discovery file so peer primals can find this service.
///
/// File path: `$XDG_RUNTIME_DIR/{ECOSYSTEM_NAMESPACE}/{CARGO_PKG_NAME}.json`
/// Contains transport addresses and capability summary.
fn write_discovery_file(desc: &coralreef_core::capability::SelfDescription) -> io::Result<()> {
    let dir = discovery_dir()?;
    std::fs::create_dir_all(&dir)?;
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

    let mut jsonrpc_transport = serde_json::json!({
        "tcp": jsonrpc_addr,
    });
    #[cfg(unix)]
    {
        let unix_sock_path = ipc::default_unix_socket_path();
        jsonrpc_transport["path"] = serde_json::Value::String(
            unix_sock_path.to_string_lossy().into_owned(),
        );
    }

    let discovery = serde_json::json!({
        "primal": env!("CARGO_PKG_NAME"),
        "pid": std::process::id(),
        "transports": {
            "jsonrpc": jsonrpc_transport,
            "tarpc": tarpc_addr,
        },
        "provides": desc.provides.iter().map(|c| &c.id).collect::<Vec<_>>(),
        "requires": desc.requires.iter().map(|c| &c.id).collect::<Vec<_>>(),
    });

    std::fs::write(
        &path,
        serde_json::to_string_pretty(&discovery).unwrap_or_default(),
    )?;
    tracing::info!(path = %path.display(), "wrote discovery file");
    Ok(())
}

/// Remove the discovery file on shutdown.
fn remove_discovery_file() {
    if let Ok(dir) = discovery_dir() {
        let path = dir.join(format!("{}.json", env!("CARGO_PKG_NAME")));
        if path.exists() {
            let _ = std::fs::remove_file(&path);
            tracing::debug!(path = %path.display(), "removed discovery file");
        }
    }
}

/// The shared discovery directory for all ecoPrimals.
fn discovery_dir() -> io::Result<std::path::PathBuf> {
    coralreef_core::config::discovery_dir()
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
            println!("{report}");
            UniBinExit::Success
        }
        Err(e) => {
            tracing::error!(error = %e, "doctor failed");
            UniBinExit::GeneralError
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coralreef_core::capability::{Capability, SelfDescription, Transport};

    #[test]
    fn parse_cli_doctor() {
        let cli = parse_cli_from(["coralreef", "doctor"]).unwrap();
        assert!(matches!(cli.command, Commands::Doctor));
    }

    #[test]
    fn parse_cli_server_defaults() {
        let cli = parse_cli_from(["coralreef", "server"]).unwrap();
        match &cli.command {
            Commands::Server {
                rpc_bind,
                tarpc_bind,
            } => {
                assert!(rpc_bind.contains("127.0.0.1"));
                assert!(tarpc_bind.is_none());
            }
            _ => panic!("expected Server command"),
        }
    }

    #[test]
    fn parse_cli_compile_minimal() {
        let cli = parse_cli_from(["coralreef", "compile", "input.wgsl"]).unwrap();
        match &cli.command {
            Commands::Compile { input, output, .. } => {
                assert_eq!(input.to_string_lossy(), "input.wgsl");
                assert!(output.is_none());
            }
            _ => panic!("expected Compile command"),
        }
    }

    #[test]
    fn parse_cli_compile_with_options() {
        let cli = parse_cli_from([
            "coralreef",
            "compile",
            "shader.wgsl",
            "--output",
            "out.bin",
            "--arch",
            "sm70",
            "--opt-level",
            "3",
        ])
        .unwrap();
        match &cli.command {
            Commands::Compile {
                input,
                output,
                arch,
                opt_level,
                ..
            } => {
                assert_eq!(input.to_string_lossy(), "shader.wgsl");
                assert_eq!(output.as_ref().unwrap().to_string_lossy(), "out.bin");
                assert_eq!(*arch, GpuArch::Sm70);
                assert_eq!(*opt_level, 3);
            }
            _ => panic!("expected Compile command"),
        }
    }

    #[test]
    fn parse_cli_rejects_missing_subcommand() {
        assert!(parse_cli_from(["coralreef"]).is_err());
    }

    #[test]
    fn parse_cli_rejects_unknown_subcommand() {
        let err = parse_cli_from(["coralreef", "nonexistent"]).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("subcommand"));
    }

    #[test]
    fn parse_cli_rejects_compile_without_input() {
        assert!(parse_cli_from(["coralreef", "compile"]).is_err());
    }

    #[test]
    fn install_panic_hook_sets_hook() {
        let prev = std::panic::take_hook();
        install_panic_hook();
        std::panic::set_hook(prev); // Restore so other tests can panic normally
    }

    #[test]
    fn unibin_exit_to_exit_code_success() {
        let ec: ExitCode = UniBinExit::Success.into();
        assert_eq!(ec, ExitCode::SUCCESS);
    }

    #[test]
    fn unibin_exit_to_exit_code_general_error() {
        let ec: ExitCode = UniBinExit::GeneralError.into();
        assert_eq!(ec, ExitCode::from(1u8));
    }

    #[test]
    fn unibin_exit_to_exit_code_config_error() {
        let ec: ExitCode = UniBinExit::ConfigError.into();
        assert_eq!(ec, ExitCode::from(2u8));
    }

    #[test]
    fn unibin_exit_to_exit_code_internal_error() {
        let ec: ExitCode = UniBinExit::InternalError.into();
        assert_eq!(ec, ExitCode::from(3u8));
    }

    #[test]
    fn unibin_exit_to_exit_code_signal() {
        let ec: ExitCode = UniBinExit::Signal.into();
        assert_eq!(ec, ExitCode::from(130u8));
    }

    #[test]
    fn discovery_dir_returns_path() {
        let dir = discovery_dir().unwrap();
        assert!(dir.ends_with(coralreef_core::config::ECOSYSTEM_NAMESPACE));
    }

    #[test]
    fn parse_cli_server_custom_bind_addresses() {
        let cli = parse_cli_from([
            "coralreef",
            "server",
            "--rpc-bind",
            "127.0.0.1:9999",
            "--tarpc-bind",
            "unix:///tmp/coralreef-test.sock",
        ])
        .unwrap();
        match &cli.command {
            Commands::Server {
                rpc_bind,
                tarpc_bind,
            } => {
                assert_eq!(rpc_bind, "127.0.0.1:9999");
                assert_eq!(
                    tarpc_bind.as_deref(),
                    Some("unix:///tmp/coralreef-test.sock")
                );
            }
            _ => panic!("expected Server command"),
        }
    }

    #[test]
    fn parse_cli_compile_with_target_and_opt_level() {
        let cli = parse_cli_from([
            "coralreef",
            "compile",
            "shader.wgsl",
            "--arch",
            "sm80",
            "--opt-level",
            "3",
        ])
        .unwrap();
        match &cli.command {
            Commands::Compile {
                input,
                arch,
                opt_level,
                ..
            } => {
                assert_eq!(input.to_string_lossy(), "shader.wgsl");
                assert_eq!(*arch, GpuArch::Sm80);
                assert_eq!(*opt_level, 3);
            }
            _ => panic!("expected Compile command"),
        }
    }

    #[test]
    fn parse_cli_log_level_global() {
        let cli = parse_cli_from(["coralreef", "--log-level", "debug", "doctor"]).unwrap();
        assert_eq!(cli.log_level, "debug");

        let cli =
            parse_cli_from(["coralreef", "--log-level", "trace", "compile", "x.wgsl"]).unwrap();
        assert_eq!(cli.log_level, "trace");
    }

    #[test]
    fn parse_cli_version_flag() {
        let result = parse_cli_from(["coralreef", "--version"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains(env!("CARGO_PKG_VERSION")),
            "version error should contain package version: {err_str}"
        );
    }

    #[test]
    fn parse_cli_rejects_invalid_arch() {
        let result = parse_cli_from([
            "coralreef",
            "compile",
            "input.wgsl",
            "--arch",
            "invalid_arch",
        ]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("arch")
                || err.to_string().to_lowercase().contains("invalid"),
            "invalid arch should produce parse error"
        );
    }

    #[tokio::test]
    async fn cmd_doctor_output_formatting() {
        let result = cmd_doctor().await;
        assert!(matches!(result, UniBinExit::Success));
        // Output is printed to stdout; run_doctor returns the report.
        // We verify cmd_doctor succeeds (exit Success) and run_doctor produces expected format.
        let report = commands::run_doctor().await.unwrap();
        assert!(report.contains("doctor"));
        assert!(report.contains("[OK]"));
        assert!(report.contains("Capabilities"));
        assert!(report.contains("Supported architectures"));
        assert!(report.contains("Diagnostic complete"));
    }

    #[test]
    fn cmd_compile_success_with_temp_file() {
        let tmp = std::env::temp_dir().join("coralreef_test_compile.wgsl");
        std::fs::write(&tmp, "@compute @workgroup_size(1)\nfn main() {}").unwrap();
        let out_path = tmp.with_extension("bin");
        let result = cmd_compile(&tmp, Some(out_path.as_path()), GpuArch::Sm70, 2, true);
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(&out_path);
        assert!(matches!(result, UniBinExit::Success));
    }

    #[test]
    fn cmd_compile_config_error_nonexistent_file() {
        let result = cmd_compile(
            std::path::Path::new("/nonexistent/path/shader.wgsl"),
            None,
            GpuArch::Sm70,
            2,
            true,
        );
        assert!(matches!(result, UniBinExit::ConfigError));
    }

    #[test]
    fn write_and_remove_discovery_file() {
        let desc = SelfDescription {
            provides: vec![Capability {
                id: "test.provide".into(),
                version: "1.0".into(),
                metadata: serde_json::Value::Null,
            }],
            requires: vec![],
            transports: vec![
                Transport {
                    protocol: "jsonrpc".into(),
                    address: "127.0.0.1:12345".into(),
                },
                Transport {
                    protocol: "tarpc+tcp".into(),
                    address: "127.0.0.1:12346".into(),
                },
            ],
        };

        write_discovery_file(&desc).unwrap();
        let dir = discovery_dir().unwrap();
        let path = dir.join(format!("{}.json", env!("CARGO_PKG_NAME")));
        assert!(path.exists(), "discovery file should exist after write");

        remove_discovery_file();
        assert!(!path.exists(), "discovery file should be removed");
    }
}
