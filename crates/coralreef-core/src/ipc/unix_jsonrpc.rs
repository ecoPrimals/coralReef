// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unix socket JSON-RPC 2.0 server — newline-delimited protocol.
//!
//! Ecosystem primals discover coralReef via a Unix socket at
//! `$XDG_RUNTIME_DIR/biomeos/<primal>-<family_id>.sock`. After bind, a
//! capability-domain symlink is also created in that directory:
//! `{CORALREEF_CAPABILITY_DOMAIN}.sock` → `<primal>-<family_id>.sock` (relative),
//! per wateringHole `CAPABILITY_BASED_DISCOVERY_STANDARD` v1.1. The symlink is
//! only installed when the socket path includes the ecosystem namespace directory segment
//! (default `biomeos` via [`crate::config::ecosystem_namespace`]); ad-hoc test paths skip it.
//! This module serves the same `shader.compile.*` and `health.*` methods as the
//! TCP/HTTP server but over newline-delimited JSON on a Unix domain socket.
//!
//! Protocol: each request is a single JSON-RPC 2.0 object terminated
//! by `\n`. Responses are also newline-terminated.

#[cfg(unix)]
mod inner {
    use std::path::{Path, PathBuf};

    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;
    use tokio::sync::watch;
    use tokio::task::JoinHandle;

    use super::super::btsp_negotiate;
    use super::super::newline_jsonrpc::{
        dispatch_maybe_blocking, make_response, process_newline_reader_writer,
    };
    use crate::ipc::btsp;

    /// Peek timeout for first-byte BTSP protocol detection.
    const PEEK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

    use super::super::newline_jsonrpc::JsonRpcRequest;

    /// Maximum encrypted frame payload (8 MiB). Prevents unbounded allocations.
    const MAX_FRAME_LEN: u32 = 8 * 1024 * 1024;

