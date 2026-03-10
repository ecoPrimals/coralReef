// SPDX-License-Identifier: AGPL-3.0-only
//! Transport implementations: TCP, Unix socket, Songbird proxy.

use crate::error::RpcError;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// How the client reaches the JSON-RPC server.
#[derive(Debug, Clone)]
pub enum Transport {
    /// Plain HTTP over TCP.
    Tcp(SocketAddr),
    /// HTTP over a Unix domain socket (primal-to-primal IPC).
    Unix(PathBuf),
    /// HTTPS via local Songbird proxy (Tower Atomic pattern).
    SongbirdProxy {
        /// Songbird's local HTTP proxy address.
        proxy_addr: SocketAddr,
        /// The external hostname Songbird will connect to via TLS 1.3.
        target_host: String,
    },
}

impl Transport {
    /// Send `body` as an HTTP POST and return the response body bytes.
    pub(crate) async fn roundtrip(&self, body: &[u8]) -> Result<Vec<u8>, RpcError> {
        match self {
            Self::Tcp(addr) => tcp_roundtrip(*addr, "localhost", "/", body).await,
            Self::Unix(path) => unix_roundtrip(path, body).await,
            Self::SongbirdProxy {
                proxy_addr,
                target_host,
            } => {
                let path = format!("/https/{target_host}");
                tcp_roundtrip(*proxy_addr, target_host, &path, body).await
            }
        }
    }
}

async fn tcp_roundtrip(
    addr: SocketAddr,
    host: &str,
    path: &str,
    body: &[u8],
) -> Result<Vec<u8>, RpcError> {
    let mut stream = tokio::net::TcpStream::connect(addr).await?;
    send_http_request(&mut stream, host, path, body).await?;
    read_http_response_body(&mut stream).await
}

async fn unix_roundtrip(path: &std::path::Path, body: &[u8]) -> Result<Vec<u8>, RpcError> {
    let mut stream = tokio::net::UnixStream::connect(path).await?;
    send_http_request(&mut stream, "localhost", "/", body).await?;
    read_http_response_body(&mut stream).await
}

async fn send_http_request<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    host: &str,
    path: &str,
    body: &[u8],
) -> Result<(), RpcError> {
    let header = format!(
        "POST {path} HTTP/1.1\r\n\
         Host: {host}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n",
        body.len()
    );
    writer.write_all(header.as_bytes()).await?;
    writer.write_all(body).await?;
    Ok(())
}

/// Read a complete HTTP response and extract the body.
///
/// Supports both `Content-Length` and reading until connection close.
async fn read_http_response_body<R: AsyncReadExt + Unpin>(
    reader: &mut R,
) -> Result<Vec<u8>, RpcError> {
    let mut buf = Vec::with_capacity(4096);
    reader.read_to_end(&mut buf).await?;

    let header_end = find_header_end(&buf).ok_or_else(|| {
        RpcError::Http("response missing HTTP header/body separator (\\r\\n\\r\\n)".into())
    })?;

    let status_line_end = buf[..header_end]
        .windows(2)
        .position(|w| w == b"\r\n")
        .unwrap_or(header_end);
    let status_line = String::from_utf8_lossy(&buf[..status_line_end]);

    if !status_line.contains("200") {
        return Err(RpcError::Http(format!("non-200 status: {status_line}")));
    }

    let body_start = header_end + 4; // skip \r\n\r\n
    Ok(buf[body_start..].to_vec())
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}
