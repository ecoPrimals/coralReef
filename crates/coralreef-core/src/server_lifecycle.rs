// SPDX-License-Identifier: AGPL-3.0-or-later
//! Server lifecycle utilities — discovery files, PID files, and signal handling.
//!
//! Extracted from `main.rs` to keep the binary entry point focused on CLI
//! parsing and orchestration, while lifecycle concerns live here.

use std::io;
use std::path::Path;

use crate::config;

/// Write a discovery file so peer primals can find this service.
///
/// File path: `{dir}/{CARGO_PKG_NAME}.json` where `dir` defaults to
/// the ecosystem discovery directory.
///
/// # Errors
///
/// Returns `io::Error` if the directory cannot be created or the file cannot be written.
pub async fn write_discovery_file(desc: &crate::capability::SelfDescription) -> io::Result<()> {
    write_discovery_file_to(&discovery_dir()?, desc).await
}

/// Write a discovery file into an explicit directory.
///
/// Separated from [`write_discovery_file`] so tests can target an isolated
/// temp directory instead of the shared `$XDG_RUNTIME_DIR/biomeos/` path.
///
/// # Errors
///
/// Returns `io::Error` if the directory cannot be created or the file cannot be written.
///
/// # Panics
///
/// Panics if `serde_json::Value` serialization fails (infallible for valid JSON).
pub async fn write_discovery_file_to(
    dir: &Path,
    desc: &crate::capability::SelfDescription,
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
pub async fn remove_discovery_file() {
    match discovery_dir() {
        Ok(dir) => remove_discovery_file_from(Some(dir.as_path())).await,
        Err(e) => {
            tracing::debug!(error = %e, "discovery dir unavailable, skipping file removal");
        }
    }
}

/// Remove a discovery file from an explicit directory.
pub async fn remove_discovery_file_from(dir: Option<&Path>) {
    if let Some(dir) = dir {
        let path = dir.join(format!("{}.json", env!("CARGO_PKG_NAME")));
        let _ = tokio::fs::remove_file(&path).await;
    }
}

/// The shared discovery directory for all ecoPrimals.
fn discovery_dir() -> io::Result<std::path::PathBuf> {
    config::discovery_dir()
}

/// Write a PID file alongside the socket for instant liveness checks.
///
/// Per `CAPABILITY_BASED_DISCOVERY_STANDARD` v1.3.0 §6.
pub fn write_pid_file() {
    let Ok(dir) = discovery_dir() else {
        return;
    };
    let path = dir.join(format!("{}.pid", env!("CARGO_PKG_NAME")));
    if let Err(e) = std::fs::write(&path, std::process::id().to_string()) {
        tracing::debug!(error = %e, path = %path.display(), "failed to write PID file");
    } else {
        tracing::debug!(path = %path.display(), pid = std::process::id(), "PID file written");
    }
}

/// Remove the PID file on shutdown.
pub fn remove_pid_file() {
    let Ok(dir) = discovery_dir() else {
        return;
    };
    let path = dir.join(format!("{}.pid", env!("CARGO_PKG_NAME")));
    let _ = std::fs::remove_file(&path);
}

/// Wait for SIGTERM or SIGINT. Returns which signal was received.
///
/// # Panics
///
/// Panics if signal registration fails (e.g. tokio runtime or OS limits).
pub async fn wait_for_shutdown_signal() -> &'static str {
    use crate::or_exit::OrExit;

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
