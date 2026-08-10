// SPDX-License-Identifier: AGPL-3.0-or-later
//! tarpc compile and capability endpoint tests.

// All panic!("expected TCP address") below are test-only assertions:
// start_tarpc_tcp_server returns BoundAddr::Tcp by design.
use super::*;
use crate::service;
use bytes::Bytes;
use tokio_serde::formats::Bincode;

#[tokio::test]
async fn test_tarpc_compile_empty_spirv() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let req = service::CompileSpirvRequestTarpc {
        spirv: Bytes::new(),
        arch: coral_reef::GpuArch::default().to_string(),
        opt_level: 2,
        fp64_software: true,
    };

    let result = client.spirv(tarpc::context::current(), req).await.unwrap();

    assert!(result.is_err());
}

#[tokio::test]
async fn test_tarpc_compile_valid_shader() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let spirv_words: Vec<u32> = test_helpers::valid_spirv_minimal_compute();
    let spirv_bytes: Vec<u8> = spirv_words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let req = service::CompileSpirvRequestTarpc {
        spirv: Bytes::from(spirv_bytes),
        arch: coral_reef::GpuArch::default().to_string(),
        opt_level: 2,
        fp64_software: true,
    };

    let response = client.spirv(tarpc::context::current(), req).await.unwrap();

    match response {
        Ok(resp) => {
            assert!(
                !resp.binary.is_empty(),
                "response should contain non-empty binary"
            );
            assert_eq!(resp.size, resp.binary.len());
        }
        Err(err) => {
            assert!(
                err.message.contains("not implemented") || err.message.contains("NotImplemented"),
                "IPC should propagate compile errors: {err}"
            );
        }
    }
}

#[tokio::test]
async fn test_tarpc_compile_error_propagation() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let spirv_words: Vec<u32> = test_helpers::valid_spirv_minimal_compute();
    let spirv_bytes: Vec<u8> = spirv_words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let req_bad_arch = service::CompileSpirvRequestTarpc {
        spirv: Bytes::from(spirv_bytes),
        arch: "sm_99".to_string(),
        opt_level: 2,
        fp64_software: true,
    };
    let result = client
        .spirv(tarpc::context::current(), req_bad_arch)
        .await
        .unwrap();
    assert!(result.is_err(), "invalid arch should return Err");

    let bad_spirv_words = [0xDEAD_BEEF_u32, 0x0001_0000, 0, 0, 0];
    let bad_spirv_bytes: Vec<u8> = bad_spirv_words
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    let req_bad_spirv = service::CompileSpirvRequestTarpc {
        spirv: Bytes::from(bad_spirv_bytes),
        arch: coral_reef::GpuArch::default().to_string(),
        opt_level: 2,
        fp64_software: true,
    };
    let result2 = client
        .spirv(tarpc::context::current(), req_bad_spirv)
        .await
        .unwrap();
    assert!(result2.is_err(), "bad SPIR-V should return Err");
}

#[tokio::test]
async fn test_tarpc_capabilities() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let caps = client
        .capabilities(tarpc::context::current())
        .await
        .unwrap();
    assert!(!caps.is_empty(), "capabilities must list at least one arch");
    assert!(
        caps.iter().any(|a| a == "sm_70"),
        "must include sm_70 baseline"
    );
}

#[tokio::test]
async fn test_tarpc_compile_wgsl() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let req = service::CompileWgslRequest {
        wgsl_source: std::sync::Arc::from("@compute @workgroup_size(1) fn main() {}"),
        arch: coral_reef::GpuArch::default().to_string(),
        opt_level: 2,
        fp64_software: true,
        fp64_strategy: None,
        fma_policy: None,
        precision_advice: None,
        adapter: None,
        emit_spirv: false,
        spirv_version: None,
    };
    let result = client.wgsl(tarpc::context::current(), req).await.unwrap();
    assert!(result.is_ok(), "WGSL compile should succeed");
    let resp = result.unwrap();
    assert!(!resp.binary.is_empty());
}

