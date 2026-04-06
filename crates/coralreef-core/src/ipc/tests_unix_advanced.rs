// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unix JSON-RPC advanced coverage: socket setup failures, environment paths,
//! large payloads, concurrent races, and drop semantics.

#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
use super::*;

#[cfg(unix)]
use tokio::io::{AsyncWriteExt, BufReader};
#[cfg(unix)]
use tokio::net::UnixStream;

#[cfg(unix)]
async fn unix_jsonrpc_send_request(sock_path: &std::path::Path, request: &str) -> String {
    use tokio::io::AsyncBufReadExt;
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

/// UTF-8 bytes of padding embedded in a JSON string for a large newline-delimited request.
#[cfg(unix)]
const LARGE_JSON_RPC_PAD_BYTES: usize = 256 * 1024;

#[cfg(unix)]
#[tokio::test]
async fn test_start_unix_jsonrpc_server_create_dir_all_fails_when_parent_is_file() {
    let tmp = tempfile::tempdir().unwrap();
    let blocking = tmp.path().join("blocker");
    std::fs::write(&blocking, b"x").unwrap();
    let sock_path = blocking.join("nested.sock");

    let (_shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let result = start_unix_jsonrpc_server(&sock_path, shutdown_rx).await;

    assert!(result.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn test_start_unix_jsonrpc_server_removes_stale_socket_file() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("stale-sock-{}.sock", std::process::id()));
    std::fs::write(&sock_path, b"stale").unwrap();
    assert!(sock_path.exists());

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .expect("bind should succeed after removing stale socket path");

    let resp_line = unix_jsonrpc_send_request(
        &sock_path,
        r#"{"jsonrpc":"2.0","method":"health.liveness","params":{},"id":1}"#,
    )
    .await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();
    assert_eq!(resp["result"]["alive"], true);

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_client_disconnect_mid_line_without_newline() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("mid-line-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    {
        let stream = UnixStream::connect(&sock_path).await.unwrap();
        let (_, mut writer) = stream.into_split();
        writer
            .write_all(br#"{"jsonrpc":"2.0","method":"shader.compile.status","par"#)
            .await
            .unwrap();
    }

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let resp_line = unix_jsonrpc_send_request(
        &sock_path,
        r#"{"jsonrpc":"2.0","method":"shader.compile.status","params":{},"id":2}"#,
    )
    .await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();
    assert_eq!(resp["id"], 2);
    assert!(resp["result"].is_object());

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_very_large_request_line() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("large-req-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "health.liveness",
        "params": { "pad": "x".repeat(LARGE_JSON_RPC_PAD_BYTES) },
        "id": 77
    });
    let resp_line = unix_jsonrpc_send_request(&sock_path, &req.to_string()).await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();
    assert_eq!(resp["id"], 77);
    assert_eq!(resp["result"]["alive"], true);

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_concurrent_many_connections() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("concurrent-many-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let req = r#"{"jsonrpc":"2.0","method":"shader.compile.status","params":{},"id":1}"#;
    let mut handles = Vec::new();
    for i in 0..16 {
        let path = sock_path.clone();
        let req_str = req.to_string();
        handles.push(tokio::spawn(async move {
            let resp = unix_jsonrpc_send_request(&path, &req_str).await;
            let parsed: serde_json::Value = serde_json::from_str(&resp).unwrap();
            assert_eq!(parsed["id"], 1);
            assert!(
                parsed["result"].is_object(),
                "client {i} should get a result object"
            );
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
async fn test_unix_jsonrpc_write_all_error_when_peer_closes_early() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("early-close-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_path, _handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    let mut tasks = Vec::new();
    for _ in 0..32 {
        let path = sock_path.clone();
        tasks.push(tokio::spawn(async move {
            let stream = UnixStream::connect(&path).await.unwrap();
            let (_, mut writer) = stream.into_split();
            writer
                .write_all(
                    b"{\"jsonrpc\":\"2.0\",\"method\":\"shader.compile.status\",\"params\":{},\"id\":1}\n",
                )
                .await
                .unwrap();
        }));
    }
    for t in tasks {
        let _ = t.await;
    }

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let resp_line = unix_jsonrpc_send_request(
        &sock_path,
        r#"{"jsonrpc":"2.0","method":"shader.compile.status","params":{},"id":2}"#,
    )
    .await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();
    assert_eq!(resp["id"], 2);
    assert!(resp["result"].is_object());

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_jsonrpc_drop_join_handle_keeps_listener_until_shutdown() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("drop-handle-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_bound, handle) = start_unix_jsonrpc_server(&sock_path, shutdown_rx)
        .await
        .unwrap();

    assert!(sock_path.exists());
    drop(handle);

    let resp_line = unix_jsonrpc_send_request(
        &sock_path,
        r#"{"jsonrpc":"2.0","method":"health.check","params":{},"id":3}"#,
    )
    .await;
    let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();
    assert_eq!(resp["id"], 3);
    assert_eq!(resp["result"]["healthy"], true);

    let _: Result<(), _> = shutdown_tx.send(());
    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[test]
fn default_unix_socket_path_matches_base_from_process_environment() {
    let resolved = std::env::var("XDG_RUNTIME_DIR").ok().map(PathBuf::from);
    let expected = unix_socket_path_for_base(resolved);
    let got = default_unix_socket_path();
    assert_eq!(got, expected);
}
