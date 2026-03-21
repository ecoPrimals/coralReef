// SPDX-License-Identifier: AGPL-3.0-only
//! Unix socket JSON-RPC 2.0 server — newline-delimited protocol.
//!
//! Ecosystem primals discover coralReef via a Unix socket at
//! `$XDG_RUNTIME_DIR/biomeos/<primal>-<family_id>.sock`. This module
//! serves the same `shader.compile.*` and `health.*` methods as the
//! TCP/HTTP server but over newline-delimited JSON on a Unix domain socket.
//!
//! Protocol: each request is a single JSON-RPC 2.0 object terminated
//! by `\n`. Responses are also newline-terminated.

#[cfg(unix)]
mod inner {
    use std::path::{Path, PathBuf};

    use serde::{Deserialize, Serialize};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;
    use tokio::sync::watch;
    use tokio::task::JoinHandle;

    use crate::ipc::error::IpcServiceError;
    use crate::service;

    #[derive(Deserialize)]
    struct JsonRpcRequest {
        jsonrpc: String,
        method: String,
        #[serde(default)]
        params: serde_json::Value,
        id: serde_json::Value,
    }

    #[derive(Serialize)]
    struct JsonRpcResponse {
        jsonrpc: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<JsonRpcError>,
        id: serde_json::Value,
    }

    #[derive(Serialize)]
    struct JsonRpcError {
        code: i32,
        message: String,
    }

    fn extract_params<T: serde::de::DeserializeOwned>(
        mut params: serde_json::Value,
    ) -> Result<T, IpcServiceError> {
        if let Some(arr) = params.as_array_mut() {
            if arr.is_empty() {
                return Err(IpcServiceError::dispatch("missing request parameter"));
            }
            serde_json::from_value(arr.remove(0))
                .map_err(|e| IpcServiceError::dispatch(format!("invalid params: {e}")))
        } else if params.is_object() {
            serde_json::from_value(params)
                .map_err(|e| IpcServiceError::dispatch(format!("invalid params: {e}")))
        } else {
            Err(IpcServiceError::dispatch("params must be array or object"))
        }
    }

    /// Route a JSON-RPC method call to the appropriate handler.
    ///
    /// # Errors
    ///
    /// Returns `IpcServiceError` if the method is unknown, params are
    /// invalid, or the handler itself fails.
    pub fn dispatch(
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, IpcServiceError> {
        match method {
            "shader.compile.status" => {
                let health = service::handle_health();
                serde_json::to_value(health).map_err(|e| IpcServiceError::internal(e.to_string()))
            }
            "shader.compile.capabilities" => {
                let health = service::handle_health();
                serde_json::to_value(health.supported_archs)
                    .map_err(|e| IpcServiceError::internal(e.to_string()))
            }
            "shader.compile.wgsl" => {
                let req: service::CompileWgslRequest = extract_params(params)?;
                match service::handle_compile_wgsl(&req) {
                    Ok(resp) => serde_json::to_value(resp)
                        .map_err(|e| IpcServiceError::internal(e.to_string())),
                    Err(e) => Err(IpcServiceError::handler(e.to_string())),
                }
            }
            "shader.compile.spirv" => {
                let req: service::CompileRequest = extract_params(params)?;
                match service::handle_compile(&req) {
                    Ok(resp) => serde_json::to_value(resp)
                        .map_err(|e| IpcServiceError::internal(e.to_string())),
                    Err(e) => Err(IpcServiceError::handler(e.to_string())),
                }
            }
            "shader.compile.wgsl.multi" => {
                let req: service::MultiDeviceCompileRequest = extract_params(params)?;
                match service::handle_compile_wgsl_multi(req) {
                    Ok(resp) => serde_json::to_value(resp)
                        .map_err(|e| IpcServiceError::internal(e.to_string())),
                    Err(e) => Err(IpcServiceError::handler(e.to_string())),
                }
            }
            "health.check" => {
                let resp = service::handle_health_check();
                serde_json::to_value(resp).map_err(|e| IpcServiceError::internal(e.to_string()))
            }
            "health.liveness" => {
                let resp = service::handle_health_liveness();
                serde_json::to_value(resp).map_err(|e| IpcServiceError::internal(e.to_string()))
            }
            "health.readiness" => {
                let resp = service::handle_health_readiness();
                serde_json::to_value(resp).map_err(|e| IpcServiceError::internal(e.to_string()))
            }
            other => Err(IpcServiceError::dispatch(format!(
                "method not found: {other}"
            ))),
        }
    }

    /// Serialize a JSON-RPC 2.0 response from a handler result.
    pub fn make_response(
        id: serde_json::Value,
        result: Result<serde_json::Value, IpcServiceError>,
    ) -> String {
        let resp = match result {
            Ok(val) => JsonRpcResponse {
                jsonrpc: "2.0",
                result: Some(val),
                error: None,
                id,
            },
            Err(e) => JsonRpcResponse {
                jsonrpc: "2.0",
                result: None,
                error: Some(JsonRpcError {
                    code: e.phase.jsonrpc_code(),
                    message: e.to_string(),
                }),
                id,
            },
        };
        serde_json::to_string(&resp).unwrap_or_else(|_| {
            r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"internal error"},"id":null}"#
                .to_owned()
        })
    }

    /// Build the socket path from an explicit base directory.
    ///
    /// When `runtime_dir` is `None`, falls back to `$TMPDIR`.
    /// Per wateringHole `PRIMAL_IPC_PROTOCOL` v3.0:
    /// `$XDG_RUNTIME_DIR/biomeos/<primal>-<family_id>.sock`
    #[must_use]
    pub fn unix_socket_path_for_base(runtime_dir: Option<PathBuf>) -> PathBuf {
        let base = runtime_dir.unwrap_or_else(std::env::temp_dir);
        base.join(crate::config::ECOSYSTEM_NAMESPACE)
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

        tracing::info!(path = %bound_path.display(), "Unix JSON-RPC server listening");

        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept = listener.accept() => {
                        match accept {
                            Ok((stream, _addr)) => {
                                tokio::spawn(async move {
                                    let (reader, mut writer) = stream.into_split();
                                    let mut lines = BufReader::new(reader).lines();
                                    while let Ok(Some(line)) = lines.next_line().await {
                                        let line = line.trim().to_owned();
                                        if line.is_empty() {
                                            continue;
                                        }
                                        let resp = match serde_json::from_str::<JsonRpcRequest>(&line) {
                                            Ok(req) => {
                                                if req.jsonrpc == "2.0" {
                                                    let result =
                                                        dispatch(&req.method, req.params);
                                                    make_response(req.id, result)
                                                } else {
                                                    make_response(
                                                        req.id,
                                                        Err(IpcServiceError::dispatch(format!(
                                                            "invalid jsonrpc version: {}",
                                                            req.jsonrpc
                                                        ))),
                                                    )
                                                }
                                            }
                                            Err(e) => {
                                                make_response(
                                                    serde_json::Value::Null,
                                                    Err(IpcServiceError::transport(format!("parse error: {e}"))),
                                                )
                                            }
                                        };
                                        let msg = format!("{resp}\n");
                                        if writer.write_all(msg.as_bytes()).await.is_err() {
                                            break;
                                        }
                                    }
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
        });

        Ok((bound_path, handle))
    }
}

#[cfg(all(unix, any(test, feature = "e2e")))]
pub use inner::dispatch;
#[cfg(all(unix, test))]
pub use inner::make_response;
#[cfg(unix)]
pub use inner::unix_socket_path_for_base;
#[cfg(unix)]
pub use inner::{default_unix_socket_path, start_unix_jsonrpc_server};