    /// Handle a single connection: attempt Phase 3 negotiate on first line,
    /// then either enter encrypted frame loop or fall back to plaintext.
    pub async fn handle_connection<R, W>(mut reader: R, mut writer: W, session_id: Option<String>)
    where
        R: tokio::io::AsyncRead + tokio::io::AsyncBufRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        let mut first_line = String::new();
        match reader.read_line(&mut first_line).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }

        let trimmed = first_line.trim();
        if trimmed.is_empty() {
            process_newline_reader_writer(reader, writer).await;
            return;
        }

        let method = serde_json::from_str::<serde_json::Value>(trimmed)
            .ok()
            .and_then(|v| v.get("method")?.as_str().map(String::from));

        if method.as_deref() != Some("btsp.negotiate") {
            let resp = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
                Ok(req) if req.jsonrpc == "2.0" => {
                    let result = dispatch_maybe_blocking(&req.method, req.params).await;
                    make_response(req.id, result)
                }
                Ok(req) => make_response(
                    req.id,
                    Err(crate::ipc::error::IpcServiceError::dispatch(format!(
                        "invalid jsonrpc version: {}",
                        req.jsonrpc
                    ))),
                ),
                Err(e) => make_response(
                    serde_json::Value::Null,
                    Err(crate::ipc::error::IpcServiceError::transport(format!(
                        "parse error: {e}"
                    ))),
                ),
            };
            if writer.write_all(resp.as_bytes()).await.is_err()
                || writer.write_all(b"\n").await.is_err()
            {
                return;
            }
            let _ = writer.flush().await;
            process_newline_reader_writer(reader, writer).await;
            return;
        }

        let (cipher, resp_text) = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
            Ok(req) if req.jsonrpc == "2.0" => {
                let result = dispatch_maybe_blocking(&req.method, req.params).await;
                let cipher = result
                    .as_ref()
                    .ok()
                    .and_then(|v| v.get("cipher"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("null")
                    .to_owned();
                let text = make_response(req.id, result);
                (cipher, text)
            }
            _ => {
                process_newline_reader_writer(reader, writer).await;
                return;
            }
        };

        if writer.write_all(resp_text.as_bytes()).await.is_err()
            || writer.write_all(b"\n").await.is_err()
        {
            return;
        }
        let _ = writer.flush().await;

        if cipher != "chacha20-poly1305" {
            process_newline_reader_writer(reader, writer).await;
            return;
        }

        let Some(sid) = session_id else {
            tracing::warn!("btsp.negotiate returned chacha20-poly1305 but no session_id");
            process_newline_reader_writer(reader, writer).await;
            return;
        };

        let Some(keys) = btsp_negotiate::take_negotiated_keys(&sid) else {
            tracing::warn!(
                session_id = %sid,
                "btsp.negotiate: no negotiated keys found after chacha20-poly1305"
            );
            process_newline_reader_writer(reader, writer).await;
            return;
        };

        tracing::info!(session_id = %sid, "switching to encrypted frame loop");
        process_encrypted_frames(reader, writer, keys).await;
    }

    /// Encrypted frame loop: `[4B BE u32 len][payload]` → decrypt → dispatch → encrypt → write.
    async fn process_encrypted_frames<R, W>(
        mut reader: R,
        mut writer: W,
        keys: btsp_negotiate::SessionKeys,
    ) where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        while let Ok(len) = reader.read_u32().await {
            if len > MAX_FRAME_LEN {
                tracing::warn!(len, "encrypted frame exceeds maximum size — dropping");
                break;
            }
            let mut frame = vec![0u8; len as usize];
            if reader.read_exact(&mut frame).await.is_err() {
                break;
            }
            let plaintext = match keys.decrypt(&frame) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(error = %e, "decryption failed — dropping connection");
                    break;
                }
            };

            let Ok(text) = std::str::from_utf8(&plaintext) else {
                tracing::warn!("decrypted frame is not valid UTF-8 — dropping");
                break;
            };
            let request_str = text.trim();

            let resp = match serde_json::from_str::<JsonRpcRequest>(request_str) {
                Ok(req) if req.jsonrpc == "2.0" => {
                    let result = dispatch_maybe_blocking(&req.method, req.params).await;
                    make_response(req.id, result)
                }
                Ok(req) => make_response(
                    req.id,
                    Err(crate::ipc::error::IpcServiceError::dispatch(format!(
                        "invalid jsonrpc version: {}",
                        req.jsonrpc
                    ))),
                ),
                Err(e) => make_response(
                    serde_json::Value::Null,
                    Err(crate::ipc::error::IpcServiceError::transport(format!(
                        "parse error: {e}"
                    ))),
                ),
            };

            let Ok(encrypted) = keys.encrypt(resp.as_bytes()) else {
                tracing::error!("response encryption failed — dropping connection");
                break;
            };

            #[allow(
                clippy::cast_possible_truncation,
                reason = "MAX_FRAME_LEN bounds output"
            )]
            let frame_len = (encrypted.len() as u32).to_be_bytes();
            if writer.write_all(&frame_len).await.is_err()
                || writer.write_all(&encrypted).await.is_err()
            {
                break;
            }
            let _ = writer.flush().await;
        }
    }

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

    /// Build the socket path from an explicit base directory (integration test utility).
    ///
    /// When `runtime_dir` is `None`, uses the centralized 3-tier resolution
    /// (`BIOMEOS_SOCKET_DIR` → `XDG_RUNTIME_DIR/biomeos` → `/run/biomeos`).
    #[must_use]
    #[allow(dead_code, reason = "pub API consumed by integration tests, not the binary")]
    pub fn unix_socket_path_for_base(runtime_dir: Option<PathBuf>) -> PathBuf {
        let base = runtime_dir.map_or_else(crate::config::socket_dir, |d| {
            d.join(crate::config::ecosystem_namespace())
        });
        base.join(crate::config::primal_socket_name())
    }

    /// Default socket path per wateringHole standard.
    ///
    /// Uses centralized [`config::socket_dir()`]: `BIOMEOS_SOCKET_DIR` →
    /// `XDG_RUNTIME_DIR/biomeos` → `/run/biomeos`. Zero `/tmp` writes.
    #[must_use]
    pub fn default_unix_socket_path() -> PathBuf {
        crate::config::socket_dir().join(crate::config::primal_socket_name())
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
                                if first_byte.is_some_and(|b| b != b'{') {
                                    peeker.consume(1);
                                }
                                let session_id = outcome.session_id().map(str::to_owned);
                                tokio::spawn(async move {
                                    handle_connection(peeker, writer, session_id).await;
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
#[cfg(all(unix, test))]
pub(super) use inner::handle_connection;
#[cfg(unix)]
pub use inner::unix_socket_path_for_base;
#[cfg(unix)]
pub use inner::{default_unix_socket_path, start_unix_jsonrpc_server};
