// SPDX-License-Identifier: AGPL-3.0-only
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
    #[serde(default)]
    pub fecs_cpuctl: u32,
    #[serde(default)]
    pub fecs_stopped: bool,
    /// CPUCTL bit 4 (firmware HALT). `fecs_hreset` was the legacy JSON key for this bit.
    #[serde(default, alias = "fecs_hreset")]
    pub fecs_halted: bool,
    #[serde(default)]
    pub fecs_sctl: u32,
    #[serde(default)]
    pub gpccs_cpuctl: u32,
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

#[cfg(test)]
mod tests {
    use super::*;
    use coral_glowplug::error::{RpcError, RpcErrorCode};
    use serde_json::json;

    #[test]
    fn make_response_ok_includes_result_and_id() {
        let resp = make_response(json!(42), Ok(json!({"status": "ok"})));
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 42);
        assert_eq!(v["result"]["status"], "ok");
        assert!(v.get("error").is_none());
    }

    #[test]
    fn make_response_err_includes_error_code_and_message() {
        let err = RpcError::invalid_params("missing bdf");
        let resp = make_response(json!(7), Err(err));
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 7);
        assert!(v.get("result").is_none());
        assert_eq!(v["error"]["code"], i32::from(RpcErrorCode::INVALID_PARAMS));
        assert_eq!(v["error"]["message"], "missing bdf");
    }

    #[test]
    fn make_response_with_null_id() {
        let resp = make_response(json!(null), Ok(json!(true)));
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(v["id"].is_null());
        assert_eq!(v["result"], true);
    }

    #[test]
    fn make_response_with_string_id() {
        let resp = make_response(json!("req-1"), Ok(json!([])));
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["id"], "req-1");
    }

    #[test]
    fn make_response_method_not_found() {
        let err = RpcError::method_not_found("unknown.method");
        let resp = make_response(json!(1), Err(err));
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(
            v["error"]["code"],
            i32::from(RpcErrorCode::METHOD_NOT_FOUND)
        );
        assert!(v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown.method"));
    }

    #[test]
    fn make_response_device_error() {
        let err = RpcError::device_error("GPU hung");
        let resp = make_response(json!(99), Err(err));
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], i32::from(RpcErrorCode::DEVICE_ERROR));
        assert_eq!(v["error"]["message"], "GPU hung");
    }

    #[test]
    fn jsonrpc_request_deserializes() {
        let raw = r#"{"jsonrpc":"2.0","method":"device.list","params":{},"id":1}"#;
        let req: JsonRpcRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.method, "device.list");
        assert_eq!(req.id, json!(1));
    }

    #[test]
    fn jsonrpc_request_params_default_to_null() {
        let raw = r#"{"jsonrpc":"2.0","method":"health.check","id":"abc"}"#;
        let req: JsonRpcRequest = serde_json::from_str(raw).unwrap();
        assert!(req.params.is_null());
    }

    #[test]
    fn device_info_serde_roundtrip() {
        let info = DeviceInfo {
            bdf: "0000:03:00.0".into(),
            name: Some("Tesla K80".into()),
            chip: "GK210".into(),
            vendor_id: 0x10DE,
            device_id: 0x102D,
            personality: "vfio".into(),
            role: Some("compute".into()),
            power: "D0".into(),
            vram_alive: true,
            domains_alive: 4,
            domains_faulted: 0,
            has_vfio_fd: true,
            pci_link_width: Some(16),
            protected: false,
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: DeviceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bdf, info.bdf);
        assert_eq!(back.vendor_id, 0x10DE);
        assert_eq!(back.pci_link_width, Some(16));
        assert!(!back.protected);
    }

    #[test]
    fn health_info_serde_roundtrip() {
        let info = HealthInfo {
            bdf: "0000:4a:00.0".into(),
            boot0: 0x164000A1,
            pmc_enable: 0x5FECD_FF1,
            vram_alive: true,
            power: "D0".into(),
            domains_alive: 6,
            domains_faulted: 1,
            fecs_cpuctl: 0x40,
            fecs_stopped: false,
            fecs_halted: false,
            fecs_sctl: 0x10,
            gpccs_cpuctl: 0x02,
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: HealthInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.boot0, 0x164000A1);
        assert_eq!(back.fecs_cpuctl, 0x40);
        assert!(!back.fecs_halted);
    }

    #[test]
    fn health_info_fecs_hreset_alias() {
        let json = r#"{"bdf":"0:0:0.0","boot0":0,"pmc_enable":0,"vram_alive":false,"power":"D0","domains_alive":0,"domains_faulted":0,"fecs_hreset":true}"#;
        let info: HealthInfo = serde_json::from_str(json).unwrap();
        assert!(info.fecs_halted);
    }

    #[test]
    fn constants_have_expected_values() {
        assert_eq!(MAX_REQUEST_LINE_BYTES, 4 * 1024 * 1024);
        assert_eq!(INITIAL_BUF_CAPACITY, 64 * 1024);
        assert_eq!(CLIENT_IDLE_TIMEOUT, std::time::Duration::from_secs(30));
    }
}
