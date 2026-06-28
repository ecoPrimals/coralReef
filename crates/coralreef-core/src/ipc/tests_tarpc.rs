// SPDX-License-Identifier: AGPL-3.0-or-later
//! tarpc (TCP and Unix) endpoint tests — server lifecycle, health, and identity.

// All panic!("expected TCP address") below are test-only assertions:
// start_tarpc_tcp_server returns BoundAddr::Tcp by design.
use super::*;
use crate::service;
use bytes::Bytes;
use tokio_serde::formats::Bincode;

#[tokio::test]
async fn test_tarpc_tcp_server_starts() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    assert!(matches!(addr, BoundAddr::Tcp(_)));
}

#[tokio::test]
async fn test_tarpc_server_auto_tcp() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    assert!(matches!(addr, BoundAddr::Tcp(_)));
}

#[tokio::test]
async fn test_tarpc_tcp_invalid_bind_address() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let result = start_tarpc_tcp_server("not-a-valid-address", rx).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("invalid")
            || err.to_string().to_lowercase().contains("address"),
        "invalid bind should produce address parse error: {err}"
    );
}

#[tokio::test]
async fn test_tarpc_server_invalid_bind_returns_error() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let result = start_tarpc_server("garbage:not-valid", rx).await;
    assert!(result.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn test_tarpc_unix_server_starts() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("test-{}.sock", std::process::id()));

    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_unix_server(&path, rx).unwrap();
    assert!(matches!(addr, BoundAddr::Unix(_)));

    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_tarpc_unix_server_invalid_path() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let path = std::env::temp_dir();
    let result = start_tarpc_unix_server(&path, rx);
    assert!(result.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn test_tarpc_unix_server_parent_is_file() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let file_path = dir.join("blocker");
    std::fs::write(&file_path, "x").unwrap();
    let sock_path = file_path.join("nested.sock");

    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let result = start_tarpc_unix_server(&sock_path, rx);
    assert!(
        result.is_err(),
        "parent as file should prevent socket creation"
    );

    let _ = std::fs::remove_file(&file_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_tarpc_server_auto_unix() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("auto-{}.sock", std::process::id()));
    let bind = format!("unix://{}", sock_path.display());

    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_server(&bind, rx).await.unwrap();
    assert!(matches!(addr, BoundAddr::Unix(_)));

    let _ = std::fs::remove_file(&sock_path);
}

#[tokio::test]
async fn test_tarpc_health_endpoint() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let response = client.status(tarpc::context::current()).await.unwrap();

    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
}

#[tokio::test]
async fn test_tarpc_health_check() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let response = client
        .health_check(tarpc::context::current())
        .await
        .unwrap();

    assert!(response.healthy);
    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
    assert!(!response.version.is_empty());
    assert!(!response.supported_archs.is_empty());
    assert!(!response.family_id.is_empty());
}

#[tokio::test]
async fn test_tarpc_health_liveness() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let response = client
        .health_liveness(tarpc::context::current())
        .await
        .unwrap();

    assert_eq!(response.status, "alive");
}

#[tokio::test]
async fn test_tarpc_health_readiness() {
    crate::service::mark_startup();
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let response = client
        .health_readiness(tarpc::context::current())
        .await
        .unwrap();

    assert!(response.ready);
    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
}

#[test]
fn test_bound_addr_tcp_protocol_and_display() {
    let tcp_addr: std::net::SocketAddr = "127.0.0.1:9090".parse().unwrap();
    let bound = BoundAddr::Tcp(tcp_addr);
    assert_eq!(bound.protocol(), "tcp");
    assert!(bound.to_string().contains("127.0.0.1"));
    assert!(bound.to_string().contains("9090"));
}

#[cfg(unix)]
#[test]
fn test_bound_addr_unix_protocol_and_display() {
    let path = std::path::PathBuf::from("/tmp/test.sock");
    let bound = BoundAddr::Unix(path);
    assert_eq!(bound.protocol(), "unix");
    assert!(bound.to_string().contains("unix://"));
    assert!(bound.to_string().contains("test.sock"));
}

// --- IpcError and tarpc error path coverage ---

#[test]
fn test_ipc_error_invalid_address_display() {
    let err: IpcError = "not-a-valid-address"
        .parse::<std::net::SocketAddr>()
        .unwrap_err()
        .into();
    let s = err.to_string();
    assert!(
        s.to_lowercase().contains("invalid") || s.to_lowercase().contains("address"),
        "IpcError should describe address parse failure: {s}"
    );
}

