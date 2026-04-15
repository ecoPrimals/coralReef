// SPDX-License-Identifier: AGPL-3.0-or-later
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

    use tokio::io::AsyncBufReadExt;

    use super::super::newline_jsonrpc::process_newline_reader_writer;
    use crate::ipc::btsp;

    /// Peek timeout for first-byte BTSP protocol detection.
    const PEEK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

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
                                let (reader, writer) = stream.into_split();
                                let mut peeker = tokio::io::BufReader::new(reader);
                                let first_byte = match tokio::time::timeout(
                                    PEEK_TIMEOUT,
                                    peeker.fill_buf(),
                                )
                                .await
                                {
                                    Ok(Ok(buf)) => buf.first().copied(),
                                    _ => None,
                                };
                                let outcome = btsp::guard_from_first_byte(first_byte).await;
                                if !outcome.should_accept() {
                                    tracing::warn!(?outcome, "BTSP rejected connection");
                                    continue;
                                }
                                tokio::spawn(async move {
                                    process_newline_reader_writer(peeker, writer).await;
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
