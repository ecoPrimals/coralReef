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
async fn test_delegated_tls_proxy_path() {
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

    let client = RpcClient::delegated_tls_proxy(addr, "api.example.com");
    let result: String = client.request("rpc.call", no_params()).await.unwrap();
    assert_eq!(result, "proxied");

    let request = handle.await.unwrap();
    assert!(
        request.contains("POST /https/api.example.com"),
        "expected delegated-TLS edge path, got: {request}"
    );
    assert!(request.contains("Host: api.example.com"));
}

#[test]
fn test_transport_debug() {
    let tcp = crate::Transport::Tcp("127.0.0.1:80".parse().unwrap());
    assert!(format!("{tcp:?}").contains("Tcp"));

    let unix = crate::Transport::Unix("/primal/coralreef".into());
    assert!(format!("{unix:?}").contains("Unix"));

    let proxy = crate::Transport::DelegatedTlsProxy {
        proxy_addr: "127.0.0.1:8080".parse().unwrap(),
        target_host: "example.com".into(),
    };
    assert!(format!("{proxy:?}").contains("DelegatedTlsProxy"));
}

#[test]
fn test_rpc_client_delegated_tls_proxy_construction() {
    let addr = "127.0.0.1:8443".parse().unwrap();
    let client = RpcClient::delegated_tls_proxy(addr, "api.example.com");
    let debug_str = format!("{client:?}");
    assert!(debug_str.contains("DelegatedTlsProxy"));
}

#[test]
fn test_rpc_client_unix_construction() {
    let client = RpcClient::unix("/run/coralreef/rpc.sock");
    let debug_str = format!("{client:?}");
    assert!(debug_str.contains("Unix"));
}

#[test]
fn test_client_clone() {
    let client = RpcClient::tcp("127.0.0.1:9090".parse().unwrap());
    let cloned = client.clone();
    assert!(format!("{client:?}").contains("Tcp"));
    assert!(format!("{cloned:?}").contains("Tcp"));
}

// ---------------------------------------------------------------------------
// Error type tests (primal-rpc-client/src/error.rs coverage)
// ---------------------------------------------------------------------------

#[test]
fn test_rpc_error_data_display() {
    let data = crate::RpcErrorData {
        code: -32600,
        message: "Invalid Request".into(),
        data: None,
    };
    let s = data.to_string();
    assert!(s.contains("-32600"));
    assert!(s.contains("Invalid Request"));
    assert!(s.contains("JSON-RPC error"));
}

#[test]
fn test_rpc_error_data_display_with_data() {
    let data = crate::RpcErrorData {
        code: -32001,
        message: "internal error".into(),
        data: Some(serde_json::json!({"detail": "oom"})),
    };
    let s = data.to_string();
    assert!(s.contains("-32001"));
    assert!(s.contains("internal error"));
}

#[test]
fn test_rpc_error_from_io() {
    let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "connection refused");
    let rpc_err: RpcError = io_err.into();
    let s = rpc_err.to_string();
    assert!(s.contains("transport I/O"));
    assert!(s.contains("connection refused"));
}

#[test]
fn test_rpc_error_from_serde_json() {
    let bad_json = serde_json::from_str::<serde_json::Value>("{ invalid }");
    let json_err = bad_json.unwrap_err();
    let rpc_err: RpcError = json_err.into();
    let s = rpc_err.to_string();
    assert!(s.contains("json"));
}

// ---------------------------------------------------------------------------
// Transport error handling (primal-rpc-client/src/transport.rs coverage)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_http_non_200_status() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let _ = stream.read(&mut buf).await.unwrap();
        let http_response =
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        stream.write_all(http_response.as_bytes()).await.unwrap();
    });

    let client = RpcClient::tcp(addr);
    let result: Result<String, _> = client.request("test.method", no_params()).await;
    assert!(matches!(result, Err(RpcError::Http(_))));
    if let Err(RpcError::Http(msg)) = result {
        assert!(msg.to_lowercase().contains("404") || msg.to_lowercase().contains("non-200"));
    }

    let _ = handle.await;
}

#[tokio::test]
async fn test_http_500_status() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let _ = stream.read(&mut buf).await.unwrap();
        let http_response =
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        stream.write_all(http_response.as_bytes()).await.unwrap();
    });

    let client = RpcClient::tcp(addr);
    let result: Result<String, _> = client.request("test.method", no_params()).await;
    assert!(matches!(result, Err(RpcError::Http(_))));
    if let Err(RpcError::Http(msg)) = result {
        assert!(msg.to_lowercase().contains("500") || msg.to_lowercase().contains("non-200"));
    }

    let _ = handle.await;
}

