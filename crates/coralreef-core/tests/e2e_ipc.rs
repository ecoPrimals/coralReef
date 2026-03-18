// SPDX-License-Identifier: AGPL-3.0-only
//! End-to-end integration tests for the coralReef IPC layer.
//!
//! Starts both JSON-RPC and tarpc servers on random ports, exercises all
//! semantic methods, verifies JSON-RPC 2.0 format, and gracefully shuts down.
//!
//! Run with: `cargo test -p coralreef-core --test e2e_ipc --features e2e`
#![cfg(feature = "e2e")]

use coralreef_core::ipc::{
    BoundAddr, FALLBACK_TCP_BIND, ShaderCompileTarpcClient, start_jsonrpc_server,
    start_tarpc_tcp_server,
};
use coralreef_core::service;
use primal_rpc_client::{RpcClient, no_params};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::watch;
use tokio_serde::formats::Bincode;

/// Send a raw JSON-RPC request and return the response body for format verification.
async fn raw_jsonrpc_request(
    addr: std::net::SocketAddr,
    method: &str,
    params: Value,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = tokio::net::TcpStream::connect(addr).await?;
    let id = 1u64;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": id
    });
    let body_bytes = body.to_string();
    let header = format!(
        "POST / HTTP/1.1\r\n\
         Host: localhost\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n",
        body_bytes.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body_bytes.as_bytes()).await?;
    stream.flush().await?;

    let mut buf = Vec::with_capacity(4096);
    stream.read_to_end(&mut buf).await?;

    let header_end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("missing header/body separator")?;
    let body_start = header_end + 4;
    Ok(buf[body_start..].to_vec())
}

/// Verify the response is valid JSON-RPC 2.0 format.
fn assert_jsonrpc_2_0_format(body: &[u8]) {
    let v: Value = serde_json::from_slice(body).expect("response must be valid JSON");
    assert!(
        v.get("jsonrpc").and_then(|j| j.as_str()) == Some("2.0"),
        "response must have jsonrpc: \"2.0\", got: {v:?}"
    );
    assert!(v.get("id").is_some(), "response must have id, got: {v:?}");
    let has_result = v.get("result").is_some();
    let has_error = v.get("error").is_some();
    assert!(
        has_result || has_error,
        "response must have result or error, got: {v:?}"
    );
}

