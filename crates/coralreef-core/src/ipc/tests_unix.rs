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
    assert!(err.to_lowercase().contains("not found"));
    assert!(err.contains("nonexistent.method"));
}

#[cfg(unix)]
#[test]
fn test_dispatch_empty_params_array() {
    let result = dispatch("shader.compile.wgsl", serde_json::json!([]));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_lowercase().contains("missing"));
}

#[cfg(unix)]
#[test]
fn test_dispatch_null_params() {
    let result = dispatch("shader.compile.wgsl", serde_json::Value::Null);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_lowercase().contains("invalid"));
}

#[cfg(unix)]
#[test]
fn test_dispatch_invalid_params_type() {
    let result = dispatch("shader.compile.wgsl", serde_json::json!("invalid"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_lowercase().contains("invalid"));
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
    let id = serde_json::json!("req-1");
    let result = Err("something went wrong".to_owned());
    let resp = make_response(id.clone(), result);
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["id"], "req-1");
    assert_eq!(parsed["error"]["code"], -32000);
    assert_eq!(parsed["error"]["message"], "something went wrong");
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
    assert!(result.unwrap_err().to_lowercase().contains("missing"));
}

#[cfg(unix)]
#[test]
fn test_dispatch_wgsl_multi_empty_array_params() {
    let result = dispatch("shader.compile.wgsl.multi", serde_json::json!([]));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_lowercase().contains("missing"));
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
            assert!(e.to_lowercase().contains("implemented") || e.to_lowercase().contains("not"));
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
            .contains(coralreef_core::config::ECOSYSTEM_NAMESPACE)
    );
    assert!(path.to_string_lossy().contains("coralreef.sock"));
}

#[cfg(unix)]
#[test]
fn test_unix_socket_path_fallback() {
    let path = unix_socket_path_for_base(None);
    assert!(
        path.to_string_lossy()
            .contains(coralreef_core::config::ECOSYSTEM_NAMESPACE)
    );
    assert!(path.to_string_lossy().contains("coralreef.sock"));
}

#[cfg(unix)]
#[test]
fn test_default_unix_socket_path_with_xdg() {
    let temp = tempfile::tempdir().unwrap();
    let xdg = temp.path().to_path_buf();
    // SAFETY: test-only; we restore the env before the test ends
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", xdg.as_os_str());
    }
    let path = default_unix_socket_path();
    // SAFETY: test-only; restoring env after our test mutation
    unsafe {
        std::env::remove_var("XDG_RUNTIME_DIR");
    }
    assert!(
        path.to_string_lossy()
            .contains(xdg.to_string_lossy().as_ref()),
        "path should contain XDG_RUNTIME_DIR"
    );
    assert!(path.to_string_lossy().contains("coralreef.sock"));
}

