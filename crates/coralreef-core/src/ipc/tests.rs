// SPDX-License-Identifier: AGPL-3.0-or-later
//! IPC integration tests — cross-protocol consistency, shutdown, misc.

use super::*;
use crate::service;
use primal_rpc_client::{RpcClient, no_params};
use tokio_serde::formats::Bincode;

#[test]
fn test_ipc_error_display() {
    use super::IpcError;

    let err: IpcError = "not-an-address:xyz"
        .parse::<std::net::SocketAddr>()
        .unwrap_err()
        .into();
    let s = err.to_string();
    assert!(
        s.to_lowercase().contains("invalid") || s.to_lowercase().contains("address"),
        "IpcError should describe address parse failure: {s}"
    );
}

#[tokio::test]
async fn test_cross_protocol_health_consistency() {
    let (_tx1, rx1) = test_helpers::test_shutdown_channel();
    let (rpc_addr, _rpc_handle) =
        start_newline_tcp_jsonrpc("127.0.0.1:0", rx1).await.unwrap();
    let (_tx2, rx2) = test_helpers::test_shutdown_channel();
    let (tarpc_addr, _tarpc_handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx2)
        .await
        .unwrap();

    let rpc_client = RpcClient::tcp_line(rpc_addr);
    let jsonrpc_health: service::HealthResponse = rpc_client
        .request("shader.compile.status", no_params())
        .await
        .unwrap();

    let BoundAddr::Tcp(tcp_addr) = tarpc_addr else {
        panic!("expected TCP address");
    };
    let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
        .await
        .unwrap();
    let tarpc_client =
        ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();
    let tarpc_health = tarpc_client
        .status(tarpc::context::current())
        .await
        .unwrap();

    assert_eq!(jsonrpc_health.name, tarpc_health.name);
    assert_eq!(jsonrpc_health.version, tarpc_health.version);
    assert_eq!(jsonrpc_health.status, tarpc_health.status);
    assert_eq!(jsonrpc_health.supported_archs, tarpc_health.supported_archs);
}

#[tokio::test]
async fn test_graceful_shutdown() {
    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (rpc_addr, rpc_handle) = start_newline_tcp_jsonrpc("127.0.0.1:0", shutdown_rx.clone())
        .await
        .unwrap();
    let (_tarpc_addr, tarpc_handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, shutdown_rx)
        .await
        .unwrap();

    let client = RpcClient::tcp_line(rpc_addr);
    let _health: service::HealthResponse = client
        .request("shader.compile.status", no_params())
        .await
        .unwrap();

    let _: Result<(), _> = shutdown_tx.send(());

    let shutdown_timeout = std::time::Duration::from_secs(5);
    let shutdown_result = tokio::time::timeout(shutdown_timeout, async move {
        rpc_handle.await.ok();
        tarpc_handle.await.ok();
    })
    .await;

    assert!(
        shutdown_result.is_ok(),
        "servers should shut down cleanly within timeout"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_graceful_shutdown_unix() {
    let dir = std::env::temp_dir().join("coralreef-test");
    let _ = std::fs::create_dir_all(&dir);
    let sock_path = dir.join(format!("shutdown-{}.sock", std::process::id()));

    let (shutdown_tx, shutdown_rx) = test_helpers::test_shutdown_channel();
    let (_addr, handle) = start_tarpc_unix_server(&sock_path, shutdown_rx).unwrap();

    let _: Result<(), _> = shutdown_tx.send(());

    let shutdown_timeout = std::time::Duration::from_secs(3);
    let shutdown_result = tokio::time::timeout(shutdown_timeout, handle).await;

    assert!(
        shutdown_result.is_ok(),
        "unix tarpc server should shut down cleanly on signal"
    );

    let _ = std::fs::remove_file(&sock_path);
}

#[tokio::test]
async fn test_bound_addr_display() {
    let tcp = BoundAddr::Tcp("127.0.0.1:8080".parse().unwrap());
    assert_eq!(tcp.to_string(), "127.0.0.1:8080");
    assert_eq!(tcp.protocol(), "tcp");

    #[cfg(unix)]
    {
        let unix = BoundAddr::Unix(std::path::PathBuf::from("/tmp/test.sock"));
        assert_eq!(unix.to_string(), "unix:///tmp/test.sock");
        assert_eq!(unix.protocol(), "unix");
    }
}

#[tokio::test]
async fn test_default_tarpc_bind() {
    let bind = default_tarpc_bind();
    #[cfg(unix)]
    assert!(
        bind.starts_with("unix://"),
        "Unix should default to unix socket"
    );
    #[cfg(not(unix))]
    assert_eq!(bind, FALLBACK_TCP_BIND);
}
