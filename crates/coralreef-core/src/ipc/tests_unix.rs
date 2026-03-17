// SPDX-License-Identifier: AGPL-3.0-only
//! Unix JSON-RPC (newline-delimited over Unix socket) tests.

#[cfg(unix)]
use super::*;

// --- Unit tests for dispatch and make_response ---

#[cfg(unix)]
#[test]
fn test_dispatch_valid_method_status() {
    let result = dispatch("shader.compile.status", serde_json::json!({}));
    let val = result.expect("status should succeed");
    assert!(val.get("name").and_then(|v| v.as_str()).is_some());
    assert!(
        val.get("supported_archs")
            .and_then(|v| v.as_array())
            .is_some()
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_health_check() {
    let result = dispatch("health.check", serde_json::json!({}));
    let val = result.expect("health.check should succeed");
    assert_eq!(
        val.get("healthy").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert!(
        val.get("name")
            .and_then(serde_json::Value::as_str)
            .is_some()
    );
    assert!(
        val.get("version")
            .and_then(serde_json::Value::as_str)
            .is_some()
    );
    assert!(
        val.get("family_id")
            .and_then(serde_json::Value::as_str)
            .is_some()
    );
    assert!(
        val.get("supported_archs")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|a| !a.is_empty())
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_health_liveness() {
    let result = dispatch("health.liveness", serde_json::json!({}));
    let val = result.expect("health.liveness should succeed");
    assert_eq!(
        val.get("alive").and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_health_readiness() {
    let result = dispatch("health.readiness", serde_json::json!({}));
    let val = result.expect("health.readiness should succeed");
    assert_eq!(
        val.get("ready").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert!(
        val.get("name")
            .and_then(serde_json::Value::as_str)
            .is_some()
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_valid_method_capabilities() {
    let result = dispatch("shader.compile.capabilities", serde_json::json!({}));
    let val = result.expect("capabilities should succeed");
    let arr = val.as_array().expect("capabilities returns array");
    assert!(!arr.is_empty());
}

#[cfg(unix)]
#[test]
fn test_dispatch_valid_method_wgsl() {
    let params = serde_json::json!({
        "wgsl_source": "@compute @workgroup_size(1) fn main() {}",
        "arch": "sm_70",
        "opt_level": 2,
        "fp64_software": true
    });
    let result = dispatch("shader.compile.wgsl", params);
    let val = result.expect("wgsl compile should succeed");
    assert!(
        val.get("size")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_unknown_method() {
    let result = dispatch("nonexistent.method", serde_json::json!({}));
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = err.to_string().to_lowercase();
    assert!(msg.contains("not found"));
    assert!(msg.contains("nonexistent.method"));
}

#[cfg(unix)]
#[test]
fn test_dispatch_empty_params_array() {
    let result = dispatch("shader.compile.wgsl", serde_json::json!([]));
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .to_lowercase()
            .contains("missing")
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_null_params() {
    let result = dispatch("shader.compile.wgsl", serde_json::Value::Null);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .to_lowercase()
            .contains("must be array or object")
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_invalid_params_type() {
    let result = dispatch("shader.compile.wgsl", serde_json::json!("invalid"));
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .to_lowercase()
            .contains("must be array or object")
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_wgsl_array_params() {
    let params = serde_json::json!([{
        "wgsl_source": "@compute @workgroup_size(1) fn main() {}",
        "arch": "sm_70",
        "opt_level": 2,
        "fp64_software": true
    }]);
    let result = dispatch("shader.compile.wgsl", params);
    let val = result.expect("wgsl compile with array params should succeed");
    assert!(
        val.get("size")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0
    );
}

#[cfg(unix)]
#[test]
fn test_make_response_success() {
    let id = serde_json::json!(42);
    let result = Ok(serde_json::json!({"foo": "bar"}));
    let resp = make_response(id.clone(), result);
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["id"], 42);
    assert_eq!(parsed["result"]["foo"], "bar");
    assert!(parsed.get("error").is_none());
}

#[cfg(unix)]
#[test]
fn test_make_response_error() {
    use super::error::IpcServiceError;
    let id = serde_json::json!("req-1");
    let result = Err(IpcServiceError::handler("something went wrong"));
    let resp = make_response(id.clone(), result);
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["id"], "req-1");
    assert_eq!(parsed["error"]["code"], -32000);
    assert!(
        parsed["error"]["message"]
            .as_str()
            .unwrap()
            .contains("something went wrong")
    );
    assert!(parsed.get("result").is_none());
}

#[cfg(unix)]
#[test]
fn test_make_response_null_id() {
    let result = Ok(serde_json::json!(true));
    let resp = make_response(serde_json::Value::Null, result);
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert!(parsed["id"].is_null());
    assert_eq!(parsed["result"], true);
}

#[cfg(unix)]
#[test]
fn test_dispatch_spirv_empty_array_params() {
    let result = dispatch("shader.compile.spirv", serde_json::json!([]));
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .to_lowercase()
            .contains("missing")
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_wgsl_multi_empty_array_params() {
    let result = dispatch("shader.compile.wgsl.multi", serde_json::json!([]));
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .to_lowercase()
            .contains("missing")
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_spirv_object_params() {
    let spirv = test_helpers::valid_spirv_minimal_compute();
    let params = serde_json::json!({
        "spirv_words": spirv,
        "arch": "sm_70",
        "opt_level": 2,
        "fp64_software": true
    });
    let result = dispatch("shader.compile.spirv", params);
    match &result {
        Ok(val) => assert!(
            val.get("size")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
        ),
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            assert!(msg.contains("implemented") || msg.contains("not"));
        }
    }
}

#[cfg(unix)]
#[test]
fn test_dispatch_wgsl_multi_array_params() {
    let params = serde_json::json!([{
        "wgsl_source": "@compute @workgroup_size(1) fn main() {}",
        "targets": [{ "card_index": 0, "arch": "sm_70" }],
        "opt_level": 2
    }]);
    let result = dispatch("shader.compile.wgsl.multi", params);
    let val = result.expect("wgsl.multi with array params should succeed");
    assert!(
        val.get("success_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            >= 1
    );
    assert!(
        !val.get("results")
            .and_then(serde_json::Value::as_array)
            .unwrap()
            .is_empty()
    );
}
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
fn test_unix_socket_path_with_xdg() {
    let path = unix_socket_path_for_base(Some("/run/user/1234".into()));
    assert!(path.to_string_lossy().contains("/run/user/1234"));
    assert!(
        path.to_string_lossy()
            .contains(crate::config::ECOSYSTEM_NAMESPACE)
    );
    assert!(
        path.to_string_lossy().contains("coralreef") && path.to_string_lossy().contains(".sock"),
        "path should contain primal name and .sock: {}",
        path.to_string_lossy()
    );
}

#[cfg(unix)]
#[test]
fn test_unix_socket_path_fallback() {
    let path = unix_socket_path_for_base(None);
    assert!(
        path.to_string_lossy()
            .contains(crate::config::ECOSYSTEM_NAMESPACE)
    );
    assert!(
        path.to_string_lossy().contains("coralreef") && path.to_string_lossy().contains(".sock"),
        "path should contain primal name and .sock: {}",
        path.to_string_lossy()
    );
}

#[cfg(unix)]
#[test]
fn test_default_unix_socket_path_with_xdg() {
    let temp = tempfile::tempdir().unwrap();
    let xdg = temp.path().to_path_buf();
    // Test unix_socket_path_for_base directly to avoid unsafe env mutation
    let path = unix_socket_path_for_base(Some(xdg.clone()));
    assert!(
        path.to_string_lossy()
            .contains(xdg.to_string_lossy().as_ref()),
        "path should contain XDG_RUNTIME_DIR"
    );
    assert!(
        path.to_string_lossy().contains("coralreef") && path.to_string_lossy().contains(".sock"),
        "path should contain primal name and .sock: {}",
        path.to_string_lossy()
    );
}

#[cfg(unix)]
#[test]
fn test_default_unix_socket_path_structure() {
    let path = default_unix_socket_path();
    assert!(
        path.to_string_lossy()
            .contains(crate::config::ECOSYSTEM_NAMESPACE)
    );
    assert!(
        path.to_string_lossy()
            .ends_with(&crate::config::primal_socket_name()),
        "path should end with primal socket name: {}",
        path.to_string_lossy()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_health() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("jsonrpc-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();

    let req = r#"{"jsonrpc":"2.0","method":"shader.compile.status","params":{},"id":1}"#;
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .unwrap();

    let mut lines = BufReader::new(reader).lines();
    let response_line = lines.next_line().await.unwrap().unwrap();
    let resp: serde_json::Value = serde_json::from_str(&response_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert!(resp["result"]["name"].is_string());
    assert!(resp["result"]["supported_archs"].is_array());

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_compile_wgsl() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("jsonrpc-wgsl-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "shader.compile.wgsl",
        "params": {
            "wgsl_source": "@compute @workgroup_size(1) fn main() {}",
            "arch": "sm_70",
            "opt_level": 2,
            "fp64_software": true
        },
        "id": 2
    });
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .unwrap();

    let mut lines = BufReader::new(reader).lines();
    let response_line = lines.next_line().await.unwrap().unwrap();
    let resp: serde_json::Value = serde_json::from_str(&response_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 2);
    assert!(resp["result"].is_object(), "compile should succeed");
    assert!(resp["result"]["size"].as_u64().unwrap() > 0);

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_parse_error() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("parse-err-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let resp_line = unix_jsonrpc_send_request(&sock_path, "not valid json").await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert!(resp["error"].is_object());
    assert_eq!(resp["error"]["code"], -32000);
    assert!(
        resp["error"]["message"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("parse")
    );

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_invalid_version() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("version-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = r#"{"jsonrpc":"1.0","method":"shader.compile.status","params":{},"id":1}"#;
    let resp_line = unix_jsonrpc_send_request(&sock_path, req).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert!(resp["error"].is_object());
    assert!(
        resp["error"]["message"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("version")
    );

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_method_not_found() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("method-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = r#"{"jsonrpc":"2.0","method":"nonexistent.method","params":{},"id":42}"#;
    let resp_line = unix_jsonrpc_send_request(&sock_path, req).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 42);
    assert!(resp["error"].is_object());
    assert!(
        resp["error"]["message"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("not found")
    );

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_capabilities() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("caps-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = r#"{"jsonrpc":"2.0","method":"shader.compile.capabilities","params":{},"id":3}"#;
    let resp_line = unix_jsonrpc_send_request(&sock_path, req).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 3);
    assert!(resp["result"].is_array());
    let archs = resp["result"].as_array().unwrap();
    assert!(!archs.is_empty());
    assert!(archs.iter().any(|a| a.as_str() == Some("sm_70")));

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_health_check() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("health-check-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = r#"{"jsonrpc":"2.0","method":"health.check","params":{},"id":10}"#;
    let resp_line = unix_jsonrpc_send_request(&sock_path, req).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 10);
    let result = &resp["result"];
    assert_eq!(result["healthy"], true);
    assert!(result["name"].is_string());
    assert!(result["version"].is_string());
    assert!(result["family_id"].is_string());
    assert!(result["supported_archs"].is_array());
    assert!(!result["supported_archs"].as_array().unwrap().is_empty());

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_health_liveness() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("health-liveness-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = r#"{"jsonrpc":"2.0","method":"health.liveness","params":{},"id":11}"#;
    let resp_line = unix_jsonrpc_send_request(&sock_path, req).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 11);
    assert_eq!(resp["result"]["alive"], true);

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_health_readiness() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("health-readiness-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = r#"{"jsonrpc":"2.0","method":"health.readiness","params":{},"id":12}"#;
    let resp_line = unix_jsonrpc_send_request(&sock_path, req).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 12);
    assert_eq!(resp["result"]["ready"], true);
    assert!(resp["result"]["name"].is_string());

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}
