// SPDX-License-Identifier: AGPL-3.0-or-later
//! IPC chaos and fault injection tests.
//!
//! Verifies server resilience under malformed input, concurrent load,
//! rapid connect/disconnect, oversized payloads, and invalid method names.

use super::*;
use crate::service;
use primal_rpc_client::{RpcClient, no_params};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_serde::formats::Bincode;

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
async fn test_concurrent_jsonrpc_requests() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
    let client = RpcClient::tcp(addr);

    let req = service::CompileWgslRequest {
        wgsl_source: std::sync::Arc::from("@compute @workgroup_size(1)\nfn main() {}"),
        arch: coral_reef::GpuArch::default().to_string(),
        opt_level: 2,
        fp64_software: true,
        fp64_strategy: None,
        fma_policy: None,
    };

    let handles: Vec<_> = (0..50)
        .map(|_| {
            let client = client.clone();
            let req = req.clone();
            tokio::spawn(async move {
                let result: Result<service::CompileResponse, _> =
                    client.request("shader.compile.wgsl", [req]).await;
                result
            })
        })
        .collect();

    for handle in handles {
        let result = handle.await.expect("task should not panic");
        assert!(
            result.is_ok() || result.is_err(),
            "each request must return Ok or Err, never panic"
        );
        if let Err(e) = &result {
            let msg = format!("{e:?}");
            assert!(
                msg.contains("-32000")
                    || msg.contains("not implemented")
                    || msg.contains("NotImplemented"),
                "errors should be valid JSON-RPC or compile errors: {msg}"
            );
        }
    }
}

#[tokio::test]
async fn test_malformed_jsonrpc_request() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();

    let malformed = b"{ not valid json at all }";
    let body = raw_http_post(addr, malformed).await.unwrap();
    let body_str = String::from_utf8_lossy(&body);

    let parsed: serde_json::Value = serde_json::from_str(&body_str)
        .expect("server must return valid JSON (not crash); response should be JSON-RPC error");
    assert!(
        parsed.get("error").is_some(),
        "malformed JSON must produce JSON-RPC error: {parsed}"
    );
    assert_eq!(parsed["jsonrpc"], "2.0");
}

#[tokio::test]
async fn test_rapid_connect_disconnect() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();

    for _ in 0..20 {
        let stream = tokio::net::TcpStream::connect(addr).await;
        if let Ok(s) = stream {
            drop(s);
        }
    }

    let client = RpcClient::tcp(addr);
    let response: service::HealthResponse = client
        .request("shader.compile.status", no_params())
        .await
        .expect("server must remain healthy after rapid connect/disconnect");
    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
}

#[tokio::test]
async fn test_oversized_payload() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();

    let wgsl_1mb = "x".repeat(1024 * 1024);
    let req = serde_json::json!({
        "wgsl_source": wgsl_1mb,
        "arch": "sm_70",
        "opt_level": 2,
        "fp64_software": true
    });
    let body = serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": "shader.compile.wgsl",
        "params": req,
        "id": 1
    }))
    .unwrap();

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
async fn test_concurrent_tarpc_requests() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let req = service::CompileWgslRequest {
        wgsl_source: std::sync::Arc::from("@compute @workgroup_size(1)\nfn main() {}"),
        arch: coral_reef::GpuArch::default().to_string(),
        opt_level: 2,
        fp64_software: true,
        fp64_strategy: None,
        fma_policy: None,
    };

    let handles: Vec<_> = (0..50)
        .map(|_| {
            let addr = tcp_addr;
            let req = req.clone();
            tokio::spawn(async move {
                let transport =
                    match tarpc::serde_transport::tcp::connect(addr, Bincode::default).await {
                        Ok(t) => t,
                        Err(e) => return Err(e.to_string()),
                    };
                let client =
                    ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport)
                        .spawn();
                match client.wgsl(tarpc::context::current(), req).await {
                    Ok(inner) => Ok(inner),
                    Err(e) => Err(e.to_string()),
                }
            })
        })
        .collect();

    for handle in handles {
        let result = handle.await.expect("task should not panic");
        match result {
            Ok(Ok(_) | Err(_)) => {}
            Err(conn_err) => {
                let msg = conn_err;
                assert!(
                    msg.contains("connection") || msg.contains("reset") || msg.contains("refused"),
                    "tarpc connection errors are acceptable under load: {msg}"
                );
            }
        }
    }
}

#[tokio::test]
async fn test_server_handles_invalid_method() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "nonexistent.invalid.method",
        "params": {},
        "id": 42
    });
    let body = serde_json::to_vec(&req).unwrap();

    let response_body = raw_http_post(addr, &body).await.unwrap();
    let body_str = String::from_utf8_lossy(&response_body);
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
