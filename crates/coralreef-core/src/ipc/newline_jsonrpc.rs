// SPDX-License-Identifier: AGPL-3.0-or-later
//! Newline-delimited JSON-RPC 2.0 — shared dispatch and wire handling.
//!
//! Used by Unix socket and TCP listeners per wateringHole `PRIMAL_IPC_PROTOCOL` v3.1.

use std::net::SocketAddr;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::btsp;
use super::error::IpcServiceError;
use super::{CoralReefError, IpcError};
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
#[must_use = "returns the handler result or an error — check the result"]
pub fn dispatch_jsonrpc(
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, IpcServiceError> {
    match method {
        "shader.compile.status" => {
            let health = service::handle_health();
            serde_json::to_value(health).map_err(|e| IpcServiceError::internal(e.to_string()))
        }
        "shader.compile.capabilities" => {
            let caps = service::handle_compile_capabilities();
            serde_json::to_value(caps).map_err(|e| IpcServiceError::internal(e.to_string()))
        }
        "shader.compile.wgsl" => {
            let req: service::CompileWgslRequest = extract_params(params)?;
            match service::handle_compile_wgsl(&req) {
                Ok(resp) => {
                    serde_json::to_value(resp).map_err(|e| IpcServiceError::internal(e.to_string()))
                }
                Err(e) => Err(IpcServiceError::handler(e.to_string())),
            }
        }
        "shader.compile.spirv" => {
            let req: service::CompileRequest = extract_params(params)?;
            match service::handle_compile(&req) {
                Ok(resp) => {
                    serde_json::to_value(resp).map_err(|e| IpcServiceError::internal(e.to_string()))
                }
                Err(e) => Err(IpcServiceError::handler(e.to_string())),
            }
        }
        "shader.compile.wgsl.multi" => {
            let req: service::MultiDeviceCompileRequest = extract_params(params)?;
            match service::handle_compile_wgsl_multi(req) {
                Ok(resp) => {
                    serde_json::to_value(resp).map_err(|e| IpcServiceError::internal(e.to_string()))
                }
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
        "identity.get" => {
            let resp = service::handle_identity_get();
            serde_json::to_value(resp).map_err(|e| IpcServiceError::internal(e.to_string()))
        }
        "capability.list" => {
            let resp = service::handle_capability_list();
            serde_json::to_value(resp).map_err(|e| IpcServiceError::internal(e.to_string()))
        }
        other => Err(IpcServiceError::dispatch(format!(
            "method not found: {other}"
        ))),
    }
}

/// Serialize a JSON-RPC 2.0 response from a handler result.
#[must_use]
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

/// Legacy name for `dispatch_jsonrpc` — kept for integration tests and fuzzing.
///
/// # Errors
///
/// Returns [`IpcServiceError`] when the method is unknown, parameters are invalid,
/// or the handler fails — same as `dispatch_jsonrpc`.
#[cfg(any(test, feature = "e2e"))]
#[allow(
    dead_code,
    reason = "pub alias for tests/e2e; lint fires on bin target but not lib"
)]
pub fn dispatch(
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, IpcServiceError> {
    dispatch_jsonrpc(method, params)
}

/// Read/write JSON-RPC lines on a stream (Unix socket or TCP).
///
/// Compile methods (`shader.compile.*`) are dispatched on a blocking thread
/// pool via `spawn_blocking` to prevent CPU-heavy compilation from starving
/// the async executor — a requirement for composition graph timing budgets.
pub async fn process_newline_reader_writer<R, W>(reader: R, mut writer: W)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
            Ok(req) => {
                if req.jsonrpc == "2.0" {
                    let result = dispatch_maybe_blocking(&req.method, req.params).await;
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
            Err(e) => make_response(
                serde_json::Value::Null,
                Err(IpcServiceError::transport(format!("parse error: {e}"))),
            ),
        };
        if writer.write_all(resp.as_bytes()).await.is_err()
            || writer.write_all(b"\n").await.is_err()
        {
            break;
        }
        let _ = writer.flush().await;
    }
}

/// Dispatch a JSON-RPC method, offloading CPU-heavy compile work to the blocking pool.
async fn dispatch_maybe_blocking(
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, IpcServiceError> {
    if method.starts_with("shader.compile.") {
        let method = method.to_owned();
        tokio::task::spawn_blocking(move || dispatch_jsonrpc(&method, params))
            .await
            .map_err(|e| IpcServiceError::internal(format!("compile task panicked: {e}")))?
    } else {
        dispatch_jsonrpc(method, params)
    }
}

/// Start a raw newline-delimited JSON-RPC server on a TCP socket.
///
/// This is the wateringHole v3.1 mandatory wire framing for inter-primal
/// composition. Springs and orchestrators connect to this endpoint.
///
/// # Errors
///
/// Returns an error if the bind address is invalid or the listener cannot be created.
pub async fn start_newline_tcp_jsonrpc(
    bind: &str,
    mut shutdown_rx: watch::Receiver<()>,
) -> Result<(SocketAddr, JoinHandle<()>), CoralReefError> {
    let addr: SocketAddr = bind.parse().map_err(IpcError::InvalidAddress)?;
    let listener = TcpListener::bind(addr).await.map_err(IpcError::JsonRpc)?;
    let bound = listener.local_addr().map_err(IpcError::JsonRpc)?;

    tracing::info!(%bound, "newline-delimited JSON-RPC (TCP) listening");

    let handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                accept = listener.accept() => {
                    match accept {
                        Ok((stream, _peer)) => {
                            let first_byte = {
                                let mut buf = [0u8; 1];
                                match tokio::time::timeout(
                                    std::time::Duration::from_secs(5),
                                    stream.peek(&mut buf),
                                )
                                .await
                                {
                                    Ok(Ok(n)) if n > 0 => Some(buf[0]),
                                    _ => None,
                                }
                            };
                            let outcome = btsp::guard_from_first_byte(first_byte).await;
                            if !outcome.should_accept() {
                                tracing::warn!(?outcome, "BTSP rejected TCP connection");
                                drop(stream);
                                continue;
                            }
                            tokio::spawn(async move {
                                let (reader, writer) = stream.into_split();
                                process_newline_reader_writer(reader, writer).await;
                            });
                        }
                        Err(e) => {
                            tracing::warn!("TCP newline JSON-RPC accept error: {e}");
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    break;
                }
            }
        }
    });

    Ok((bound, handle))
}
