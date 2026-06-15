// SPDX-License-Identifier: AGPL-3.0-or-later
//! Newline-delimited JSON-RPC 2.0 — shared dispatch and wire handling.
//!
//! Used by Unix socket and TCP listeners per wateringHole `PRIMAL_IPC_PROTOCOL` v3.1.

use std::net::SocketAddr;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::btsp;
use super::error::IpcServiceError;
use super::method_gate;
use super::{CoralReefError, IpcError};
use crate::env_keys;
use crate::service;

#[derive(Deserialize)]
pub(super) struct JsonRpcRequest {
    pub(super) jsonrpc: String,
    pub(super) method: String,
    #[serde(default)]
    pub(super) params: serde_json::Value,
    pub(super) id: serde_json::Value,
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
/// The method gate (JH-0) runs pre-dispatch: public methods pass through,
/// protected methods are checked against the caller context. In permissive
/// mode (default), protected methods are logged but allowed.
///
/// # Errors
///
/// Returns `IpcServiceError` if the method is unknown, params are
/// invalid, the handler itself fails, or the gate rejects the call.
#[must_use = "returns the handler result or an error — check the result"]
pub fn dispatch_jsonrpc(
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, IpcServiceError> {
    let caller = method_gate::CallerContext::from_params(&params);
    if let Err(denied) = method_gate::gate().check(method, &caller) {
        return Err(IpcServiceError::gate_denied(denied.message));
    }

    match method {
        "auth.check" => {
            let resp = serde_json::json!({
                "authenticated": caller.bearer_token.is_some(),
                "origin": format!("{:?}", caller.origin).to_lowercase(),
            });
            Ok(resp)
        }
        "auth.mode" => {
            let gate = method_gate::gate();
            let resp = serde_json::json!({
                "mode": gate.mode().as_str(),
            });
            Ok(resp)
        }
        "auth.peer_info" => {
            let peer_info = caller
                .peer
                .as_ref()
                .map(|p| serde_json::json!({ "uid": p.uid, "pid": p.pid }));
            let resp = serde_json::json!({
                "peer": peer_info,
                "origin": format!("{:?}", caller.origin).to_lowercase(),
                "has_token": caller.bearer_token.is_some(),
            });
            Ok(resp)
        }
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
                Ok(resp) => serde_json::to_value(resp.with_provenance())
                    .map_err(|e| IpcServiceError::internal(e.to_string())),
                Err(e) => Err(IpcServiceError::from(e)),
            }
        }
        "shader.compile.spirv" => {
            let req: service::CompileRequest = extract_params(params)?;
            match service::handle_compile(&req) {
                Ok(resp) => serde_json::to_value(resp.with_provenance())
                    .map_err(|e| IpcServiceError::internal(e.to_string())),
                Err(e) => Err(IpcServiceError::from(e)),
            }
        }
        "shader.compile.wgsl.multi" => {
            let req: service::MultiDeviceCompileRequest = extract_params(params)?;
            match service::handle_compile_wgsl_multi(req) {
                Ok(resp) => {
                    serde_json::to_value(resp).map_err(|e| IpcServiceError::internal(e.to_string()))
                }
                Err(e) => Err(IpcServiceError::from(e)),
            }
        }
        "shader.compile.gemm" => {
            let req: service::GemmCompileRequest = extract_params(params)?;
            match service::handle_compile_gemm(&req) {
                Ok(resp) => serde_json::to_value(resp.with_provenance())
                    .map_err(|e| IpcServiceError::internal(e.to_string())),
                Err(e) => Err(IpcServiceError::from(e)),
            }
        }
        "health" => Ok(service::handle_health_standard()),
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
        "health.version" => {
            let resp = service::handle_health_version();
            serde_json::to_value(resp).map_err(|e| IpcServiceError::internal(e.to_string()))
        }
        "identity.get" => {
            let resp = service::handle_identity_get();
            serde_json::to_value(resp).map_err(|e| IpcServiceError::internal(e.to_string()))
        }
        "capability.list" | "capabilities.list" => {
            let resp = service::handle_capability_list();
            serde_json::to_value(resp).map_err(|e| IpcServiceError::internal(e.to_string()))
        }
        "btsp.negotiate" => {
            let req: super::btsp_negotiate::NegotiateRequest = extract_params(params)?;
            match super::btsp_negotiate::handle_negotiate(&req) {
                Ok(resp) => {
                    serde_json::to_value(resp).map_err(|e| IpcServiceError::internal(e.to_string()))
                }
                Err(e) => Err(IpcServiceError::handler(e.to_string())),
            }
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

/// Default compile timeout (seconds). Override with `CORALREEF_COMPILE_TIMEOUT_SECS`.
const DEFAULT_COMPILE_TIMEOUT_SECS: u64 = 120;

/// Timeout for first-byte protocol detection on new TCP connections.
const TCP_PEEK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// riboCipher signal prefix: `[0xEC, 0x01]` — ecosystem health signal.
const RIBOCIPHER_PREFIX: &[u8] = &[0xEC, 0x01];

pub(super) fn compile_timeout() -> std::time::Duration {
    let secs = std::env::var(env_keys::CORALREEF_COMPILE_TIMEOUT_SECS)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_COMPILE_TIMEOUT_SECS);
    std::time::Duration::from_secs(secs)
}

/// Dispatch a JSON-RPC method, offloading CPU-heavy compile work to the blocking pool.
///
/// Compile methods are wrapped in a deadline (`CORALREEF_COMPILE_TIMEOUT_SECS`,
/// default 120s) to prevent unbounded blocking from stalling the IPC server.
pub(super) async fn dispatch_maybe_blocking(
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, IpcServiceError> {
    if method.starts_with("shader.compile.") {
        let method = method.to_owned();
        let deadline = compile_timeout();
        let task = tokio::task::spawn_blocking(move || dispatch_jsonrpc(&method, params));
        match tokio::time::timeout(deadline, task).await {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => Err(IpcServiceError::internal(format!(
                "compile task panicked: {e}"
            ))),
            Err(_elapsed) => Err(IpcServiceError::internal(format!(
                "shader compilation exceeded {deadline:?} deadline"
            ))),
        }
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
                                    TCP_PEEK_TIMEOUT,
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
                            let consume_marker = first_byte.is_some_and(|b| b != b'{');
                            let is_ribocipher = first_byte == Some(RIBOCIPHER_PREFIX[0]);
                            tokio::spawn(async move {
                                let (reader, writer) = stream.into_split();
                                if consume_marker {
                                    let mut br = tokio::io::BufReader::new(reader);
                                    let _ = br.read_u8().await;
                                    if is_ribocipher {
                                        let _ = br.read_u8().await;
                                    }
                                    process_newline_reader_writer(br, writer).await;
                                } else {
                                    process_newline_reader_writer(reader, writer).await;
                                }
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
