// SPDX-License-Identifier: AGPL-3.0-only
//! IPC fault injection and resilience tests.
//!
//! Verifies server resilience under client disconnects, malformed/truncated
//! JSON, missing required fields, oversized payloads, empty input, and version mismatch.

use super::*;
use crate::service;
use primal_rpc_client::{RpcClient, no_params};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Send raw HTTP POST to the JSON-RPC server and return the response body.
async fn raw_http_post(addr: std::net::SocketAddr, body: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut stream = tokio::net::TcpStream::connect(addr).await?;
    let header = format!(
        "POST / HTTP/1.1\r\n\
         Host: localhost\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;

    let mut buf = Vec::with_capacity(8192);
    stream.read_to_end(&mut buf).await?;

    let header_end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map_or(0, |i| i + 4);
    Ok(buf[header_end..].to_vec())
}

#[tokio::test]
async fn test_connection_resilience_client_disconnect_mid_request() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();

    for _ in 0..10 {
        let stream = tokio::net::TcpStream::connect(addr).await;
        if let Ok(mut s) = stream {
            let partial = b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n{\"jsonrpc\":\"2.0\",\"method\":\"shader.compile.status\",\"params\":";
            let _ = s.write_all(partial).await;
            drop(s);
        }
    }

    let client = RpcClient::tcp(addr);
    let response: service::HealthResponse = client
        .request("shader.compile.status", no_params())
        .await
        .expect("server must remain healthy after client disconnect mid-request");
    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
}

#[tokio::test]
async fn test_malformed_json_truncated_payload() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();

    let truncated = b"{\"jsonrpc\":\"2.0\",\"method\":\"shader.compile.status\",\"params\":";
    let body = raw_http_post(addr, truncated).await.unwrap();
    let body_str = String::from_utf8_lossy(&body);

    let parsed: serde_json::Value = serde_json::from_str(&body_str)
        .expect("server must return valid JSON (not crash); response should be JSON-RPC error");
    assert!(
        parsed.get("error").is_some(),
        "truncated JSON must produce JSON-RPC error: {parsed}"
    );
    assert_eq!(parsed["jsonrpc"], "2.0");
}

#[tokio::test]
async fn test_malformed_json_corrupt_payload() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();

    let corrupt = b"{ not valid json at all }";
    let body = raw_http_post(addr, corrupt).await.unwrap();
    let body_str = String::from_utf8_lossy(&body);

    let parsed: serde_json::Value = serde_json::from_str(&body_str)
        .expect("server must return valid JSON (not crash); response should be JSON-RPC error");
    assert!(
        parsed.get("error").is_some(),
        "corrupt JSON must produce JSON-RPC error: {parsed}"
    );
    assert_eq!(parsed["jsonrpc"], "2.0");
}

#[tokio::test]
async fn test_invalid_method_name_returns_proper_error() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "nonexistent.invalid.method",
        "params": {},
        "id": 42
    });
    let body = raw_http_post(addr, &serde_json::to_vec(&req).unwrap())
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    let parsed: serde_json::Value = serde_json::from_str(&body_str)
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
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "shader.compile.status",
        "params": {}
    });
    let body = raw_http_post(addr, &serde_json::to_vec(&req).unwrap())
        .await
        .unwrap();
    let binding = String::from_utf8_lossy(&body);
    let body_str = binding.trim();
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
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "params": {},
        "id": 1
    });
    let body = raw_http_post(addr, &serde_json::to_vec(&req).unwrap())
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    let parsed: serde_json::Value = serde_json::from_str(&body_str)
        .expect("server must return valid JSON for missing method (not crash)");

    assert!(
        parsed.get("error").is_some(),
        "missing method must produce JSON-RPC error: {parsed}"
    );
    assert_eq!(parsed["jsonrpc"], "2.0");
}

#[tokio::test]
async fn test_missing_required_field_params() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "shader.compile.status",
        "id": 1
    });
    let body = raw_http_post(addr, &serde_json::to_vec(&req).unwrap())
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    let parsed: serde_json::Value = serde_json::from_str(&body_str)
        .expect("server must return valid JSON for missing params (not crash)");

    assert!(
        parsed.get("result").is_some() || parsed.get("error").is_some(),
        "missing params may be optional for no-arg methods; server must not crash: {parsed}"
    );
}

#[tokio::test]
async fn test_concurrent_stress_rapid_connect_disconnect() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();

    for _ in 0..50 {
        let stream = tokio::net::TcpStream::connect(addr).await;
        if let Ok(s) = stream {
            drop(s);
        }
    }

    let client = RpcClient::tcp(addr);
    let response: service::HealthResponse = client
        .request("shader.compile.status", no_params())
        .await
        .expect("server must remain healthy after rapid connect/disconnect cycles");
    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
}

#[tokio::test]
async fn test_oversized_payload_handled_gracefully() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();

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

    let response_body = raw_http_post(addr, &body).await.unwrap();
    let body_str = String::from_utf8_lossy(&response_body);
    let parsed: serde_json::Value = serde_json::from_str(&body_str).expect(
        "server must return valid JSON for oversized payload (process or error, not crash)",
    );

    assert!(
        parsed.get("result").is_some() || parsed.get("error").is_some(),
        "server must return either result or error for oversized payload: {parsed}"
    );
}

#[tokio::test]
async fn test_empty_payload_handled_gracefully() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();

    let body = raw_http_post(addr, b"").await.unwrap();
    let body_str = String::from_utf8_lossy(&body);

    let parsed: serde_json::Value = serde_json::from_str(&body_str)
        .expect("server must return valid JSON for empty payload (not crash)");
    assert!(
        parsed.get("error").is_some(),
        "empty payload must produce JSON-RPC error: {parsed}"
    );
}

#[tokio::test]
async fn test_whitespace_only_payload_handled_gracefully() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();

    let body = raw_http_post(addr, b"   \n\t  ").await.unwrap();
    let body_str = String::from_utf8_lossy(&body);

    let parsed: serde_json::Value = serde_json::from_str(&body_str)
        .expect("server must return valid JSON for whitespace-only payload (not crash)");
    assert!(
        parsed.get("error").is_some(),
        "whitespace-only payload must produce JSON-RPC error: {parsed}"
    );
}

#[tokio::test]
async fn test_version_mismatch_returns_error() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();

    let req = serde_json::json!({
        "jsonrpc": "1.0",
        "method": "shader.compile.status",
        "params": {},
        "id": 1
    });
    let body = raw_http_post(addr, &serde_json::to_vec(&req).unwrap())
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    let parsed: serde_json::Value = serde_json::from_str(&body_str)
        .expect("server must return valid JSON for wrong version (not crash)");

    assert!(
        parsed.get("error").is_some(),
        "wrong jsonrpc version (1.0) must produce error: {parsed}"
    );
    assert_eq!(parsed["jsonrpc"], "2.0");
}
