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

    use tokio::net::UnixListener;
    use tokio::sync::watch;
    use tokio::task::JoinHandle;

    use tokio::io::AsyncBufReadExt;

    use super::super::newline_jsonrpc::{
        process_newline_after_brace_line, process_newline_reader_writer,
    };
    use crate::ipc::btsp;
    use crate::ipc::ipc_protocol::IpcProtocol;
    use crate::ipc::protocol_negotiation;
    use crate::ipc::transport::TransportStream;

    /// Peek timeout for first-byte BTSP protocol detection.
    const PEEK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

    /// G65 timeout for first-byte protocol negotiation detection.
    const G65_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(100);

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
    #[allow(
        dead_code,
        reason = "pub API used by integration tests and Unix embedders"
    )]
    pub fn unix_socket_path_for_base(runtime_dir: Option<PathBuf>) -> PathBuf {
        let base = runtime_dir.unwrap_or_else(std::env::temp_dir);
        base.join(crate::config::ecosystem_namespace())
            .join(crate::config::primal_socket_name())
    }

    /// Default socket path per wateringHole standard.
    ///
    /// Delegates to [`crate::config::default_socket_path`] for canonical 4-tier
    /// resolution (`$BIOMEOS_SOCKET_DIR` > `$XDG_RUNTIME_DIR` > `/run/{ns}` > `$TMPDIR`).
    #[must_use]
    pub fn default_unix_socket_path() -> PathBuf {
        crate::config::default_socket_path()
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

        #[cfg(feature = "tarpc-transport")]
        let tarpc_available = true;
        #[cfg(not(feature = "tarpc-transport"))]
        let tarpc_available = false;

        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept = listener.accept() => {
                        match accept {
                            Ok((stream, _addr)) => {
                                let transport = TransportStream::Unix(stream);
                                dispatch_connection(transport, tarpc_available).await;
                            }
                            Err(e) => {
                                tracing::warn!("accept error: {e}");
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

    /// Dispatch a newly accepted connection — G65 + G66 composed.
    ///
    /// Reads the first byte to determine the protocol framing, then
    /// dispatches to G65 negotiation, BTSP, or plain JSON-RPC.
    /// Transport-agnostic: works on UDS or TCP via `TransportStream`.
    async fn dispatch_connection(mut stream: TransportStream, tarpc_available: bool) {
        use tokio::io::AsyncReadExt;
        let mut first = [0u8; 1];
        let first_byte =
            match tokio::time::timeout(G65_TIMEOUT, stream.read_exact(&mut first)).await {
                Ok(Ok(_)) => Some(first[0]),
                Ok(Err(_)) => None,
                Err(_) => {
                    match tokio::time::timeout(
                        PEEK_TIMEOUT.saturating_sub(G65_TIMEOUT),
                        stream.read_exact(&mut first),
                    )
                    .await
                    {
                        Ok(Ok(_)) => Some(first[0]),
                        _ => None,
                    }
                }
            };

        match first_byte {
            Some(b'P') => {
                handle_g65_connection(stream, tarpc_available).await;
            }
            Some(b'{') => {
                handle_brace_connection(stream).await;
            }
            other => {
                let outcome = btsp::guard_from_first_byte(other).await;
                if !outcome.should_accept() {
                    tracing::warn!(?outcome, "BTSP rejected connection");
                    return;
                }
                let (reader, writer) = tokio::io::split(stream);
                let peeker = tokio::io::BufReader::new(reader);
                tokio::spawn(async move {
                    process_newline_reader_writer(peeker, writer).await;
                });
            }
        }
    }

    /// Handle G65 protocol negotiation on a `TransportStream`.
    ///
    /// Transport-agnostic: works on UDS, TCP, or any future transport.
    async fn handle_g65_connection(mut stream: TransportStream, tarpc_available: bool) {
        let server_supported = if tarpc_available {
            IpcProtocol::supported()
        } else {
            vec![IpcProtocol::JsonRpc]
        };

        let selected =
            protocol_negotiation::negotiate_server_after_p(&mut stream, &server_supported).await;

        match selected {
            #[cfg(feature = "tarpc-transport")]
            Some(IpcProtocol::Tarpc) => {
                tracing::info!("G65: dispatching to tarpc on negotiated stream");
                tokio::spawn(async move {
                    super::super::tarpc_transport::handle_tarpc_negotiated(stream).await;
                });
            }
            _ => {
                let (reader, writer) = tokio::io::split(stream);
                let peeker = tokio::io::BufReader::new(reader);
                tokio::spawn(async move {
                    process_newline_reader_writer(peeker, writer).await;
                });
            }
        }
    }

    /// Handle a `{`-prefixed connection (BTSP or plain JSON-RPC).
    ///
    /// Transport-agnostic: works on any `TransportStream`.
    async fn handle_brace_connection(stream: TransportStream) {
        let (reader, mut writer) = tokio::io::split(stream);
        let mut peeker = tokio::io::BufReader::new(reader);
        let mut line_rest = String::new();
        match tokio::time::timeout(PEEK_TIMEOUT, peeker.read_line(&mut line_rest)).await {
            Ok(Ok(0) | Err(_)) | Err(_) => return,
            Ok(Ok(_)) => {}
        }
        let first_line = format!("{{{line_rest}");

        if btsp::line_looks_like_btsp_client_hello(&first_line) {
            if btsp::relay_json_line_handshake(&first_line, &mut peeker, &mut writer)
                .await
                .is_err()
            {
                return;
            }
            tokio::spawn(async move {
                process_newline_reader_writer(peeker, writer).await;
            });
        } else {
            tokio::spawn(async move {
                process_newline_after_brace_line(first_line, peeker, writer).await;
            });
        }
    }
}

/// Handle a single connection with optional BTSP session context.
///
/// When `session_id` is `Some`, the handler recognises `btsp.negotiate` requests
/// and, upon successful negotiation to `chacha20-poly1305`, switches to encrypted
/// framing for the remainder of the connection.
///
/// Without a `session_id` (or after negotiation returns `"null"`), the handler
/// falls through to plain newline-delimited JSON-RPC.
#[cfg(unix)]
#[allow(
    dead_code,
    reason = "pub API for tests and e2e Phase 3 encrypted transport"
)]
pub async fn handle_connection<R, W>(reader: R, mut writer: W, session_id: Option<String>)
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use super::btsp_negotiate;
    use super::newline_jsonrpc::{dispatch_jsonrpc, make_response};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let mut lines = reader.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parsed: Result<serde_json::Value, _> = serde_json::from_str(trimmed);
        let Ok(req_val) = parsed else {
            let resp = make_response(
                serde_json::Value::Null,
                Err(super::error::IpcServiceError::transport("parse error")),
            );
            if writer.write_all(resp.as_bytes()).await.is_err()
                || writer.write_all(b"\n").await.is_err()
            {
                break;
            }
            let _ = writer.flush().await;
            continue;
        };

        let method = req_val
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let id = req_val
            .get("id")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let params = req_val
            .get("params")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        if method == "btsp.negotiate" {
            if let Ok(neg_req) =
                serde_json::from_value::<btsp_negotiate::NegotiateRequest>(params.clone())
            {
                match btsp_negotiate::handle_negotiate(&neg_req) {
                    Ok(neg_resp) => {
                        let result =
                            serde_json::to_value(&neg_resp).unwrap_or(serde_json::Value::Null);
                        let resp = make_response(id, Ok(result));
                        if writer.write_all(resp.as_bytes()).await.is_err()
                            || writer.write_all(b"\n").await.is_err()
                        {
                            break;
                        }
                        let _ = writer.flush().await;

                        if neg_resp.cipher == "chacha20-poly1305" {
                            let sid = session_id.as_deref().unwrap_or(&neg_req.session_id);
                            if let Some(keys) = btsp_negotiate::take_negotiated_keys(sid) {
                                process_encrypted_frames(&mut lines, &mut writer, keys).await;
                                return;
                            }
                        }
                        continue;
                    }
                    Err(e) => {
                        let resp = make_response(
                            id,
                            Err(super::error::IpcServiceError::handler(e.to_string())),
                        );
                        if writer.write_all(resp.as_bytes()).await.is_err()
                            || writer.write_all(b"\n").await.is_err()
                        {
                            break;
                        }
                        let _ = writer.flush().await;
                        continue;
                    }
                }
            }
        }

        let result = dispatch_jsonrpc(method, params);
        let resp = make_response(id, result);
        if writer.write_all(resp.as_bytes()).await.is_err()
            || writer.write_all(b"\n").await.is_err()
        {
            break;
        }
        let _ = writer.flush().await;
    }
}