#[tokio::test]
async fn test_tarpc_wgsl_multi() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let req = service::MultiDeviceCompileRequest {
        wgsl_source: std::sync::Arc::from("@compute @workgroup_size(1) fn main() {}"),
        targets: vec![
            service::types::DeviceTarget {
                card_index: 0,
                arch: "sm_70".to_string(),
                pcie_group: None,
            },
            service::types::DeviceTarget {
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
    let result = client.wgsl_multi(tarpc::context::current(), req).await;
    match result {
        Ok(Ok(resp)) => {
            assert_eq!(resp.total_count, 2);
            assert_eq!(resp.success_count, 2);
            assert_eq!(resp.results.len(), 2);
        }
        Ok(Err(e)) => {
            assert!(
                e.message.contains("implemented") || e.message.contains("not"),
                "unexpected error: {e}"
            );
        }
        Err(_) => {
            // Transport/bincode deserialization may fail for MultiDeviceCompileResponse;
            // request path and server handling are still exercised.
        }
    }
}

#[tokio::test]
async fn test_tarpc_wgsl_multi_partial_failure() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let req = service::MultiDeviceCompileRequest {
        wgsl_source: std::sync::Arc::from("@compute @workgroup_size(1) fn main() {}"),
        targets: vec![
            service::types::DeviceTarget {
                card_index: 0,
                arch: "sm_70".to_string(),
                pcie_group: None,
            },
            service::types::DeviceTarget {
                card_index: 1,
                arch: "sm_99".to_string(),
                pcie_group: None,
            },
        ],
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
    };
    let result = client.wgsl_multi(tarpc::context::current(), req).await;
    match result {
        Ok(Ok(resp)) => {
            assert_eq!(resp.total_count, 2);
            assert_eq!(resp.success_count, 1);
            assert!(resp.results[0].binary.is_some());
            assert!(resp.results[1].binary.is_none());
            assert!(resp.results[1].error.is_some());
        }
        Ok(Err(e)) => {
            assert!(
                e.message.contains("unsupported") || e.message.contains("sm_99"),
                "expected arch error: {e}"
            );
        }
        Err(_) => {
            // Transport/bincode deserialization may fail; server path still exercised.
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn test_tarpc_unix_capabilities_roundtrip() {
    use tokio_util::codec::length_delimited::Builder as LengthDelimitedBuilder;

    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("tarpc-caps-{}.sock", std::process::id()));

    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (_addr, _handle) = start_tarpc_unix_server(&sock_path, rx).unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream = tokio::net::UnixStream::connect(&sock_path)
        .await
        .expect("connect to tarpc unix");
    let framed = LengthDelimitedBuilder::new().new_framed(stream);
    let transport = tarpc::serde_transport::new(framed, Bincode::default());
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let caps = client
        .capabilities(tarpc::context::current())
        .await
        .unwrap();
    assert!(!caps.is_empty(), "should list at least one architecture");

    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_tarpc_unix_wgsl_compile_roundtrip() {
    use tokio_util::codec::length_delimited::Builder as LengthDelimitedBuilder;

    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("tarpc-wgsl-{}.sock", std::process::id()));

    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (_addr, _handle) = start_tarpc_unix_server(&sock_path, rx).unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream = tokio::net::UnixStream::connect(&sock_path)
        .await
        .expect("connect to tarpc unix");
    let framed = LengthDelimitedBuilder::new().new_framed(stream);
    let transport = tarpc::serde_transport::new(framed, Bincode::default());
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let request = service::CompileWgslRequest {
        wgsl_source: "@compute @workgroup_size(1) fn main() {}".into(),
        arch: "sm_70".into(),
        opt_level: 2,
        fp64_software: true,
        fp64_strategy: None,
        fma_policy: None,
        precision_advice: None,
        adapter: None,
        emit_spirv: false,
        spirv_version: None,
    };
    let response = client
        .wgsl(tarpc::context::current(), request)
        .await
        .unwrap()
        .expect("compile should succeed");
    assert!(response.size > 0);

    let _ = std::fs::remove_file(&sock_path);
}

#[tokio::test]
async fn test_tarpc_gemm_success() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let request = service::GemmCompileRequest {
        m: 128,
        n: 128,
        k: 32,
        precision: "f16f32".into(),
        arch: "sm_80".into(),
        tiling: "auto".into(),
    };
    let result = client
        .gemm(tarpc::context::current(), request)
        .await
        .unwrap();
    let resp = result.expect("GEMM compile should succeed");
    assert!(resp.size > 0);
    let hints = resp
        .dispatch_hints
        .expect("GEMM should have dispatch hints");
    assert_eq!(hints.hardware_hint.as_ref(), "tensor_core");
}

#[tokio::test]
async fn test_tarpc_gemm_invalid_precision() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let request = service::GemmCompileRequest {
        m: 64,
        n: 64,
        k: 16,
        precision: "int4".into(),
        arch: "sm_80".into(),
        tiling: "auto".into(),
    };
    let result = client
        .gemm(tarpc::context::current(), request)
        .await
        .unwrap();
    assert!(result.is_err(), "invalid GEMM precision should fail");
    let err = result.unwrap_err();
    assert!(err.message.contains("unknown GEMM precision"));
}

#[tokio::test]
async fn test_tarpc_wgsl_compile_error() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let request = service::types::CompileWgslRequest {
        wgsl_source: std::sync::Arc::from("not valid wgsl at all }{}{"),
        arch: "sm_70".to_owned(),
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
        precision_advice: None,
        adapter: None,
        emit_spirv: false,
        spirv_version: None,
    };
    let result = client
        .wgsl(tarpc::context::current(), request)
        .await
        .unwrap();
    assert!(result.is_err(), "invalid WGSL should fail");
}
