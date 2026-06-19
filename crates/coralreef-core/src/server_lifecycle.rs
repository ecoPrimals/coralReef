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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{self, SelfDescription, Transport};

    fn test_desc() -> SelfDescription {
        let mut desc = capability::self_description();
        desc.transports = vec![
            Transport {
                protocol: "jsonrpc".into(),
                address: "127.0.0.1:9200".into(),
            },
            Transport {
                protocol: "tarpc-bincode".into(),
                address: "127.0.0.1:9201".into(),
            },
        ];
        desc
    }

    fn test_desc_no_transports() -> SelfDescription {
        let mut desc = capability::self_description();
        desc.transports.clear();
        desc
    }

    #[tokio::test]
    async fn write_and_read_discovery_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let desc = test_desc();
        write_discovery_file_to(tmp.path(), &desc)
            .await
            .expect("write");

        let path = tmp.path().join(format!("{}.json", env!("CARGO_PKG_NAME")));
        assert!(path.exists(), "discovery file should exist");

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse");

        assert_eq!(parsed["primal"], env!("CARGO_PKG_NAME"));
        assert_eq!(parsed["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(parsed["transports"]["jsonrpc"]["bind"], "127.0.0.1:9200");
        assert_eq!(parsed["transports"]["tarpc"]["bind"], "127.0.0.1:9201");
        assert!(
            parsed["provides"].as_array().is_some_and(|a| !a.is_empty()),
            "should advertise capabilities"
        );
    }

    #[tokio::test]
    async fn write_discovery_file_creates_parent_dirs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let nested = tmp.path().join("deep").join("nested").join("dir");
        let desc = test_desc();
        write_discovery_file_to(&nested, &desc)
            .await
            .expect("write to nested dir");
        let path = nested.join(format!("{}.json", env!("CARGO_PKG_NAME")));
        assert!(path.exists());
    }

    #[tokio::test]
    async fn write_discovery_file_no_transports_uses_empty_strings() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let desc = test_desc_no_transports();
        write_discovery_file_to(tmp.path(), &desc)
            .await
            .expect("write");

        let path = tmp.path().join(format!("{}.json", env!("CARGO_PKG_NAME")));
        let content = tokio::fs::read_to_string(&path).await.expect("read");
        let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse");

        assert_eq!(parsed["transports"]["jsonrpc"]["bind"], "");
        assert_eq!(parsed["transports"]["tarpc"]["bind"], "");
    }

    #[tokio::test]
    async fn remove_discovery_file_from_removes_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let desc = test_desc();
        write_discovery_file_to(tmp.path(), &desc)
            .await
            .expect("write");
        let path = tmp.path().join(format!("{}.json", env!("CARGO_PKG_NAME")));
        assert!(path.exists());

        remove_discovery_file_from(Some(tmp.path())).await;
        assert!(!path.exists(), "file should be removed");
    }

    #[tokio::test]
    async fn remove_discovery_file_from_none_is_noop() {
        remove_discovery_file_from(None).await;
    }

    #[tokio::test]
    async fn remove_discovery_file_from_nonexistent_is_noop() {
        let tmp = tempfile::tempdir().expect("tempdir");
        remove_discovery_file_from(Some(tmp.path())).await;
    }

    #[tokio::test]
    async fn write_overwrites_existing_discovery_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let desc1 = test_desc();
        write_discovery_file_to(tmp.path(), &desc1)
            .await
            .expect("first write");

        let mut desc2 = test_desc_no_transports();
        desc2.transports.push(Transport {
            protocol: "jsonrpc".into(),
            address: "127.0.0.1:7777".into(),
        });
        write_discovery_file_to(tmp.path(), &desc2)
            .await
            .expect("second write");

        let path = tmp.path().join(format!("{}.json", env!("CARGO_PKG_NAME")));
        let content = tokio::fs::read_to_string(&path).await.expect("read");
        let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse");
        assert_eq!(
            parsed["transports"]["jsonrpc"]["bind"], "127.0.0.1:7777",
            "second write should overwrite"
        );
    }

    #[tokio::test]
    async fn discovery_file_contains_pid() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let desc = test_desc();
        write_discovery_file_to(tmp.path(), &desc)
            .await
            .expect("write");

        let path = tmp.path().join(format!("{}.json", env!("CARGO_PKG_NAME")));
        let content = tokio::fs::read_to_string(&path).await.expect("read");
        let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse");
        assert_eq!(
            parsed["pid"].as_u64().expect("pid should be a number"),
            u64::from(std::process::id())
        );
    }

    #[tokio::test]
    async fn discovery_file_serializes_requires() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let desc = test_desc();
        write_discovery_file_to(tmp.path(), &desc)
            .await
            .expect("write");

        let path = tmp.path().join(format!("{}.json", env!("CARGO_PKG_NAME")));
        let content = tokio::fs::read_to_string(&path).await.expect("read");
        let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse");
        assert!(
            parsed.get("requires").is_some(),
            "discovery file must contain 'requires' field"
        );
    }

    #[tokio::test]
    async fn discovery_file_first_jsonrpc_transport_wins() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut desc = capability::self_description();
        desc.transports = vec![
            Transport {
                protocol: "jsonrpc".into(),
                address: "127.0.0.1:1111".into(),
            },
            Transport {
                protocol: "jsonrpc".into(),
                address: "127.0.0.1:2222".into(),
            },
        ];
        write_discovery_file_to(tmp.path(), &desc)
            .await
            .expect("write");

        let path = tmp.path().join(format!("{}.json", env!("CARGO_PKG_NAME")));
        let content = tokio::fs::read_to_string(&path).await.expect("read");
        let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse");
        let bind = parsed["transports"]["jsonrpc"]["bind"]
            .as_str()
            .expect("bind");
        assert_eq!(bind, "127.0.0.1:1111", "first jsonrpc should win");
    }

    #[tokio::test]
    async fn discovery_file_non_tarpc_protocol_yields_empty_tarpc_bind() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut desc = capability::self_description();
        desc.transports = vec![Transport {
            protocol: "grpc".into(),
            address: "127.0.0.1:5555".into(),
        }];
        write_discovery_file_to(tmp.path(), &desc)
            .await
            .expect("write");

        let path = tmp.path().join(format!("{}.json", env!("CARGO_PKG_NAME")));
        let content = tokio::fs::read_to_string(&path).await.expect("read");
        let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse");
        let tarpc_bind = parsed["transports"]["tarpc"]["bind"].as_str().unwrap_or("");
        assert!(tarpc_bind.is_empty(), "non-tarpc should yield empty bind");
    }

    #[tokio::test]
    async fn write_discovery_file_to_fails_when_target_is_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let blocking_dir = tmp.path().join(format!("{}.json", env!("CARGO_PKG_NAME")));
        std::fs::create_dir_all(&blocking_dir).expect("create blocking dir");

        let desc = test_desc();
        let result = write_discovery_file_to(tmp.path(), &desc).await;
        assert!(result.is_err(), "writing to a directory path should fail");
    }

    #[test]
    fn discovery_dir_returns_ok() {
        let dir = discovery_dir();
        assert!(dir.is_ok(), "discovery_dir should succeed in test env");
        assert!(
            !dir.unwrap().as_os_str().is_empty(),
            "discovery dir should be non-empty"
        );
    }

    #[tokio::test]
    async fn write_discovery_file_to_creates_parent_dirs_deep() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let nested = tmp.path().join("a").join("b").join("c");
        let desc = test_desc();
        let result = write_discovery_file_to(&nested, &desc).await;
        assert!(result.is_ok(), "should create nested dirs: {result:?}");
        let path = nested.join(format!("{}.json", env!("CARGO_PKG_NAME")));
        assert!(path.exists());
    }
}
