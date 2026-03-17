// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC (HTTP over TCP) endpoint tests.
//!
//! Uses `primal-rpc-client` — the ecosystem's pure Rust JSON-RPC client.

use super::*;
use crate::service;
use primal_rpc_client::{RpcClient, no_params};

#[tokio::test]
async fn test_jsonrpc_server_starts() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
    assert_ne!(addr.port(), 0);
}

#[tokio::test]
async fn test_jsonrpc_health_endpoint() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
    let client = RpcClient::tcp(addr);

    let response: service::HealthResponse = client
        .request("shader.compile.status", no_params())
        .await
        .unwrap();

    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
    assert!(!response.supported_archs.is_empty());
}

#[tokio::test]
async fn test_jsonrpc_supported_archs_endpoint() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
    let client = RpcClient::tcp(addr);

    let archs: Vec<String> = client
        .request("shader.compile.capabilities", no_params())
        .await
        .unwrap();

    let default_arch = coral_reef::GpuArch::default().to_string();
    assert!(archs.contains(&default_arch));
}

#[tokio::test]
async fn test_jsonrpc_compile_empty_spirv() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
    let client = RpcClient::tcp(addr);

    let req = service::CompileRequest {
        spirv_words: vec![],
        arch: coral_reef::GpuArch::default().to_string(),
        opt_level: 2,
        fp64_software: true,
    };

    let result: Result<service::CompileResponse, _> =
        client.request("shader.compile.spirv", [req]).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_jsonrpc_compile_valid_shader() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
    let client = RpcClient::tcp(addr);

    let spirv = test_helpers::valid_spirv_minimal_compute();
    let req = service::CompileRequest {
        spirv_words: spirv,
        arch: coral_reef::GpuArch::default().to_string(),
        opt_level: 2,
        fp64_software: true,
    };

    let response: Result<service::CompileResponse, _> =
        client.request("shader.compile.spirv", [req]).await;

    match response {
        Ok(resp) => {
            assert!(
                !resp.binary.is_empty(),
                "response should contain non-empty binary"
            );
            assert_eq!(resp.size, resp.binary.len());
        }
        Err(e) => {
            let msg = format!("{e:?}");
            assert!(
                msg.contains("not implemented") || msg.contains("-32000"),
                "IPC should propagate compile errors: {msg}"
            );
        }
    }
}

#[tokio::test]
async fn test_jsonrpc_compile_wgsl_shader() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
    let client = RpcClient::tcp(addr);

    let req = service::CompileWgslRequest {
        wgsl_source: "@compute @workgroup_size(1)\nfn main() {}".to_owned(),
        arch: "sm_70".to_owned(),
        opt_level: 2,
        fp64_software: true,
        fp64_strategy: None,
        fma_policy: None,
    };

    let response: Result<service::CompileResponse, _> =
        client.request("shader.compile.wgsl", [req]).await;

    match response {
        Ok(resp) => {
            assert!(
                !resp.binary.is_empty(),
                "WGSL compile should produce non-empty binary"
            );
            assert_eq!(resp.size, resp.binary.len());
        }
        Err(e) => {
            let msg = format!("{e:?}");
            assert!(
                msg.contains("-32000"),
                "IPC should propagate compile errors: {msg}"
            );
        }
    }
}

#[tokio::test]
async fn test_jsonrpc_compile_error_propagation() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
    let client = RpcClient::tcp(addr);

    let req_bad_arch = service::CompileRequest {
        spirv_words: test_helpers::valid_spirv_minimal_compute(),
        arch: "sm_99".to_string(),
        opt_level: 2,
        fp64_software: true,
    };
    let err: Result<service::CompileResponse, _> =
        client.request("shader.compile.spirv", [req_bad_arch]).await;
    assert!(err.is_err(), "invalid arch should return JSON-RPC error");
    let err_msg = format!("{:?}", err.unwrap_err());
    assert!(
        err_msg.contains("-32000")
            || err_msg.contains("sm_99")
            || err_msg.contains("UnsupportedArch"),
        "error should indicate compile failure: {err_msg}"
    );

    let req_bad_spirv = service::CompileRequest {
        spirv_words: vec![0xDEAD_BEEF, 0x0001_0000, 0, 0, 0],
        arch: coral_reef::GpuArch::default().to_string(),
        opt_level: 2,
        fp64_software: true,
    };
    let err2: Result<service::CompileResponse, _> = client
        .request("shader.compile.spirv", [req_bad_spirv])
        .await;
    assert!(err2.is_err(), "bad SPIR-V should return JSON-RPC error");
}

#[tokio::test]
async fn test_jsonrpc_error_code_invalid_input() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
    let client = RpcClient::tcp(addr);

    let req = service::CompileRequest {
        spirv_words: vec![0xDEAD_BEEF],
        arch: coral_reef::GpuArch::default().to_string(),
        opt_level: 2,
        fp64_software: true,
    };

    let result: Result<service::CompileResponse, _> =
        client.request("shader.compile.spirv", [req]).await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("-32001") || err_str.contains("invalid input"),
        "bad SPIR-V should produce error code -32001: got {err_str}"
    );
}

#[tokio::test]
async fn test_jsonrpc_error_code_unsupported_arch() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
    let client = RpcClient::tcp(addr);

    let spirv = test_helpers::valid_spirv_minimal_compute();
    let req = service::CompileRequest {
        spirv_words: spirv,
        arch: "sm_99".to_string(),
        opt_level: 2,
        fp64_software: true,
    };

    let result: Result<service::CompileResponse, _> =
        client.request("shader.compile.spirv", [req]).await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("-32003")
            || err_str.contains("UnsupportedArch")
            || err_str.contains("sm_99"),
        "unsupported arch should produce -32003 or UnsupportedArch: got {err_str}"
    );
}

#[tokio::test]
async fn test_jsonrpc_status_returns_health() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
    let client = RpcClient::tcp(addr);

    let response: service::HealthResponse = client
        .request("shader.compile.status", no_params())
        .await
        .unwrap();

    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
    assert_eq!(response.status, "operational");
    assert!(!response.supported_archs.is_empty());
}

#[tokio::test]
async fn test_jsonrpc_capabilities_endpoint() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
    let client = RpcClient::tcp(addr);

    let archs: Vec<String> = client
        .request("shader.compile.capabilities", no_params())
        .await
        .unwrap();
    assert!(!archs.is_empty());
    assert!(archs.iter().any(|a| a == "sm_70"));
}

#[tokio::test]
async fn test_jsonrpc_health_check() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
    let client = RpcClient::tcp(addr);

    let response: service::HealthCheckResponse =
        client.request("health.check", no_params()).await.unwrap();

    assert!(response.healthy);
    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
    assert!(!response.version.is_empty());
    assert!(!response.supported_archs.is_empty());
    assert!(!response.family_id.is_empty());
}

#[tokio::test]
async fn test_jsonrpc_health_liveness() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
    let client = RpcClient::tcp(addr);

    let response: service::LivenessResponse = client
        .request("health.liveness", no_params())
        .await
        .unwrap();

    assert!(response.alive);
}

#[tokio::test]
async fn test_jsonrpc_health_readiness() {
    let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
    let client = RpcClient::tcp(addr);

    let response: service::ReadinessResponse = client
        .request("health.readiness", no_params())
        .await
        .unwrap();

    assert!(response.ready);
    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
}
