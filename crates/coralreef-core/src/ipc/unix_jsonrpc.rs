// SPDX-License-Identifier: AGPL-3.0-only
//! Unix socket JSON-RPC 2.0 server — newline-delimited protocol.
//!
//! Ecosystem primals discover coralReef via a Unix socket at
//! `$XDG_RUNTIME_DIR/biomeos/<primal>-<family_id>.sock`. After bind, a
//! capability-domain symlink is also created in that directory:
//! `{CORALREEF_CAPABILITY_DOMAIN}.sock` → `<primal>-<family_id>.sock` (relative),
//! per wateringHole `CAPABILITY_BASED_DISCOVERY_STANDARD` v1.1. The symlink is
//! only installed when the socket path includes the shared `biomeos` directory segment
//! (production layout); ad-hoc test paths skip it to avoid collisions.
//! This module serves the same `shader.compile.*` and `health.*` methods as the
//! TCP/HTTP server but over newline-delimited JSON on a Unix domain socket.
//!
//! Protocol: each request is a single JSON-RPC 2.0 object terminated
//! by `\n`. Responses are also newline-terminated.

#[cfg(unix)]
mod inner {
    use std::path::{Path, PathBuf};

    use tokio::net::UnixListener;
    use tokio::sync::watch;
    use tokio::task::JoinHandle;

    use super::super::newline_jsonrpc::process_newline_reader_writer;

    /// `true` when the bound socket path uses the shared ecosystem directory segment.
    fn path_in_ecosystem_namespace(socket_path: &Path) -> bool {
        socket_path
            .iter()
            .any(|c| c == std::ffi::OsStr::new(crate::config::ecosystem_namespace()))
    }

    /// After a successful bind, install `{domain}.sock` → instance socket (relative symlink).
    ///
    /// Returns the symlink path when created, for shutdown cleanup. Skipped when the socket
    /// is not under the ecosystem layout or when symlink creation fails (caller logs).
    fn install_capability_domain_symlink(bound_path: &Path) -> Option<PathBuf> {
        if !path_in_ecosystem_namespace(bound_path) {
            return None;
        }
        let parent = bound_path.parent()?;
        let link = parent.join(crate::config::capability_domain_socket_filename());
        if link.as_path() == bound_path {
            return None;
        }
        let target_name = bound_path.file_name()?;
        if link.exists() {
            let _ = std::fs::remove_file(&link);
        }
        match std::os::unix::fs::symlink(target_name, &link) {
            Ok(()) => Some(link),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    link = %link.display(),
                    target = %target_name.to_string_lossy(),
                    "failed to create capability-domain symlink (non-fatal)"
                );
                None
            }
        }
    }

    /// Build the socket path from an explicit base directory.
    ///
    /// When `runtime_dir` is `None`, falls back to `$TMPDIR`.
    /// Per wateringHole `PRIMAL_IPC_PROTOCOL` v3.0:
    /// `$XDG_RUNTIME_DIR/biomeos/<primal>-<family_id>.sock`
    #[must_use]
    pub fn unix_socket_path_for_base(runtime_dir: Option<PathBuf>) -> PathBuf {
        let base = runtime_dir.unwrap_or_else(std::env::temp_dir);
        base.join(crate::config::ecosystem_namespace())
            .join(crate::config::primal_socket_name())
    }

    /// Default socket path per wateringHole standard.
    ///
    /// `$XDG_RUNTIME_DIR/biomeos/<primal>-<family_id>.sock`
    /// Falls back to `$TMPDIR/biomeos/<primal>-<family_id>.sock` if XDG is unset.
    #[must_use]
    pub fn default_unix_socket_path() -> PathBuf {
        unix_socket_path_for_base(std::env::var("XDG_RUNTIME_DIR").ok().map(PathBuf::from))
    }

    /// Start a Unix socket JSON-RPC server.
    ///
    /// Returns the socket path and a join handle. The server runs until
    /// `shutdown_rx` receives a signal.
    ///
    /// # Errors
    ///
    /// Returns an error if the socket cannot be bound.
    pub async fn start_unix_jsonrpc_server(
        path: &Path,
        mut shutdown_rx: watch::Receiver<()>,
    ) -> Result<(PathBuf, JoinHandle<()>), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        let bound_path = path.to_path_buf();
        let cleanup_path = bound_path.clone();
        let cleanup_capability_link = install_capability_domain_symlink(&bound_path);

        tracing::info!(path = %bound_path.display(), "Unix JSON-RPC server listening");

        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept = listener.accept() => {
                        match accept {
                            Ok((stream, _addr)) => {
                                tokio::spawn(async move {
                                    let (reader, writer) = stream.into_split();
                                    process_newline_reader_writer(reader, writer).await;
                                });
                            }
                            Err(e) => {
                                tracing::warn!("Unix accept error: {e}");
                            }
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        break;
                    }
                }
            }
            let _ = std::fs::remove_file(&cleanup_path);
            if let Some(ref cap_link) = cleanup_capability_link {
                let _ = std::fs::remove_file(cap_link);
            }
        });

        Ok((bound_path, handle))
    }
}