#[tokio::test]
async fn e2e_ipc_full_integration() {
    // 1. Start JSON-RPC server on random port (127.0.0.1:0)
    let (jsonrpc_addr, jsonrpc_handle) = start_jsonrpc_server(FALLBACK_TCP_BIND)
        .await
        .expect("JSON-RPC server must start");
    assert_ne!(
        jsonrpc_addr.port(),
        0,
        "JSON-RPC should bind to OS-assigned port"
    );

    // 2. Start tarpc server on random port
    let (shutdown_tx, shutdown_rx) = watch::channel(());
    let (tarpc_addr, tarpc_handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, shutdown_rx)
        .await
        .expect("tarpc server must start");
    let BoundAddr::Tcp(tarpc_tcp_addr) = tarpc_addr else {
        panic!("expected TCP address for tarpc");
    };
    assert_ne!(
        tarpc_tcp_addr.port(),
        0,
        "tarpc should bind to OS-assigned port"
    );

    let client = RpcClient::tcp(jsonrpc_addr);

    // 3. Test all semantic methods via JSON-RPC HTTP

    // shader.compile.wgsl
    let wgsl_req = service::CompileWgslRequest {
        wgsl_source: std::sync::Arc::from("@compute @workgroup_size(1)\nfn main() {}"),
        arch: coral_reef::GpuArch::default().to_string(),
        opt_level: 2,
        fp64_software: true,
        fp64_strategy: None,
        fma_policy: None,
    };
    let wgsl_result: Result<service::CompileResponse, _> =
        client.request("shader.compile.wgsl", [wgsl_req]).await;
    match &wgsl_result {
        Ok(resp) => {
            assert!(!resp.binary.is_empty());
            assert_eq!(resp.size, resp.binary.len());
        }
        Err(_) => {
            // May fail with -32000 if NVVM not available
        }
    }

    // shader.compile.status
    let status: service::HealthResponse = client
        .request("shader.compile.status", no_params())
        .await
        .unwrap();
    assert_eq!(status.name, env!("CARGO_PKG_NAME"));
    assert!(!status.supported_archs.is_empty());
    assert_eq!(status.status, "operational");

    // shader.compile.capabilities
    let archs: Vec<String> = client
        .request("shader.compile.capabilities", no_params())
        .await
        .unwrap();
    let default_arch = coral_reef::GpuArch::default().to_string();
    assert!(archs.contains(&default_arch));

    // health.check
    let health_check: service::HealthCheckResponse =
        client.request("health.check", no_params()).await.unwrap();
    assert_eq!(health_check.name, env!("CARGO_PKG_NAME"));
    assert!(health_check.healthy);
    assert!(!health_check.supported_archs.is_empty());

    // health.liveness
    let liveness: service::LivenessResponse = client
        .request("health.liveness", no_params())
        .await
        .unwrap();
    assert!(liveness.alive);

    // health.readiness
    let readiness: service::ReadinessResponse = client
        .request("health.readiness", no_params())
        .await
        .unwrap();
    assert!(readiness.ready);
    assert_eq!(readiness.name, env!("CARGO_PKG_NAME"));

    // 4. Test health methods via tarpc
    let transport = tarpc::serde_transport::tcp::connect(tarpc_tcp_addr, || Bincode::default())
        .await
        .unwrap();
    let tarpc_client =
        ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let tarpc_status = tarpc_client
        .status(tarpc::context::current())
        .await
        .unwrap();
    assert_eq!(tarpc_status.name, env!("CARGO_PKG_NAME"));

    let tarpc_health_check = tarpc_client
        .health_check(tarpc::context::current())
        .await
        .unwrap();
    assert!(tarpc_health_check.healthy);

    let tarpc_liveness = tarpc_client
        .health_liveness(tarpc::context::current())
        .await
        .unwrap();
    assert!(tarpc_liveness.alive);

    let tarpc_readiness = tarpc_client
        .health_readiness(tarpc::context::current())
        .await
        .unwrap();
    assert!(tarpc_readiness.ready);

    // 5. Test shader.compile.wgsl.multi with multiple targets
    let multi_req = service::MultiDeviceCompileRequest {
        wgsl_source: std::sync::Arc::from("@compute @workgroup_size(1)\nfn main() {}"),
        targets: vec![
            service::DeviceTarget {
                card_index: 0,
                arch: "sm_70".to_string(),
                pcie_group: None,
            },
            service::DeviceTarget {
                card_index: 1,
                arch: "sm_89".to_string(),
                pcie_group: Some(0),
            },
        ],
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
    };
    let multi_result: Result<service::MultiDeviceCompileResponse, _> = client
        .request("shader.compile.wgsl.multi", [multi_req])
        .await;
    match multi_result {
        Ok(resp) => {
            assert_eq!(resp.total_count, 2);
            assert!(resp.results.len() == 2);
        }
        Err(_) => {
            // May fail if NVVM not available
        }
    }

    // 6. Verify responses are correct JSON-RPC 2.0 format
    let raw_status =
        raw_jsonrpc_request(jsonrpc_addr, "shader.compile.status", serde_json::json!([]))
            .await
            .unwrap();
    assert_jsonrpc_2_0_format(&raw_status);

    let raw_health = raw_jsonrpc_request(jsonrpc_addr, "health.check", serde_json::json!([]))
        .await
        .unwrap();
    assert_jsonrpc_2_0_format(&raw_health);

    // 7. Gracefully shutdown both servers
    let _: Result<(), _> = shutdown_tx.send(());
    let _ = jsonrpc_handle.stop();

    let shutdown_timeout = std::time::Duration::from_secs(5);
    let rpc_stopped = jsonrpc_handle.clone().stopped();
    let shutdown_result = tokio::time::timeout(shutdown_timeout, async move {
        rpc_stopped.await;
        tarpc_handle.await.ok();
    })
    .await;

    assert!(
        shutdown_result.is_ok(),
        "servers should shut down cleanly within timeout"
    );
}
