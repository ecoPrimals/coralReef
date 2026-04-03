// SPDX-License-Identifier: AGPL-3.0-only
//! Newline-delimited JSON-RPC 2.0 — shared dispatch and wire handling.
//!
//! Used by Unix socket and TCP listeners per wateringHole `PRIMAL_IPC_PROTOCOL` v3.1.

use std::net::SocketAddr;

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

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
        "capabilities.list" => {
            let desc = crate::capability::self_description();
            let caps: Vec<&str> = desc.provides.iter().map(|c| c.id.as_ref()).collect();
            serde_json::to_value(caps).map_err(|e| IpcServiceError::internal(e.to_string()))
        }
        "shader.compile.cpu" => {
            let req: coral_reef_cpu::CompileCpuRequest = extract_params(params)?;
            match service::handle_compile_cpu(&req) {
                Ok(resp) => {
                    serde_json::to_value(resp).map_err(|e| IpcServiceError::internal(e.to_string()))
                }
                Err(e) => Err(IpcServiceError::handler(e.to_string())),
            }
        }
        "shader.execute.cpu" => {
            let req: coral_reef_cpu::ExecuteCpuRequest = extract_params(params)?;
            match service::handle_execute_cpu(&req) {
                Ok(resp) => {
                    serde_json::to_value(resp).map_err(|e| IpcServiceError::internal(e.to_string()))
                }
                Err(e) => Err(IpcServiceError::handler(e.to_string())),
            }
        }
        "shader.validate" => {
            let req: coral_reef_cpu::ValidateRequest = extract_params(params)?;
            match service::handle_validate(&req) {
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

const JSONRPC_INTERNAL_ERROR: &[u8] =
    br#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"internal error"},"id":null}"#;

fn jsonrpc_response_struct(
    id: serde_json::Value,
    result: Result<serde_json::Value, IpcServiceError>,
) -> JsonRpcResponse {
    match result {
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
    }
}

/// Serialize a JSON-RPC 2.0 response from a handler result into `buf`, reusing capacity.
fn serialize_jsonrpc_response_into(
    id: serde_json::Value,
    result: Result<serde_json::Value, IpcServiceError>,
    buf: &mut Vec<u8>,
) {
    buf.clear();
    let resp = jsonrpc_response_struct(id, result);
    if serde_json::to_writer(&mut *buf, &resp).is_err() {
        buf.clear();
        buf.extend_from_slice(JSONRPC_INTERNAL_ERROR);
    }
}

/// Serialize a JSON-RPC 2.0 response from a handler result.
#[allow(
    dead_code,
    reason = "called via `unix_jsonrpc` re-export and integration tests"
)]
pub fn make_response(
    id: serde_json::Value,
    result: Result<serde_json::Value, IpcServiceError>,
) -> String {
    let mut buf = Vec::with_capacity(256);
    serialize_jsonrpc_response_into(id, result, &mut buf);
    String::from_utf8(buf)
        .unwrap_or_else(|_| String::from_utf8_lossy(JSONRPC_INTERNAL_ERROR).into_owned())
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
    reason = "used in integration tests — dead in binary when e2e feature active"
)]
pub fn dispatch(
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, IpcServiceError> {
    dispatch_jsonrpc(method, params)
}

/// Read/write JSON-RPC lines on a stream (Unix socket or TCP).
pub async fn process_newline_reader_writer<R, W>(reader: R, mut writer: W)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut line_buf = String::new();
    let mut out_buf = Vec::with_capacity(4096);
    loop {
        line_buf.clear();
        match reader.read_line(&mut line_buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let line = line_buf.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<JsonRpcRequest>(line) {
            Ok(req) => {
                if req.jsonrpc == "2.0" {
                    let result = dispatch_jsonrpc(&req.method, req.params);
                    serialize_jsonrpc_response_into(req.id, result, &mut out_buf);
                } else {
                    serialize_jsonrpc_response_into(
                        req.id,
                        Err(IpcServiceError::dispatch(format!(
                            "invalid jsonrpc version: {}",
                            req.jsonrpc
                        ))),
                        &mut out_buf,
                    );
                }
            }
            Err(e) => serialize_jsonrpc_response_into(
                serde_json::Value::Null,
                Err(IpcServiceError::transport(format!("parse error: {e}"))),
                &mut out_buf,
            ),
        }
        out_buf.push(b'\n');
        let wire: Bytes = Bytes::from(std::mem::take(&mut out_buf));
        if writer.write_all(&wire).await.is_err() {
            break;
        }
        out_buf = Vec::from(wire);
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