#[cfg(all(unix, test))]
pub use super::newline_jsonrpc::make_response;
#[cfg(unix)]
pub use inner::unix_socket_path_for_base;
#[cfg(unix)]
pub use inner::{default_unix_socket_path, start_unix_jsonrpc_server};

#[cfg(all(unix, test))]
mod unix_jsonrpc_unit_tests {
    //! Focused coverage for socket path construction and bind lifecycle in this module.

    use std::path::Path;

    use super::{start_unix_jsonrpc_server, unix_socket_path_for_base};
    use crate::config;
    use crate::ipc::test_helpers;

    const TEMP_RUNTIME_LEAF: &str = "coralreef_unix_jsonrpc_unit_tests";

    #[test]
    fn unix_socket_path_for_base_joins_ecosystem_namespace_and_primal_socket_filename() {
        let base = std::env::temp_dir().join(TEMP_RUNTIME_LEAF);
        let path = unix_socket_path_for_base(Some(base.clone()));
        assert!(path.starts_with(&base));
        assert!(
            path.ends_with(Path::new(&config::primal_socket_name())),
            "expected .../<namespace>/{}",
            config::primal_socket_name()
        );
        let ns = path
            .parent()
            .and_then(std::path::Path::file_name)
            .and_then(|n| n.to_str());
        assert_eq!(ns, Some(config::ecosystem_namespace()));
    }

    #[test]
    fn unix_socket_path_for_base_with_none_uses_temp_dir_fallback() {
        let path = unix_socket_path_for_base(None);
        assert!(
            path.ends_with(Path::new(&config::primal_socket_name())),
            "fallback base should still end with primal socket name"
        );
    }

    #[tokio::test]
    async fn start_unix_jsonrpc_server_removes_stale_socket_file_before_bind() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir
            .path()
            .join(format!("stale-socket-{}.sock", std::process::id()));
        std::fs::write(&sock_path, b"stale").expect("seed stale socket path");
        let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
        let (_bound, handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
            .await
            .expect("bind after removing stale file");
        let _: Result<(), _> = shutdown_tx.send(());
        handle.await.ok();
    }

    #[tokio::test]
    async fn start_unix_jsonrpc_server_installs_capability_symlink_when_under_biomeos() {
        let dir = tempfile::tempdir().expect("tempdir");
        let biomeos = dir.path().join(config::ecosystem_namespace());
        let sock_path = biomeos.join(format!("instance-{}.sock", std::process::id()));
        let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
        let (_bound, handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
            .await
            .expect("bind under biomeos layout");
        let domain_link = biomeos.join(config::capability_domain_socket_filename());
        if domain_link != sock_path {
            assert!(
                domain_link.is_symlink() || domain_link.exists(),
                "expected capability-domain symlink at {}",
                domain_link.display()
            );
        }
        let _: Result<(), _> = shutdown_tx.send(());
        handle.await.ok();
        assert!(
            !sock_path.exists(),
            "instance socket path should be removed on shutdown"
        );
    }
}