#[test]
fn test_ipc_error_from_addr_parse_error() {
    use std::net::AddrParseError;
    let parse_err: AddrParseError = "garbage".parse::<std::net::SocketAddr>().unwrap_err();
    let ipc_err: IpcError = parse_err.into();
    assert!(!ipc_err.to_string().is_empty());
}

#[tokio::test]
async fn test_tarpc_tcp_bind_port_zero() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server("127.0.0.1:0", rx).await.unwrap();
    let BoundAddr::Tcp(sock_addr) = addr else {
        panic!("expected TCP address");
    };
    assert_ne!(sock_addr.port(), 0, "OS should assign a port");
}

// --- Unix tarpc client roundtrip tests ---

#[cfg(unix)]
#[tokio::test]
async fn test_tarpc_unix_status_roundtrip() {
    use tokio_util::codec::length_delimited::Builder as LengthDelimitedBuilder;

    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("tarpc-status-{}.sock", std::process::id()));

    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (_addr, _handle) = start_tarpc_unix_server(&sock_path, rx).unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream = tokio::net::UnixStream::connect(&sock_path)
        .await
        .expect("connect to tarpc unix");
    let framed = LengthDelimitedBuilder::new().new_framed(stream);
    let transport = tarpc::serde_transport::new(framed, Bincode::default());
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let response = client.status(tarpc::context::current()).await.unwrap();
    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
    assert!(!response.supported_archs.is_empty());

    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_tarpc_unix_health_check_roundtrip() {
    use tokio_util::codec::length_delimited::Builder as LengthDelimitedBuilder;

    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("tarpc-health-{}.sock", std::process::id()));

    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (_addr, _handle) = start_tarpc_unix_server(&sock_path, rx).unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream = tokio::net::UnixStream::connect(&sock_path)
        .await
        .expect("connect to tarpc unix");
    let framed = LengthDelimitedBuilder::new().new_framed(stream);
    let transport = tarpc::serde_transport::new(framed, Bincode::default());
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let response = client
        .health_check(tarpc::context::current())
        .await
        .unwrap();
    assert!(response.healthy);
    assert_eq!(response.name, env!("CARGO_PKG_NAME"));
    assert!(!response.version.is_empty());
    assert!(!response.family_id.is_empty());

    let _ = std::fs::remove_file(&sock_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_tarpc_unix_liveness_and_readiness_roundtrip() {
    use tokio_util::codec::length_delimited::Builder as LengthDelimitedBuilder;

    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("tarpc-live-{}.sock", std::process::id()));

    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (_addr, _handle) = start_tarpc_unix_server(&sock_path, rx).unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream = tokio::net::UnixStream::connect(&sock_path)
        .await
        .expect("connect to tarpc unix");
    let framed = LengthDelimitedBuilder::new().new_framed(stream);
    let transport = tarpc::serde_transport::new(framed, Bincode::default());
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let liveness = client
        .health_liveness(tarpc::context::current())
        .await
        .unwrap();
    assert_eq!(liveness.status, "alive");

    let readiness = client
        .health_readiness(tarpc::context::current())
        .await
        .unwrap();
    assert!(readiness.ready);
    assert!(!readiness.name.is_empty());

    let _ = std::fs::remove_file(&sock_path);
}

#[tokio::test]
async fn test_tarpc_health_version() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let response = client
        .health_version(tarpc::context::current())
        .await
        .unwrap();
    assert_eq!(response.name.as_ref(), env!("CARGO_PKG_NAME"));
    assert_eq!(response.version.as_ref(), env!("CARGO_PKG_VERSION"));
    assert!(!response.build_hash.is_empty());
    assert!(!response.session.is_empty());
}

// identity_get uses types (Vec<Capability>) that bincode cannot deserialize
// (DeserializeAnyNotSupported). identity.get is tested via JSON-RPC in
// tests_jsonrpc.rs and tests_unix_dispatch.rs instead.

#[tokio::test]
async fn test_tarpc_capability_list() {
    let (_tx, rx) = test_helpers::test_shutdown_channel();
    let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
    let BoundAddr::Tcp(tcp_addr) = addr else {
        panic!("expected TCP address");
    };

    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let client = ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let response = client
        .capability_list(tarpc::context::current())
        .await
        .unwrap();
    assert_eq!(response.primal.as_ref(), env!("CARGO_PKG_NAME"));
    assert!(!response.methods.is_empty());
    assert!(response.methods.iter().any(|m| m == "shader.compile.wgsl"));
    assert!(!response.capabilities.is_empty());
}
