// SPDX-License-Identifier: AGPL-3.0-or-later
//! Direct `dispatch()` and `make_response` unit tests for Unix JSON-RPC.
//!
//! Exercises method routing, parameter validation, error paths, health endpoints, and
//! response serialization without a live socket. Includes targeted socket integration
//! cases (unicode WGSL, invalid `jsonrpc` version) and `unix_socket_path` helpers.

#[cfg(unix)]
use super::*;
#[cfg(unix)]
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(unix)]
use tokio::net::UnixStream;

#[cfg(unix)]
async fn unix_jsonrpc_send_request(sock_path: &std::path::Path, request: &str) -> String {
    let stream = UnixStream::connect(sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    writer
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    writer.shutdown().await.unwrap();

    let mut lines = BufReader::new(reader).lines();
    lines.next_line().await.unwrap().unwrap_or_default()
}

#[cfg(unix)]
#[test]
fn dispatch_status_returns_health() {
    let result = super::newline_jsonrpc::dispatch("shader.compile.status", serde_json::json!({}));
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(val.is_object());
    assert_eq!(val["status"], "operational");
}

#[cfg(unix)]
#[test]
fn dispatch_capabilities_returns_archs() {
    let result =
        super::newline_jsonrpc::dispatch("shader.compile.capabilities", serde_json::json!({}));
    assert!(result.is_ok());
    let val = result.unwrap();
    let obj = val.as_object().expect("capabilities returns object");
    let archs = obj["targets"]
        .as_array()
        .expect("targets is array (Gate 1 wire contract)");
    assert!(!archs.is_empty());
    let f64_caps = obj["f64_transcendentals"]
        .as_object()
        .expect("f64_transcendentals is object");
    assert_eq!(f64_caps["composite_lowering"], true);
}

#[cfg(unix)]
#[test]
fn dispatch_unknown_method_returns_error() {
    let result = super::newline_jsonrpc::dispatch("nonexistent.method", serde_json::json!({}));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("method not found"));
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_with_object_params() {
    let params = serde_json::json!({
        "wgsl_source": "@compute @workgroup_size(64) fn main() {}",
        "arch": "sm70"
    });
    let result = super::newline_jsonrpc::dispatch("shader.compile.wgsl", params);
    assert!(result.is_ok());
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_with_array_params() {
    let params = serde_json::json!([{
        "wgsl_source": "@compute @workgroup_size(64) fn main() {}",
        "arch": "sm70"
    }]);
    let result = super::newline_jsonrpc::dispatch("shader.compile.wgsl", params);
    assert!(result.is_ok());
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_with_invalid_params_type() {
    let result =
        super::newline_jsonrpc::dispatch("shader.compile.wgsl", serde_json::json!("string"));
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("must be array or object")
    );
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_with_empty_array() {
    let result = super::newline_jsonrpc::dispatch("shader.compile.wgsl", serde_json::json!([]));
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("missing request parameter")
    );
}

#[cfg(unix)]
#[test]
fn dispatch_spirv_with_invalid_params_type() {
    let result = super::newline_jsonrpc::dispatch("shader.compile.spirv", serde_json::json!(42));
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn dispatch_spirv_with_empty_array() {
    let result = super::newline_jsonrpc::dispatch("shader.compile.spirv", serde_json::json!([]));
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_multi_with_object_params() {
    let params = serde_json::json!({
        "wgsl_source": "@compute @workgroup_size(64) fn main() {}",
        "targets": [{"arch": "sm_70"}]
    });
    let result = super::newline_jsonrpc::dispatch("shader.compile.wgsl.multi", params);
    assert!(result.is_ok());
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_multi_with_invalid_params_type() {
    let result =
        super::newline_jsonrpc::dispatch("shader.compile.wgsl.multi", serde_json::json!(true));
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_multi_with_empty_array() {
    let result =
        super::newline_jsonrpc::dispatch("shader.compile.wgsl.multi", serde_json::json!([]));
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn make_response_success_format() {
    let resp =
        super::unix_jsonrpc::make_response(serde_json::json!(1), Ok(serde_json::json!("ok")));
    let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["id"], 1);
    assert_eq!(parsed["result"], "ok");
    assert!(parsed.get("error").is_none() || parsed["error"].is_null());
}

#[cfg(unix)]
#[test]
fn make_response_error_format() {
    use super::error::IpcServiceError;
    let resp = super::unix_jsonrpc::make_response(
        serde_json::json!(2),
        Err(IpcServiceError::handler("something went wrong")),
    );
    let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["id"], 2);
    assert_eq!(parsed["error"]["code"], -32000);
    assert!(
        parsed["error"]["message"]
            .as_str()
            .unwrap()
            .contains("something went wrong")
    );
}

#[cfg(unix)]
#[test]
fn make_response_null_id() {
    let resp =
        super::unix_jsonrpc::make_response(serde_json::Value::Null, Ok(serde_json::json!(42)));
    let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(parsed["id"].is_null());
    assert_eq!(parsed["result"], 42);
}

// --- Additional unix_jsonrpc error path coverage ---

#[cfg(unix)]
#[test]
fn dispatch_extract_params_invalid_object_structure() {
    let params = serde_json::json!({
        "wrong_field": "value",
        "arch": "sm_70"
    });
    let result = super::newline_jsonrpc::dispatch("shader.compile.wgsl", params);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        msg.contains("invalid") || msg.contains("params") || msg.contains("wgsl"),
        "invalid params structure should produce error: {msg}"
    );
}

#[cfg(unix)]
#[test]
fn dispatch_extract_params_array_with_invalid_inner() {
    let params = serde_json::json!([{ "not_wgsl_source": "x" }]);
    let result = super::newline_jsonrpc::dispatch("shader.compile.wgsl", params);
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn dispatch_extract_params_object_invalid_spirv_type() {
    let params = serde_json::json!({
        "spirv_words": "not an array",
        "arch": "sm_70",
        "opt_level": 2,
        "fp64_software": true
    });
    let result = super::newline_jsonrpc::dispatch("shader.compile.spirv", params);
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn make_response_transport_error_code() {
    use super::error::IpcServiceError;
    let resp = super::unix_jsonrpc::make_response(
        serde_json::json!(1),
        Err(IpcServiceError::transport("connection refused")),
    );
    let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(parsed["error"]["code"], -32000);
    assert!(
        parsed["error"]["message"]
            .as_str()
            .unwrap()
            .contains("connection refused")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_unicode_in_request() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("unicode-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "shader.compile.wgsl",
        "params": {
            "wgsl_source": "// 日本語 comment\n@compute @workgroup_size(1) fn main() {}",
            "arch": "sm_70",
            "opt_level": 2,
            "fp64_software": true
        },
        "id": 1
    });
    let resp_line = unix_jsonrpc_send_request(&sock_path, &req.to_string()).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert!(resp["result"].is_object() || resp["error"].is_object());

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

// --- unix_jsonrpc edge cases for 95%+ coverage ---

#[cfg(unix)]
#[test]
fn dispatch_params_must_be_array_or_object_number() {
    let result = super::newline_jsonrpc::dispatch("shader.compile.wgsl", serde_json::json!(42));
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        msg.contains("array") || msg.contains("object") || msg.contains("params"),
        "number params should produce 'params must be array or object': {msg}"
    );
}

#[cfg(unix)]
#[test]
fn dispatch_params_must_be_array_or_object_bool() {
    let result = super::newline_jsonrpc::dispatch("shader.compile.wgsl", serde_json::json!(true));
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        msg.contains("array") || msg.contains("object") || msg.contains("params"),
        "bool params should produce error: {msg}"
    );
}

#[cfg(unix)]
#[test]
fn unix_socket_path_for_base_with_none() {
    let path = super::unix_jsonrpc::unix_socket_path_for_base(None);
    assert!(
        path.to_string_lossy()
            .ends_with(&crate::config::primal_socket_name()),
        "path should end with primal socket name: {}",
        path.display()
    );
    assert!(
        path.to_string_lossy()
            .contains(crate::config::ECOSYSTEM_NAMESPACE)
    );
}

#[cfg(unix)]
#[test]
fn unix_socket_path_for_base_with_some() {
    let base = std::env::temp_dir().join("coralreef-test-socket-base");
    let path = super::unix_jsonrpc::unix_socket_path_for_base(Some(base.clone()));
    assert!(path.starts_with(&base));
    assert!(
        path.file_name()
            .is_some_and(|f| f.to_string_lossy().ends_with(".sock")),
        "path filename should end with .sock: {}",
        path.display()
    );
}

#[cfg(unix)]
#[test]
fn default_unix_socket_path_format() {
    let path = super::unix_jsonrpc::default_unix_socket_path();
    assert!(
        path.to_string_lossy()
            .ends_with(&crate::config::primal_socket_name()),
        "path should end with primal socket name: {}",
        path.display()
    );
    assert!(
        path.to_string_lossy()
            .contains(crate::config::ECOSYSTEM_NAMESPACE)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_invalid_jsonrpc_version_string() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("bad-jsonrpc-ver-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .expect("start unix jsonrpc server");

    let req = r#"{"jsonrpc":"1.0","method":"shader.compile.status","params":{},"id":77}"#;
    let resp_line = unix_jsonrpc_send_request(&sock_path, req).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).expect("parse response json");

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 77);
    assert!(resp["error"].is_object());
    let msg = resp["error"]["message"]
        .as_str()
        .expect("error message string")
        .to_lowercase();
    assert!(
        msg.contains("jsonrpc") || msg.contains("version"),
        "expected invalid jsonrpc version error: {msg}"
    );
    assert_eq!(resp["error"]["code"], -32601);

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[test]
fn dispatch_health_check_liveness_readiness() {
    let check = super::newline_jsonrpc::dispatch("health.check", serde_json::json!({}));
    assert!(check.is_ok());
    let v = check.expect("health.check");
    assert_eq!(v["healthy"], true);

    let live = super::newline_jsonrpc::dispatch("health.liveness", serde_json::json!({}));
    assert!(live.is_ok());
    assert_eq!(live.expect("liveness")["status"], "alive");

    let ready = super::newline_jsonrpc::dispatch("health.readiness", serde_json::json!({}));
    assert!(ready.is_ok());
    assert_eq!(ready.expect("readiness")["ready"], true);
}

#[cfg(unix)]
#[test]
fn make_response_internal_error_jsonrpc_code() {
    use super::error::IpcServiceError;
    let resp = super::unix_jsonrpc::make_response(
        serde_json::json!(3),
        Err(IpcServiceError::internal("serialization bug")),
    );
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("parse make_response json");
    assert_eq!(parsed["error"]["code"], -32603);
    assert!(
        parsed["error"]["message"]
            .as_str()
            .expect("internal error message")
            .contains("serialization bug")
    );
}

#[cfg(unix)]
#[test]
fn dispatch_handler_error_returns_handler_phase() {
    let params = serde_json::json!({
        "wgsl_source": "invalid wgsl {{",
        "arch": "sm_70",
        "opt_level": 2,
        "fp64_software": true
    });
    let result = super::newline_jsonrpc::dispatch("shader.compile.wgsl", params);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        !err.to_string().is_empty(),
        "handler error should have message"
    );
}
