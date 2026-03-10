// SPDX-License-Identifier: AGPL-3.0-only
//! Unit and integration tests for the JSON-RPC client.

use crate::{RpcClient, RpcError, no_params};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct EchoResult {
    echo: String,
}

async fn spawn_mock_server(
    response_body: &str,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let resp = response_body.to_string();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).await.unwrap();
        let request = String::from_utf8_lossy(&buf[..n]).to_string();

        let http_response = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n\
             {}",
            resp.len(),
            resp
        );
        stream.write_all(http_response.as_bytes()).await.unwrap();
        request
    });

    (addr, handle)
}

#[tokio::test]
async fn test_tcp_request_success() {
    let resp = r#"{"jsonrpc":"2.0","result":"ok","id":1}"#;
    let (addr, handle) = spawn_mock_server(resp).await;

    let client = RpcClient::tcp(addr);
    let result: String = client.request("test.method", no_params()).await.unwrap();
    assert_eq!(result, "ok");

    let request = handle.await.unwrap();
    assert!(request.contains("POST / HTTP/1.1"));
    assert!(request.contains("test.method"));
}

#[tokio::test]
async fn test_tcp_request_server_error() {
    let resp = r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Invalid Request"},"id":1}"#;
    let (addr, _handle) = spawn_mock_server(resp).await;

    let client = RpcClient::tcp(addr);
    let result: Result<String, _> = client.request("bad.method", no_params()).await;

    match result {
        Err(RpcError::Server(e)) => {
            assert_eq!(e.code, -32600);
            assert_eq!(e.message, "Invalid Request");
        }
        other => panic!("expected Server error, got {other:?}"),
    }
}

#[tokio::test]
async fn test_tcp_connection_refused() {
    let client = RpcClient::tcp("127.0.0.1:1".parse().unwrap());
    let result: Result<String, _> = client.request("test", no_params()).await;
    assert!(matches!(result, Err(RpcError::Io(_))));
}

#[tokio::test]
async fn test_empty_response_error() {
    let resp = r#"{"jsonrpc":"2.0","id":1}"#;
    let (addr, _handle) = spawn_mock_server(resp).await;

    let client = RpcClient::tcp(addr);
    let result: Result<String, _> = client.request("test", no_params()).await;
    assert!(matches!(result, Err(RpcError::EmptyResponse)));
}

#[tokio::test]
async fn test_structured_params() {
    #[derive(Serialize)]
    struct Params {
        source: String,
        target: String,
    }

    let resp = r#"{"jsonrpc":"2.0","result":{"echo":"hello"},"id":1}"#;
    let (addr, handle) = spawn_mock_server(resp).await;

    let client = RpcClient::tcp(addr);
    let params = Params {
        source: "test.wgsl".into(),
        target: "sm_70".into(),
    };
    let result: EchoResult = client.request("shader.compile.wgsl", params).await.unwrap();
    assert_eq!(
        result,
        EchoResult {
            echo: "hello".into()
        }
    );

    let request = handle.await.unwrap();
    assert!(request.contains("shader.compile.wgsl"));
    assert!(request.contains("test.wgsl"));
}

#[tokio::test]
async fn test_notify() {
    let resp = r#"{"jsonrpc":"2.0","result":null,"id":null}"#;
    let (addr, handle) = spawn_mock_server(resp).await;

    let client = RpcClient::tcp(addr);
    client.notify("system.shutdown", no_params()).await.unwrap();

    let request = handle.await.unwrap();
    assert!(request.contains("system.shutdown"));
    assert!(!request.contains(r#""id""#));
}

#[tokio::test]
async fn test_error_display_formats() {
    let io_err = RpcError::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout"));
    assert!(io_err.to_string().contains("timeout"));

    let http_err = RpcError::Http("bad status".into());
    assert!(http_err.to_string().contains("bad status"));

    let empty_err = RpcError::EmptyResponse;
    assert!(empty_err.to_string().contains("empty response"));

    let server_err = RpcError::Server(crate::RpcErrorData {
        code: -32001,
        message: "internal".into(),
        data: None,
    });
    assert!(server_err.to_string().contains("-32001"));
    assert!(server_err.to_string().contains("internal"));
}

#[tokio::test]
async fn test_songbird_proxy_path() {
    let resp = r#"{"jsonrpc":"2.0","result":"proxied","id":1}"#;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).await.unwrap();
        let request = String::from_utf8_lossy(&buf[..n]).to_string();

        let http_response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            resp.len(),
            resp
        );
        stream.write_all(http_response.as_bytes()).await.unwrap();
        request
    });

    let client = RpcClient::songbird_proxy(addr, "api.example.com");
    let result: String = client.request("rpc.call", no_params()).await.unwrap();
    assert_eq!(result, "proxied");

    let request = handle.await.unwrap();
    assert!(
        request.contains("POST /https/api.example.com"),
        "expected Songbird proxy path, got: {request}"
    );
    assert!(request.contains("Host: api.example.com"));
}

#[test]
fn test_transport_debug() {
    let tcp = crate::Transport::Tcp("127.0.0.1:80".parse().unwrap());
    assert!(format!("{tcp:?}").contains("Tcp"));

    let unix = crate::Transport::Unix("/primal/coralreef".into());
    assert!(format!("{unix:?}").contains("Unix"));

    let proxy = crate::Transport::SongbirdProxy {
        proxy_addr: "127.0.0.1:8080".parse().unwrap(),
        target_host: "example.com".into(),
    };
    assert!(format!("{proxy:?}").contains("SongbirdProxy"));
}

#[test]
fn test_client_clone() {
    let client = RpcClient::tcp("127.0.0.1:9090".parse().unwrap());
    let cloned = client.clone();
    assert!(format!("{cloned:?}").contains("Tcp"));
}