/// Read encrypted frames, dispatch JSON-RPC, write encrypted responses.
#[cfg(unix)]
#[allow(
    dead_code,
    reason = "pub API for tests and e2e Phase 3 encrypted transport"
)]
async fn process_encrypted_frames<R, W>(
    lines: &mut tokio::io::Lines<R>,
    writer: &mut W,
    keys: super::btsp_negotiate::SessionKeys,
) where
    R: tokio::io::AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use super::newline_jsonrpc::{dispatch_jsonrpc, make_response};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let raw_reader = lines.get_mut();
    while let Ok(len) = tokio::io::AsyncReadExt::read_u32(raw_reader).await {
        let frame_len = len as usize;
        if frame_len == 0 || frame_len > 16 * 1024 * 1024 {
            break;
        }
        let mut frame = vec![0u8; frame_len];
        if raw_reader.read_exact(&mut frame).await.is_err() {
            break;
        }
        let plaintext = match keys.decrypt(&frame) {
            Ok(pt) => pt,
            Err(e) => {
                tracing::warn!(error = %e, "encrypted frame decrypt failed");
                break;
            }
        };
        let Ok(req_str) = std::str::from_utf8(&plaintext) else {
            break;
        };
        let Ok(req_val) = serde_json::from_str::<serde_json::Value>(req_str) else {
            break;
        };
        let method = req_val
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let id = req_val
            .get("id")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let params = req_val
            .get("params")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let result = dispatch_jsonrpc(method, params);
        let resp = make_response(id, result);

        match keys.encrypt(resp.as_bytes()) {
            Ok(encrypted) => {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "JSON-RPC responses are well under 4 GiB"
                )]
                let len_bytes = (encrypted.len() as u32).to_be_bytes();
                if writer.write_all(&len_bytes).await.is_err()
                    || writer.write_all(&encrypted).await.is_err()
                {
                    break;
                }
                let _ = writer.flush().await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "encrypted frame encrypt failed");
                break;
            }
        }
    }
}

