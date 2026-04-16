// SPDX-License-Identifier: AGPL-3.0-or-later
//! IPC fault injection and resilience tests.
//!
//! Verifies server resilience under client disconnects, malformed/truncated
//! JSON, missing required fields, oversized payloads, empty input, and version mismatch.

use super::*;
use crate::service;
use primal_rpc_client::{RpcClient, no_params};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Send a raw newline-delimited JSON-RPC payload and return the response line.
async fn raw_newline_rpc(
    addr: std::net::SocketAddr,
    body: &[u8],
) -> Result<String, std::io::Error> {
    let mut stream = tokio::net::TcpStream::connect(addr).await?;
    stream.write_all(body).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;

    let mut line = String::new();
    BufReader::new(&mut stream).read_line(&mut line).await?;
    Ok(line)
}

#[tokio::test]
async fn test_connection_resilience_client_disconnect_mid_request() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();

    for _ in 0..10 {
        let stream = tokio::net::TcpStream::connect(addr).await;
        if let Ok(mut s) = stream {
            let partial = b"{\"jsonrpc\":\"2.0\",\"method\":\"shader.compile.status\",\"params\":";
            let _ = s.write_all(partial).await;
            drop(s);
        }
    }

    let client = RpcClient::tcp_line(addr);
    let response: service::HealthResponse = client
        .request("shader.compile.status", no_params())
        .await
        .expect("server must remain healthy after client disconnect mid-request");
    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
}

#[tokio::test]
async fn test_malformed_json_truncated_payload() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();

    let truncated = b"{\"jsonrpc\":\"2.0\",\"method\":\"shader.compile.status\",\"params\":";
    let body = raw_newline_rpc(addr, truncated).await.unwrap();

    let parsed: serde_json::Value = serde_json::from_str(body.trim())
        .expect("server must return valid JSON (not crash); response should be JSON-RPC error");
    assert!(
        parsed.get("error").is_some(),
        "truncated JSON must produce JSON-RPC error: {parsed}"
    );
    assert_eq!(parsed["jsonrpc"], "2.0");
}

#[tokio::test]
async fn test_malformed_json_corrupt_payload() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();

    let corrupt = b"{ not valid json at all }";
    let body = raw_newline_rpc(addr, corrupt).await.unwrap();

    let parsed: serde_json::Value = serde_json::from_str(body.trim())
        .expect("server must return valid JSON (not crash); response should be JSON-RPC error");
    assert!(
        parsed.get("error").is_some(),
        "corrupt JSON must produce JSON-RPC error: {parsed}"
    );
    assert_eq!(parsed["jsonrpc"], "2.0");
}

#[tokio::test]
async fn test_invalid_method_name_returns_proper_error() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "nonexistent.invalid.method",
        "params": {},
        "id": 42
    });
    let body = raw_newline_rpc(addr, &serde_json::to_vec(&req).unwrap())
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(body.trim())
        .expect("server must return valid JSON for invalid method (not crash)");

    assert!(
        parsed.get("error").is_some(),
        "invalid method must produce error: {parsed}"
    );
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["id"], 42);
    let err_msg = parsed["error"]["message"]
        .as_str()
        .unwrap_or("")
        .to_lowercase();
    assert!(
        err_msg.contains("not found") || err_msg.contains("method"),
        "error should indicate method not found: {err_msg}"
    );
}

#[tokio::test]
async fn test_missing_required_field_id() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "shader.compile.status",
        "params": {}
    });
    let body = raw_newline_rpc(addr, &serde_json::to_vec(&req).unwrap())
        .await
        .unwrap();
    let body_str = body.trim();
    if body_str.is_empty() {
        return;
    }
    let parsed: serde_json::Value = serde_json::from_str(body_str)
        .expect("server must return valid JSON or empty (notification) for missing id (not crash)");
    assert!(
        parsed.get("error").is_some() || parsed.get("result").is_some() || parsed.is_null(),
        "if server responds, must be valid JSON (error, result, or null): {parsed}"
    );
}

#[tokio::test]
async fn test_missing_required_field_method() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "params": {},
        "id": 1
    });
    let body = raw_newline_rpc(addr, &serde_json::to_vec(&req).unwrap())
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(body.trim())
        .expect("server must return valid JSON for missing method (not crash)");

    assert!(
        parsed.get("error").is_some(),
        "missing method must produce JSON-RPC error: {parsed}"
    );
    assert_eq!(parsed["jsonrpc"], "2.0");
}

#[tokio::test]
async fn test_missing_required_field_params() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "shader.compile.status",
        "id": 1
    });
    let body = raw_newline_rpc(addr, &serde_json::to_vec(&req).unwrap())
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(body.trim())
        .expect("server must return valid JSON for missing params (not crash)");

    assert!(
        parsed.get("result").is_some() || parsed.get("error").is_some(),
        "missing params may be optional for no-arg methods; server must not crash: {parsed}"
    );
}

#[tokio::test]
async fn test_concurrent_stress_rapid_connect_disconnect() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();

    for _ in 0..50 {
        let stream = tokio::net::TcpStream::connect(addr).await;
        if let Ok(s) = stream {
            drop(s);
        }
    }

    let client = RpcClient::tcp_line(addr);
    let response: service::HealthResponse = client
        .request("shader.compile.status", no_params())
        .await
        .expect("server must remain healthy after rapid connect/disconnect cycles");
    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
}

#[tokio::test]
async fn test_oversized_payload_handled_gracefully() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();

    let payload_65kb = "x".repeat(65 * 1024);
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "shader.compile.wgsl",
        "params": {
            "wgsl_source": payload_65kb,
            "arch": "sm_70",
            "opt_level": 2,
            "fp64_software": true
        },
        "id": 1
    });
    let body = serde_json::to_vec(&req).unwrap();

    let response = raw_newline_rpc(addr, &body).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(response.trim()).expect(
        "server must return valid JSON for oversized payload (process or error, not crash)",
    );

    assert!(
        parsed.get("result").is_some() || parsed.get("error").is_some(),
        "server must return either result or error for oversized payload: {parsed}"
    );
}

#[tokio::test]
async fn test_empty_payload_server_survives() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();

    {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.write_all(b"\n\n\n").await.unwrap();
        stream.flush().await.unwrap();
        drop(stream);
    }

    let client = RpcClient::tcp_line(addr);
    let response: service::HealthResponse = client
        .request("shader.compile.status", no_params())
        .await
        .expect("server must remain healthy after receiving empty lines");
    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
}

#[tokio::test]
async fn test_whitespace_only_payload_server_survives() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();

    {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.write_all(b"   \t  \n").await.unwrap();
        stream.flush().await.unwrap();
        drop(stream);
    }

    let client = RpcClient::tcp_line(addr);
    let response: service::HealthResponse = client
        .request("shader.compile.status", no_params())
        .await
        .expect("server must remain healthy after receiving whitespace-only lines");
    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
}

#[tokio::test]
async fn test_version_mismatch_returns_error() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();

    let req = serde_json::json!({
        "jsonrpc": "1.0",
        "method": "shader.compile.status",
        "params": {},
        "id": 1
    });
    let body = raw_newline_rpc(addr, &serde_json::to_vec(&req).unwrap())
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(body.trim())
        .expect("server must return valid JSON for wrong version (not crash)");

    assert!(
        parsed.get("error").is_some(),
        "wrong jsonrpc version (1.0) must produce error: {parsed}"
    );
    assert_eq!(parsed["jsonrpc"], "2.0");
}
