// SPDX-License-Identifier: AGPL-3.0-or-later
//! JSON-RPC 2.0 request/response types and line writers.

use std::io::{ErrorKind, Write};

use serde::{Deserialize, Serialize};

/// Incoming JSON-RPC 2.0 request line (single object per connection read).
#[derive(Deserialize)]
pub struct JsonRpcRequest {
    /// Must be `"2.0"`.
    pub jsonrpc: String,
    /// Method name (e.g. `ember.list`).
    pub method: String,
    #[serde(default)]
    /// Method parameters object.
    pub params: serde_json::Value,
    /// Request correlation id.
    pub id: serde_json::Value,
}

/// JSON-RPC 2.0 response envelope.
#[derive(Serialize)]
pub struct JsonRpcResponse {
    /// Protocol version (`"2.0"`).
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Success payload.
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Error payload.
    pub error: Option<JsonRpcError>,
    /// Matches the request `id`.
    pub id: serde_json::Value,
}

/// JSON-RPC error object.
#[derive(Serialize)]
pub struct JsonRpcError {
    /// JSON-RPC error code.
    pub code: i32,
    /// Human-readable message.
    pub message: String,
}

pub(crate) fn make_jsonrpc_ok(id: serde_json::Value, result: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        result: Some(result),
        error: None,
        id,
    }
}

pub(crate) fn write_jsonrpc_ok(
    stream: &mut impl Write,
    id: serde_json::Value,
    result: serde_json::Value,
) -> std::io::Result<()> {
    let resp = make_jsonrpc_ok(id, result);
    let json =
        serde_json::to_string(&resp).map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e))?;
    stream.write_all(format!("{json}\n").as_bytes())
}

pub(crate) fn write_jsonrpc_error(
    stream: &mut impl Write,
    id: serde_json::Value,
    code: i32,
    message: &str,
) -> std::io::Result<()> {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0",
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.to_string(),
        }),
        id,
    };
    let json =
        serde_json::to_string(&resp).map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e))?;
    stream.write_all(format!("{json}\n").as_bytes())
}