#[cfg(unix)]
#[test]
fn test_default_unix_socket_path_structure() {
    let path = default_unix_socket_path();
    assert!(
        path.to_string_lossy()
            .contains(coralreef_core::config::ECOSYSTEM_NAMESPACE)
    );
    assert!(path.to_string_lossy().ends_with("coralreef.sock"));
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
async fn test_unix_jsonrpc_spirv_array_params() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("spirv-arr-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let spirv = test_helpers::valid_spirv_minimal_compute();
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "shader.compile.spirv",
        "params": [{
            "spirv_words": spirv,
            "arch": "sm_70",
            "opt_level": 2,
            "fp64_software": true
        }],
        "id": 4
    });
    let resp_line = unix_jsonrpc_send_request(&sock_path, &req.to_string()).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 4);
    match (resp.get("result"), resp.get("error")) {
        (Some(r), _) if r.is_object() => assert!(r["size"].as_u64().unwrap_or(0) > 0),
        (_, Some(e)) => assert!(e["message"].as_str().unwrap_or("").contains("implemented")),
        _ => panic!("expected result or error"),
    }

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_spirv_empty_array_params() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("spirv-empty-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = r#"{"jsonrpc":"2.0","method":"shader.compile.spirv","params":[],"id":5}"#;
    let resp_line = unix_jsonrpc_send_request(&sock_path, req).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert!(resp["error"].is_object());
    assert!(
        resp["error"]["message"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("missing")
    );

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_spirv_invalid_params() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("spirv-inv-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = r#"{"jsonrpc":"2.0","method":"shader.compile.spirv","params":"invalid","id":6}"#;
    let resp_line = unix_jsonrpc_send_request(&sock_path, req).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert!(resp["error"].is_object());
    assert!(
        resp["error"]["message"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("invalid")
    );

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_wgsl_array_params() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("wgsl-arr-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "shader.compile.wgsl",
        "params": [{
            "wgsl_source": "@compute @workgroup_size(1) fn main() {}",
            "arch": "sm_70",
            "opt_level": 2,
            "fp64_software": true
        }],
        "id": 7
    });
    let resp_line = unix_jsonrpc_send_request(&sock_path, &req.to_string()).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 7);
    assert!(resp["result"].is_object());
    assert!(resp["result"]["size"].as_u64().unwrap_or(0) > 0);

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_wgsl_empty_array_params() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("wgsl-empty-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = r#"{"jsonrpc":"2.0","method":"shader.compile.wgsl","params":[],"id":8}"#;
    let resp_line = unix_jsonrpc_send_request(&sock_path, req).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert!(resp["error"].is_object());
    assert!(
        resp["error"]["message"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("missing")
    );

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_wgsl_multi() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("wgsl-multi-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "shader.compile.wgsl.multi",
        "params": {
            "wgsl_source": "@compute @workgroup_size(1) fn main() {}",
            "targets": [
                { "card_index": 0, "arch": "sm_70" },
                { "card_index": 1, "arch": "sm_89" }
            ],
            "opt_level": 2
        },
        "id": 10
    });
    let resp_line = unix_jsonrpc_send_request(&sock_path, &req.to_string()).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 10);
    assert!(resp["result"].is_object(), "multi compile should succeed");
    assert_eq!(resp["result"]["success_count"], 2);
    assert_eq!(resp["result"]["total_count"], 2);
    let results = resp["result"]["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["arch"], "sm_70");
    assert_eq!(results[1]["arch"], "sm_89");

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_missing_method_field() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("no-method-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = r#"{"jsonrpc":"2.0","params":{},"id":1}"#;
    let resp_line = unix_jsonrpc_send_request(&sock_path, req).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert!(resp["error"].is_object());
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
async fn test_unix_jsonrpc_batch_request_rejected() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("batch-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = r#"[{"jsonrpc":"2.0","method":"shader.compile.status","params":{},"id":1}]"#;
    let resp_line = unix_jsonrpc_send_request(&sock_path, req).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert!(resp["error"].is_object());
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
async fn test_unix_jsonrpc_malformed_json_truncated() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("truncated-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let resp_line = unix_jsonrpc_send_request(&sock_path, "{").await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert!(resp["error"].is_object());
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
async fn test_start_unix_jsonrpc_server_bind_fails_when_path_is_directory() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("dir-as-sock-{}.sock", std::process::id()));
    std::fs::write(&sock_path, "").unwrap();
    std::fs::remove_file(&sock_path).unwrap();
    std::fs::create_dir(&sock_path).unwrap();

    let (_shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let result = start_unix_jsonrpc_server(&sock_path, shutdown_rx).await;

    assert!(result.is_err());
    let _ = std::fs::remove_dir(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_start_unix_jsonrpc_server_bind_fails_invalid_parent() {
    let sock_path = std::path::Path::new("/dev/null/coralreef.sock");

    let (_shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let result = start_unix_jsonrpc_server(sock_path, shutdown_rx).await;

    assert!(result.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_concurrent_connections() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("concurrent-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = r#"{"jsonrpc":"2.0","method":"shader.compile.status","params":{},"id":1}"#;
    let mut handles = Vec::new();
    for i in 0..5 {
        let path = sock_path.clone();
        let req_str = req.to_string();
        handles.push(tokio::spawn(async move {
            let resp = unix_jsonrpc_send_request(&path, &req_str).await;
            let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
            assert_eq!(parsed["id"], 1);
            assert!(parsed["result"].is_object(), "client {i} should get result");
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_multiple_requests_same_connection() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("multi-req-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();

    let req1 = r#"{"jsonrpc":"2.0","method":"shader.compile.status","params":{},"id":1}"#;
    let req2 = r#"{"jsonrpc":"2.0","method":"shader.compile.capabilities","params":{},"id":2}"#;
    writer
        .write_all(format!("{req1}\n{req2}\n").as_bytes())
        .await
        .unwrap();
    writer.shutdown().await.unwrap();

    let mut lines = BufReader::new(reader).lines();
    let resp1 = lines.next_line().await.unwrap().unwrap();
    let resp2 = lines.next_line().await.unwrap().unwrap();

    let r1: serde_json::Value = serde_json::from_str(&resp1).unwrap();
    let r2: serde_json::Value = serde_json::from_str(&resp2).unwrap();

    assert_eq!(r1["id"], 1);
    assert!(r1["result"].is_object());
    assert_eq!(r2["id"], 2);
    assert!(r2["result"].is_array());

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_client_disconnect_before_read() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("disconnect-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (_reader, mut writer) = stream.into_split();
    writer
        .write_all(
            b"{\"jsonrpc\":\"2.0\",\"method\":\"shader.compile.status\",\"params\":{},\"id\":1}\n",
        )
        .await
        .unwrap();
    drop(writer);

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let resp = unix_jsonrpc_send_request(
        &sock_path,
        r#"{"jsonrpc":"2.0","method":"shader.compile.status","params":{},"id":2}"#,
    )
    .await;
    let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(parsed["id"], 2);
    assert!(parsed["result"].is_object());

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_server_shutdown_cleans_up_socket() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("shutdown-cleanup-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (path, handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    assert!(sock_path.exists());
    let _: Result<(), _> = shutdown_tx.send(());
    handle.await.unwrap();
    assert!(!path.exists(), "socket file should be removed on shutdown");
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_empty_line_skipped() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("empty-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();

    writer.write_all(b"\n\n").await.unwrap();
    let req = r#"{"jsonrpc":"2.0","method":"shader.compile.status","params":{},"id":9}"#;
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .unwrap();
    writer.shutdown().await.unwrap();

    let mut lines = BufReader::new(reader).lines();
    let response_line = lines.next_line().await.unwrap().unwrap();
    let resp: serde_json::Value = serde_json::from_str(&response_line).unwrap();

    assert_eq!(resp["id"], 9);
    assert!(resp["result"].is_object());

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[test]
fn dispatch_status_returns_health() {
    let result = super::dispatch("shader.compile.status", serde_json::json!({}));
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(val.is_object());
    assert_eq!(val["status"], "operational");
}

#[cfg(unix)]
#[test]
fn dispatch_capabilities_returns_archs() {
    let result = super::dispatch("shader.compile.capabilities", serde_json::json!({}));
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(val.is_array());
    assert!(!val.as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn dispatch_unknown_method_returns_error() {
    let result = super::dispatch("nonexistent.method", serde_json::json!({}));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("method not found"));
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_with_object_params() {
    let params = serde_json::json!({
        "wgsl_source": "@compute @workgroup_size(64) fn main() {}",
        "arch": "sm70"
    });
    let result = super::dispatch("shader.compile.wgsl", params);
    assert!(result.is_ok());
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_with_array_params() {
    let params = serde_json::json!([{
        "wgsl_source": "@compute @workgroup_size(64) fn main() {}",
        "arch": "sm70"
    }]);
    let result = super::dispatch("shader.compile.wgsl", params);
    assert!(result.is_ok());
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_with_invalid_params_type() {
    let result = super::dispatch("shader.compile.wgsl", serde_json::json!("string"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("invalid params"));
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_with_empty_array() {
    let result = super::dispatch("shader.compile.wgsl", serde_json::json!([]));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("missing request parameter"));
}

#[cfg(unix)]
#[test]
fn dispatch_spirv_with_invalid_params_type() {
    let result = super::dispatch("shader.compile.spirv", serde_json::json!(42));
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn dispatch_spirv_with_empty_array() {
    let result = super::dispatch("shader.compile.spirv", serde_json::json!([]));
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_multi_with_object_params() {
    let params = serde_json::json!({
        "wgsl_source": "@compute @workgroup_size(64) fn main() {}",
        "targets": [{"arch": "sm_70"}]
    });
    let result = super::dispatch("shader.compile.wgsl.multi", params);
    assert!(result.is_ok());
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_multi_with_invalid_params_type() {
    let result = super::dispatch("shader.compile.wgsl.multi", serde_json::json!(true));
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_multi_with_empty_array() {
    let result = super::dispatch("shader.compile.wgsl.multi", serde_json::json!([]));
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn make_response_success_format() {
    let resp = super::make_response(serde_json::json!(1), Ok(serde_json::json!("ok")));
    let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["id"], 1);
    assert_eq!(parsed["result"], "ok");
    assert!(parsed.get("error").is_none() || parsed["error"].is_null());
}

#[cfg(unix)]
#[test]
fn make_response_error_format() {
    let resp = super::make_response(serde_json::json!(2), Err("something went wrong".to_owned()));
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
    let resp = super::make_response(serde_json::Value::Null, Ok(serde_json::json!(42)));
    let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(parsed["id"].is_null());
    assert_eq!(parsed["result"], 42);
}
