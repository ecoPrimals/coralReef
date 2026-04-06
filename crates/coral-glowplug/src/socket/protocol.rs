// SPDX-License-Identifier: AGPL-3.0-or-later
//! JSON-RPC 2.0 wire types and response serialization.

use serde::{Deserialize, Serialize};

/// Maximum line length for a single JSON-RPC request (4 MiB).
/// Sized for compute dispatch payloads (base64-encoded shader + buffers)
/// while still bounding memory from unbounded input.
pub(super) const MAX_REQUEST_LINE_BYTES: usize = 4 * 1024 * 1024;

/// Initial per-connection read buffer (64 KiB).
/// Tokio's `BufReader` grows on demand up to `MAX_REQUEST_LINE_BYTES` via
/// `lines()`, so idle connections only use this smaller allocation.
pub(super) const INITIAL_BUF_CAPACITY: usize = 64 * 1024;

/// Per-client request timeout (30 seconds idle = disconnect).
pub(super) const CLIENT_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Deserialize)]
pub(crate) struct JsonRpcRequest {
    pub(crate) jsonrpc: String,
    pub(crate) method: String,
    #[serde(default)]
    pub(crate) params: serde_json::Value,
    pub(crate) id: serde_json::Value,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceInfo {
    pub bdf: String,
    pub name: Option<String>,
    pub chip: String,
    pub vendor_id: u16,
    pub device_id: u16,
    pub personality: String,
    pub role: Option<String>,
    pub power: String,
    pub vram_alive: bool,
    pub domains_alive: usize,
    pub domains_faulted: usize,
    pub has_vfio_fd: bool,
    pub pci_link_width: Option<u8>,
    /// True when the device has `role = "display"` and is immune to swaps.
    #[serde(default)]
    pub protected: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthInfo {
    pub bdf: String,
    pub boot0: u32,
    pub pmc_enable: u32,
    pub vram_alive: bool,
    pub power: String,
    pub domains_alive: usize,
    pub domains_faulted: usize,
}

pub(super) fn make_response(
    id: serde_json::Value,
    result: Result<serde_json::Value, coral_glowplug::error::RpcError>,
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
                code: e.code.into(),
                message: e.message,
            }),
            id,
        },
    };
    match serde_json::to_string(&resp) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "failed to serialize JSON-RPC response");
            r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"internal error"},"id":null}"#
                .to_owned()
        }
    }
}
