// SPDX-License-Identifier: AGPL-3.0-only
//! Tests for newline-delimited JSON-RPC over TCP (`newline_jsonrpc`).

use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use super::*;
use crate::service;

#[tokio::test]
async fn newline_tcp_request_response_roundtrip() {
    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (addr, handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", shutdown_rx)
        .await
        .unwrap();
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let req = json!({
        "jsonrpc": "2.0",
        "method": "health.check",
        "params": {},
        "id": 1_u64
    });
    let line = format!("{}\n", serde_json::to_string(&req).unwrap());
    stream.write_all(line.as_bytes()).await.unwrap();
    let mut reader = BufReader::new(stream);
    let mut out = String::new();
    reader.read_line(&mut out).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert!(
        v.get("result").is_some(),
        "expected JSON-RPC result object: {v}"
    );
    let _ = shutdown_tx.send(());
    handle.abort();
}

#[tokio::test]
async fn newline_tcp_invalid_json_returns_error_object() {
    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (addr, handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", shutdown_rx)
        .await
        .unwrap();
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(b"{ this is not json }\n").await.unwrap();
    let mut reader = BufReader::new(stream);
    let mut out = String::new();
    reader.read_line(&mut out).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert!(
        v.get("error").is_some(),
        "expected JSON-RPC error for invalid JSON: {v}"
    );
    let _ = shutdown_tx.send(());
    handle.abort();
}

#[tokio::test]
async fn newline_tcp_multiple_requests_one_connection() {
    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (addr, handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", shutdown_rx)
        .await
        .unwrap();
    let mut stream = TcpStream::connect(addr).await.unwrap();
    for id in [1_u64, 2_u64] {
        let req = json!({
            "jsonrpc": "2.0",
            "method": "health.liveness",
            "params": {},
            "id": id
        });
        let line = format!("{}\n", serde_json::to_string(&req).unwrap());
        stream.write_all(line.as_bytes()).await.unwrap();
    }
    let mut reader = BufReader::new(stream);
    for expected_id in [1_u64, 2_u64] {
        let mut out = String::new();
        reader.read_line(&mut out).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(v.get("id"), Some(&json!(expected_id)));
        assert!(v.get("result").is_some() || v.get("error").is_some());
    }
    let _ = shutdown_tx.send(());
    handle.abort();
}

#[tokio::test]
async fn newline_tcp_connection_drop_without_request_closes_cleanly() {
    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (addr, handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", shutdown_rx)
        .await
        .unwrap();
    let stream = TcpStream::connect(addr).await.unwrap();
    drop(stream);
    let _ = shutdown_tx.send(());
    handle.abort();
}

#[tokio::test]
async fn newline_tcp_invalid_jsonrpc_version_returns_error() {
    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (addr, handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", shutdown_rx)
        .await
        .unwrap();
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let req = json!({
        "jsonrpc": "1.0",
        "method": "identity.get",
        "params": {},
        "id": 7_u64
    });
    let line = format!("{}\n", serde_json::to_string(&req).unwrap());
    stream.write_all(line.as_bytes()).await.unwrap();
    let mut reader = BufReader::new(stream);
    let mut out = String::new();
    reader.read_line(&mut out).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert!(v.get("error").is_some());
    let _ = shutdown_tx.send(());
    handle.abort();
}

#[tokio::test]
async fn newline_tcp_shader_compile_status_roundtrip() {
    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (addr, handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", shutdown_rx)
        .await
        .unwrap();
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let req = json!({
        "jsonrpc": "2.0",
        "method": "shader.compile.status",
        "params": {},
        "id": "a"
    });
    let line = format!("{}\n", serde_json::to_string(&req).unwrap());
    stream.write_all(line.as_bytes()).await.unwrap();
    let mut reader = BufReader::new(stream);
    let mut out = String::new();
    reader.read_line(&mut out).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    let result = v.get("result").expect("result");
    let hr: service::HealthResponse = serde_json::from_value(result.clone()).unwrap();
    assert_eq!(hr.name.as_ref(), env!("CARGO_PKG_NAME"));
    let _ = shutdown_tx.send(());
    handle.abort();
}
