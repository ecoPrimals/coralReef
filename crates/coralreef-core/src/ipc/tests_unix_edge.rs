// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unix JSON-RPC edge-case and integration tests.
//!
//! Covers error paths, param variations, server bind failures,
//! concurrent connections, and protocol edge cases.

#[cfg(unix)]
use super::*;

#[cfg(unix)]
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
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
    assert!(r2["result"].is_object());

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

/// Verify the full BTSP Phase 3 encrypted transport path is reachable:
/// `handle_connection` → negotiate → `process_encrypted_frames`.
///
/// This test bypasses the accept loop to inject a valid session_id directly,
/// simulating a production connection where Phase 2 has completed.
#[cfg(unix)]
#[tokio::test]
async fn test_btsp_phase3_encrypted_frame_loop_reachable() {
    use base64::Engine;
    use tokio::io::duplex;

    let session_id = format!("phase3-integ-test-{}", std::process::id());
    let handshake_key = [0xAB; 32];

    btsp_negotiate::register_session(session_id.clone(), Some(handshake_key));

    let client_nonce_bytes = [0x01u8; 16];
    let client_nonce_b64 = base64::engine::general_purpose::STANDARD.encode(client_nonce_bytes);

    let negotiate_req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "btsp.negotiate",
        "params": {
            "session_id": session_id,
            "preferred_cipher": "chacha20-poly1305",
            "client_nonce": client_nonce_b64,
            "bond_type": "Covalent"
        },
        "id": 1
    });

    let (client_stream, server_stream) = duplex(65536);
    let (server_reader, server_writer) = tokio::io::split(server_stream);
    let server_reader = BufReader::new(server_reader);
    let (mut client_reader, mut client_writer) = tokio::io::split(client_stream);

    let sid_clone = session_id.clone();
    let server_handle = tokio::spawn(async move {
        unix_jsonrpc::handle_connection(server_reader, server_writer, Some(sid_clone)).await;
    });

    let negotiate_line = format!("{}\n", serde_json::to_string(&negotiate_req).unwrap());
    client_writer
        .write_all(negotiate_line.as_bytes())
        .await
        .unwrap();

    let mut resp_buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        client_reader.read_exact(&mut byte).await.unwrap();
        if byte[0] == b'\n' {
            break;
        }
        resp_buf.push(byte[0]);
    }
    let negotiate_resp: serde_json::Value = serde_json::from_slice(&resp_buf).unwrap();
    assert_eq!(negotiate_resp["result"]["cipher"], "chacha20-poly1305");

    let server_nonce_b64 = negotiate_resp["result"]["server_nonce"]
        .as_str()
        .expect("server_nonce must be present");
    let server_nonce_bytes = base64::engine::general_purpose::STANDARD
        .decode(server_nonce_b64)
        .unwrap();

    let keys = btsp_negotiate::SessionKeys::derive(
        &handshake_key,
        &client_nonce_bytes,
        &server_nonce_bytes,
        true, // client side
    )
    .unwrap();

    let health_req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "health.liveness",
        "params": {},
        "id": 2
    });
    let health_plaintext = serde_json::to_string(&health_req).unwrap();

    let encrypted = keys.encrypt(health_plaintext.as_bytes()).unwrap();

    #[allow(clippy::cast_possible_truncation, reason = "test payload is tiny")]
    let frame_len = (encrypted.len() as u32).to_be_bytes();
    client_writer.write_all(&frame_len).await.unwrap();
    client_writer.write_all(&encrypted).await.unwrap();
    let _ = client_writer.flush().await;

    let resp_len = client_reader.read_u32().await.unwrap();
    assert!(
        resp_len > 0 && resp_len < 65536,
        "response frame len is reasonable"
    );
    let mut resp_frame = vec![0u8; resp_len as usize];
    client_reader.read_exact(&mut resp_frame).await.unwrap();

    let resp_plaintext = keys.decrypt(&resp_frame).unwrap();
    let resp_str = std::str::from_utf8(&resp_plaintext).unwrap();
    let resp_json: serde_json::Value = serde_json::from_str(resp_str).unwrap();
    assert_eq!(resp_json["id"], 2);
    assert_eq!(resp_json["result"]["status"].as_str(), Some("alive"));

    drop(client_writer);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server_handle).await;
}
