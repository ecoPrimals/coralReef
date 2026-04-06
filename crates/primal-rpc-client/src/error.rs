// SPDX-License-Identifier: AGPL-3.0-or-later
//! Error types for the JSON-RPC client.

use serde::Deserialize;

/// Errors returned by [`RpcClient`](crate::RpcClient) operations.
#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    /// I/O error during transport (connect, read, write).
    #[error("transport I/O: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization or deserialization failure.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    /// The HTTP response could not be parsed (missing headers, bad status, etc.).
    #[error("http: {0}")]
    Http(String),

    /// The server returned a JSON-RPC error object.
    #[error("server error {}: {}", .0.code, .0.message)]
    Server(RpcErrorData),

    /// The server returned neither `result` nor `error`.
    #[error("empty response: no result or error field")]
    EmptyResponse,
}

/// JSON-RPC 2.0 error object from the server.
#[derive(Debug, Clone, Deserialize)]
pub struct RpcErrorData {
    /// Numeric error code (e.g. `-32600` for invalid request).
    pub code: i64,
    /// Human-readable error description.
    pub message: String,
    /// Optional structured data attached by the server.
    pub data: Option<serde_json::Value>,
}

impl std::fmt::Display for RpcErrorData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON-RPC error {}: {}", self.code, self.message)
    }
}
