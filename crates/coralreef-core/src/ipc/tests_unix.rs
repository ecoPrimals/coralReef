// SPDX-License-Identifier: AGPL-3.0-only
//! Unix JSON-RPC (newline-delimited over Unix socket) tests.

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
fn test_unix_socket_path_with_xdg() {
    let path = unix_socket_path_for_base(Some("/run/user/1234".into()));
    assert!(path.to_string_lossy().contains("/run/user/1234"));
    assert!(path.to_string_lossy().contains("biomeos"));
    assert!(path.to_string_lossy().contains("coralreef.sock"));
}

#[cfg(unix)]
#[test]
fn test_unix_socket_path_fallback() {
    let path = unix_socket_path_for_base(None);
    assert!(path.to_string_lossy().contains("biomeos"));
    assert!(path.to_string_lossy().contains("coralreef.sock"));
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
