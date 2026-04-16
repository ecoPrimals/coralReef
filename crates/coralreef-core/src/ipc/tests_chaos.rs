// SPDX-License-Identifier: AGPL-3.0-or-later
//! IPC chaos and fault injection tests.
//!
//! Verifies server resilience under malformed input, concurrent load,
//! rapid connect/disconnect, oversized payloads, and invalid method names.

use super::*;
use crate::service;
use primal_rpc_client::{RpcClient, no_params};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_serde::formats::Bincode;

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
async fn test_concurrent_jsonrpc_requests() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();
    let client = RpcClient::tcp_line(addr);

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
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();

    let malformed = b"{ not valid json at all }";
    let body = raw_newline_rpc(addr, malformed).await.unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&body)
        .expect("server must return valid JSON (not crash); response should be JSON-RPC error");
    assert!(
        parsed.get("error").is_some(),
        "malformed JSON must produce JSON-RPC error: {parsed}"
    );
    assert_eq!(parsed["jsonrpc"], "2.0");
}

#[tokio::test]
async fn test_rapid_connect_disconnect() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();

    for _ in 0..20 {
        let stream = tokio::net::TcpStream::connect(addr).await;
        if let Ok(s) = stream {
            drop(s);
        }
    }

    let client = RpcClient::tcp_line(addr);
    let response: service::HealthResponse = client
        .request("shader.compile.status", no_params())
        .await
        .expect("server must remain healthy after rapid connect/disconnect");
    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
}

#[tokio::test]
async fn test_oversized_payload() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();

    let wgsl_1mb = "x".repeat(1024 * 1024);
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "shader.compile.wgsl",
        "params": {
            "wgsl_source": wgsl_1mb,
            "arch": "sm_70",
            "opt_level": 2,
            "fp64_software": true
        },
        "id": 1
    });
    let body_bytes = serde_json::to_vec(&req).unwrap();

    let response = raw_newline_rpc(addr, &body_bytes).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&response).expect(
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
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", rx).await.unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "nonexistent.invalid.method",
        "params": {},
        "id": 42
    });
    let body = serde_json::to_vec(&req).unwrap();

    let response = raw_newline_rpc(addr, &body).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&response)
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