#[tokio::test]
async fn test_http_missing_header_separator() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let _ = stream.read(&mut buf).await.unwrap();
        let http_response = "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nno-double-crlf-here";
        stream.write_all(http_response.as_bytes()).await.unwrap();
    });

    let client = RpcClient::tcp(addr);
    let result: Result<String, _> = client.request("test.method", no_params()).await;
    assert!(matches!(result, Err(RpcError::Http(_))));
    if let Err(RpcError::Http(msg)) = result {
        assert!(
            msg.to_lowercase().contains("separator") || msg.to_lowercase().contains("\\r\\n\\r\\n"),
            "expected header separator error: {msg}"
        );
    }

    let _ = handle.await;
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_socket_connection_refused() {
    let client = RpcClient::unix("/nonexistent/path/coralreef.sock");
    let result: Result<String, _> = client.request("test.method", no_params()).await;
    assert!(matches!(result, Err(RpcError::Io(_))));
}

#[tokio::test]
async fn test_delegated_tls_proxy_custom_path() {
    // Local TLS edge uses /https/{target_host} path format
    let resp = r#"{"jsonrpc":"2.0","result":"ok","id":1}"#;
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

    let client = RpcClient::delegated_tls_proxy(addr, "rpc.internal.example.org");
    let result: String = client.request("method", no_params()).await.unwrap();
    assert_eq!(result, "ok");

    let request = handle.await.unwrap();
    assert!(
        request.contains("POST /https/rpc.internal.example.org"),
        "delegated-TLS path should be /https/{{host}}: {request}"
    );
}

#[tokio::test]
async fn test_http_response_body_extraction() {
    #[derive(serde::Deserialize)]
    struct NestedResult {
        nested: String,
    }

    let body = r#"{"jsonrpc":"2.0","result":{"nested":"value"},"id":1}"#;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let _ = stream.read(&mut buf).await.unwrap();
        let http_response = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n\
             {}",
            body.len(),
            body
        );
        stream.write_all(http_response.as_bytes()).await.unwrap();
    });

    let client = RpcClient::tcp(addr);
    let result: NestedResult = client.request("test", no_params()).await.unwrap();
    assert_eq!(result.nested, "value");

    let _ = handle.await;
}

#[tokio::test]
async fn test_http_empty_body_200() {
    // 200 with Content-Length: 0 - body should be empty bytes
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let _ = stream.read(&mut buf).await.unwrap();
        let http_response = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        stream.write_all(http_response.as_bytes()).await.unwrap();
    });

    let client = RpcClient::tcp(addr);
    // Empty body will fail JSON parse - we get EmptyResponse or Json error
    let result: Result<String, _> = client.request("test", no_params()).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, RpcError::EmptyResponse | RpcError::Json(_)),
        "empty body should produce EmptyResponse or Json error: {err:?}"
    );

    let _ = handle.await;
}

#[tokio::test]
async fn test_http_status_line_parsing() {
    // Status line without \r\n before headers - edge case (malformed but we handle it)
    let resp = r#"{"jsonrpc":"2.0","result":"ok","id":1}"#;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let _ = stream.read(&mut buf).await.unwrap();
        // Status line with \r\n, then headers, then \r\n\r\n, then body
        let http_response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            resp.len(),
            resp
        );
        stream.write_all(http_response.as_bytes()).await.unwrap();
    });

    let client = RpcClient::tcp(addr);
    let result: String = client.request("test", no_params()).await.unwrap();
    assert_eq!(result, "ok");

    let _ = handle.await;
}

#[test]
fn test_rpc_error_variants_display_substrings() {
    let io_err = RpcError::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout"));
    assert!(io_err.to_string().contains("transport I/O"));
    assert!(io_err.to_string().contains("timeout"));

    let json_err = RpcError::Json(serde_json::from_str::<serde_json::Value>("{").unwrap_err());
    assert!(json_err.to_string().contains("json"));

    let http_err = RpcError::Http("bad status 500".into());
    assert!(http_err.to_string().contains("http"));
    assert!(http_err.to_string().contains("bad status 500"));

    let server_err = RpcError::Server(crate::RpcErrorData {
        code: -32603,
        message: "Internal error".into(),
        data: None,
    });
    assert!(server_err.to_string().contains("-32603"));
    assert!(server_err.to_string().contains("Internal error"));

    let empty_err = RpcError::EmptyResponse;
    assert!(empty_err.to_string().contains("empty response"));
}
