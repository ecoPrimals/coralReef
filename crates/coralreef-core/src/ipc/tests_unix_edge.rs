// SPDX-License-Identifier: AGPL-3.0-only
//! Unix JSON-RPC edge-case and integration tests.
//!
//! Covers error paths, param variations, server bind failures,
//! concurrent connections, and protocol edge cases.

#[cfg(unix)]
use std::path::PathBuf;

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

// --- SPIRV / WGSL param variations over Unix socket ---

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
    let msg = resp["error"]["message"]
        .as_str()
        .unwrap_or("")
        .to_lowercase();
    assert!(
        msg.contains("must be array or object") || msg.contains("invalid"),
        "expected error about invalid params, got: {msg}"
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

// --- Protocol / request parsing edge cases ---

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

// --- Server bind failures ---

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

// --- Concurrent connections and connection lifecycle ---

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

// --- Additional dispatch / make_response unit tests ---

#[cfg(unix)]
#[test]
fn dispatch_status_returns_health() {
    let result = super::unix_jsonrpc::dispatch("shader.compile.status", serde_json::json!({}));
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(val.is_object());
    assert_eq!(val["status"], "operational");
}

#[cfg(unix)]
#[test]
fn dispatch_capabilities_returns_archs() {
    let result =
        super::unix_jsonrpc::dispatch("shader.compile.capabilities", serde_json::json!({}));
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!(val.is_array());
    assert!(!val.as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn dispatch_unknown_method_returns_error() {
    let result = super::unix_jsonrpc::dispatch("nonexistent.method", serde_json::json!({}));
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
    let result = super::unix_jsonrpc::dispatch("shader.compile.wgsl", params);
    assert!(result.is_ok());
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_with_array_params() {
    let params = serde_json::json!([{
        "wgsl_source": "@compute @workgroup_size(64) fn main() {}",
        "arch": "sm70"
    }]);
    let result = super::unix_jsonrpc::dispatch("shader.compile.wgsl", params);
    assert!(result.is_ok());
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_with_invalid_params_type() {
    let result = super::unix_jsonrpc::dispatch("shader.compile.wgsl", serde_json::json!("string"));
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
    let result = super::unix_jsonrpc::dispatch("shader.compile.wgsl", serde_json::json!([]));
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
    let result = super::unix_jsonrpc::dispatch("shader.compile.spirv", serde_json::json!(42));
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn dispatch_spirv_with_empty_array() {
    let result = super::unix_jsonrpc::dispatch("shader.compile.spirv", serde_json::json!([]));
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_multi_with_object_params() {
    let params = serde_json::json!({
        "wgsl_source": "@compute @workgroup_size(64) fn main() {}",
        "targets": [{"arch": "sm_70"}]
    });
    let result = super::unix_jsonrpc::dispatch("shader.compile.wgsl.multi", params);
    assert!(result.is_ok());
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_multi_with_invalid_params_type() {
    let result =
        super::unix_jsonrpc::dispatch("shader.compile.wgsl.multi", serde_json::json!(true));
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn dispatch_wgsl_multi_with_empty_array() {
    let result = super::unix_jsonrpc::dispatch("shader.compile.wgsl.multi", serde_json::json!([]));
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
    let result = super::unix_jsonrpc::dispatch("shader.compile.wgsl", params);
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
    let result = super::unix_jsonrpc::dispatch("shader.compile.wgsl", params);
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
    let result = super::unix_jsonrpc::dispatch("shader.compile.spirv", params);
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
    let result = super::unix_jsonrpc::dispatch("shader.compile.wgsl", serde_json::json!(42));
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
    let result = super::unix_jsonrpc::dispatch("shader.compile.wgsl", serde_json::json!(true));
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
#[test]
fn dispatch_handler_error_returns_handler_phase() {
    let params = serde_json::json!({
        "wgsl_source": "invalid wgsl {{",
        "arch": "sm_70",
        "opt_level": 2,
        "fp64_software": true
    });
    let result = super::unix_jsonrpc::dispatch("shader.compile.wgsl", params);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        !err.to_string().is_empty(),
        "handler error should have message"
    );
}
