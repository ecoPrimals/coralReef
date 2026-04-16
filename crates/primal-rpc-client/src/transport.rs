// SPDX-License-Identifier: AGPL-3.0-or-later
//! Transport implementations: TCP, Unix socket, delegated TLS via local edge proxy.

use crate::error::RpcError;
use bytes::Bytes;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// How the client reaches the JSON-RPC server.
#[derive(Debug, Clone)]
pub enum Transport {
    /// Plain HTTP over TCP.
    Tcp(SocketAddr),
    /// Newline-delimited JSON-RPC over TCP (wateringHole v3.1 inter-primal framing).
    TcpLine(SocketAddr),
    /// HTTP over a Unix domain socket (primal-to-primal IPC).
    Unix(PathBuf),
    /// Newline-delimited JSON-RPC over a Unix domain socket.
    UnixLine(PathBuf),
    /// HTTPS via a local edge proxy that performs TLS on behalf of this client (Tower Atomic pattern).
    DelegatedTlsProxy {
        /// Local HTTP address of the TLS edge (plain HTTP from this process).
        proxy_addr: SocketAddr,
        /// Upstream hostname the edge uses for the TLS 1.3 connection.
        target_host: String,
    },
}

impl Transport {
    /// Send a JSON-RPC request and return the response body bytes.
    pub(crate) async fn roundtrip(&self, body: &[u8]) -> Result<Bytes, RpcError> {
        match self {
            Self::Tcp(addr) => {
                let host = addr.ip().to_string();
                tcp_roundtrip(*addr, &host, "/", body).await
            }
            Self::TcpLine(addr) => tcp_line_roundtrip(*addr, body).await,
            Self::Unix(path) => unix_roundtrip(path, body).await,
            Self::UnixLine(path) => unix_line_roundtrip(path, body).await,
            Self::DelegatedTlsProxy {
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
) -> Result<Bytes, RpcError> {
    let mut stream = tokio::net::TcpStream::connect(addr).await?;
    send_http_request(&mut stream, host, path, body).await?;
    read_http_response_body(&mut stream).await
}

async fn unix_roundtrip(path: &std::path::Path, body: &[u8]) -> Result<Bytes, RpcError> {
    let mut stream = tokio::net::UnixStream::connect(path).await?;
    let host = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unix");
    send_http_request(&mut stream, host, "/", body).await?;
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
) -> Result<Bytes, RpcError> {
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
    buf.drain(..body_start);
    Ok(Bytes::from(buf))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Newline-delimited JSON-RPC roundtrip over TCP.
async fn tcp_line_roundtrip(addr: SocketAddr, body: &[u8]) -> Result<Bytes, RpcError> {
    let mut stream = tokio::net::TcpStream::connect(addr).await?;
    line_roundtrip(&mut stream, body).await
}

/// Newline-delimited JSON-RPC roundtrip over a Unix domain socket.
#[cfg(unix)]
async fn unix_line_roundtrip(path: &std::path::Path, body: &[u8]) -> Result<Bytes, RpcError> {
    let mut stream = tokio::net::UnixStream::connect(path).await?;
    line_roundtrip(&mut stream, body).await
}

#[cfg(not(unix))]
async fn unix_line_roundtrip(_path: &std::path::Path, _body: &[u8]) -> Result<Bytes, RpcError> {
    Err(RpcError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Unix sockets not available on this platform",
    )))
}

async fn line_roundtrip<S>(stream: &mut S, body: &[u8]) -> Result<Bytes, RpcError>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    stream.write_all(body).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;

    let mut line = String::new();
    tokio::io::AsyncBufReadExt::read_line(&mut tokio::io::BufReader::new(stream), &mut line)
        .await?;
    Ok(Bytes::from(line.into_bytes()))
}
