// SPDX-License-Identifier: AGPL-3.0-or-later
//! Async Unix JSON-RPC integration tests (server-based).

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
    assert!(resp["result"].is_object());
    let result = &resp["result"];
    let archs = result["targets"].as_array().expect("Gate 1: targets array");
    assert!(!archs.is_empty());
    assert!(archs.iter().any(|a| a.as_str() == Some("sm_70")));
    assert_eq!(result["f64_transcendentals"]["composite_lowering"], true);

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
    assert_eq!(resp["result"]["status"], "alive");

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_health_readiness() {
    crate::service::mark_startup();
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

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_blank_lines_ignored() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("blank-lines-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let stream = tokio::net::UnixStream::connect(&sock_path)
        .await
        .expect("connect");
    let (reader, mut writer) = stream.into_split();
    let mut lines = tokio::io::BufReader::new(reader).lines();

    writer.write_all(b"\n\n   \n").await.unwrap();
    let real_req = r#"{"jsonrpc":"2.0","method":"health.liveness","params":{},"id":42}"#;
    writer
        .write_all(format!("{real_req}\n").as_bytes())
        .await
        .unwrap();

    let resp_line = tokio::time::timeout(std::time::Duration::from_secs(5), lines.next_line())
        .await
        .expect("timeout")
        .expect("io")
        .expect("line");
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();
    assert_eq!(resp["id"], 42);
    assert_eq!(resp["result"]["status"], "alive");

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_invalid_version_returns_error() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("bad-version-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = r#"{"jsonrpc":"1.0","method":"health.check","params":{},"id":7}"#;
    let resp_line = unix_jsonrpc_send_request(&sock_path, req).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 7);
    assert!(
        resp["error"].is_object(),
        "invalid version should produce error"
    );
    assert!(
        resp["error"]["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("version")
    );

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_malformed_json_returns_parse_error() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("malformed-json-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = r"{this is not valid json at all}";
    let resp_line = unix_jsonrpc_send_request(&sock_path, req).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert!(resp["id"].is_null(), "malformed JSON should have null id");
    assert!(resp["error"].is_object());
    assert!(
        resp["error"]["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("parse error")
    );

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_compile_invalid_wgsl_returns_error_response() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("bad-wgsl-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = r#"{"jsonrpc":"2.0","method":"shader.compile.wgsl","params":{"wgsl_source":"INVALID{{{","arch":"sm_70"},"id":99}"#;
    let resp_line = unix_jsonrpc_send_request(&sock_path, req).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 99);
    assert!(
        resp["error"].is_object(),
        "invalid WGSL should produce error response"
    );

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}