#[cfg(unix)]
pub use inner::unix_socket_path_for_base;
#[cfg(unix)]
pub use inner::{default_unix_socket_path, start_unix_jsonrpc_server};

/// Returns a fallback socket path on non-Unix platforms.
///
/// On Windows, this returns a nominal path since UDS is unavailable.
/// The server bind will fail with `Unsupported` if actually invoked.
#[cfg(not(unix))]
#[must_use]
pub fn default_unix_socket_path() -> std::path::PathBuf {
    std::path::PathBuf::from("coralreef-core.sock")
}

/// Non-Unix stub: returns `Unsupported` since UDS is unavailable.
///
/// # Errors
///
/// Always returns [`std::io::ErrorKind::Unsupported`] on non-Unix platforms.
#[cfg(not(unix))]
pub async fn start_unix_jsonrpc_server(
    _path: &std::path::Path,
    _shutdown: tokio::sync::watch::Receiver<()>,
) -> Result<(std::path::PathBuf, tokio::task::JoinHandle<()>), std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Unix domain sockets are not available on this platform",
    ))
}

/// Non-Unix stub: computes a nominal socket path for the given base.
#[cfg(not(unix))]
#[must_use]
#[allow(
    dead_code,
    reason = "pub API parity with Unix; used by integration tests"
)]
pub fn unix_socket_path_for_base(base: &std::path::Path, _family_id: &str) -> std::path::PathBuf {
    base.join("coralreef-core.sock")
}
