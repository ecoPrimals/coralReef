// SPDX-License-Identifier: AGPL-3.0-or-later

use super::TransportEndpoint;

/// Connected async byte pipe — the G66 transport abstraction.
///
/// `#[cfg(unix)]` lives here, not in business logic. Protocol negotiation,
/// JSON-RPC dispatch, and tarpc framing all operate on `TransportStream`
/// without knowing the underlying transport.
#[allow(dead_code, reason = "G66 API — wired by unix_jsonrpc accept loop")]
pub enum TransportStream {
    /// Unix domain socket connection (fastest local path, `SO_PEERCRED` available).
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),

    /// TCP connection (cross-host, container, or non-Unix fallback).
    Tcp(tokio::net::TcpStream),
}

impl std::fmt::Debug for TransportStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(unix)]
            Self::Unix(_) => f.write_str("TransportStream::Unix(..)"),
            Self::Tcp(_) => f.write_str("TransportStream::Tcp(..)"),
        }
    }
}

impl TransportStream {
    /// Whether this stream runs over a local (same-host) transport.
    #[must_use]
    #[allow(
        dead_code,
        reason = "G66 API — wired by BTSP local-trust and auth decisions"
    )]
    pub const fn is_local(&self) -> bool {
        match self {
            #[cfg(unix)]
            Self::Unix(_) => true,
            Self::Tcp(_) => false,
        }
    }
}

impl tokio::io::AsyncRead for TransportStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            Self::Tcp(s) => std::pin::Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for TransportStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            Self::Tcp(s) => std::pin::Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(s) => std::pin::Pin::new(s).poll_flush(cx),
            Self::Tcp(s) => std::pin::Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            Self::Tcp(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// Server-side listener abstraction — accepts incoming `TransportStream` connections.
///
/// The accept loop uses this instead of platform-specific listener types.
/// G65 protocol negotiation composes on top: negotiate protocol on any transport.
#[allow(dead_code, reason = "G66 API — wired by unix_jsonrpc accept loop")]
pub enum TransportListener {
    /// Unix domain socket listener.
    #[cfg(unix)]
    Unix(tokio::net::UnixListener),

    /// TCP listener.
    Tcp(tokio::net::TcpListener),
}

impl std::fmt::Debug for TransportListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(unix)]
            Self::Unix(_) => f.write_str("TransportListener::Unix(..)"),
            Self::Tcp(_) => f.write_str("TransportListener::Tcp(..)"),
        }
    }
}

impl TransportListener {
    /// Accept an incoming connection, returning a platform-abstracted stream.
    ///
    /// # Errors
    ///
    /// Returns the underlying OS accept error.
    #[allow(dead_code, reason = "G66 API — wired by accept loop")]
    pub async fn accept(&self) -> std::io::Result<TransportStream> {
        match self {
            #[cfg(unix)]
            Self::Unix(l) => {
                let (stream, _addr) = l.accept().await?;
                Ok(TransportStream::Unix(stream))
            }
            Self::Tcp(l) => {
                let (stream, _addr) = l.accept().await?;
                Ok(TransportStream::Tcp(stream))
            }
        }
    }
}

/// Connect to a `TransportEndpoint`, returning a platform-abstracted stream.
///
/// All `#[cfg(unix)]` conditionals for connection live here — callers
/// operate on `TransportStream` without knowing the transport underneath.
///
/// # Errors
///
/// Returns IO errors from the underlying connect, or `Unsupported` when
/// the endpoint requires a transport unavailable on this platform.
#[allow(
    dead_code,
    reason = "G66 API — wired by BTSP client and ecosystem connect"
)]
pub async fn connect_transport(endpoint: &TransportEndpoint) -> std::io::Result<TransportStream> {
    match endpoint {
        #[cfg(unix)]
        TransportEndpoint::Uds { path } => {
            let stream = tokio::net::UnixStream::connect(path).await?;
            Ok(TransportStream::Unix(stream))
        }
        #[cfg(not(unix))]
        TransportEndpoint::Uds { path } => Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("UDS not available on this platform for {path}"),
        )),
        TransportEndpoint::Tcp { host, port } => {
            let stream = tokio::net::TcpStream::connect(format!("{host}:{port}")).await?;
            Ok(TransportStream::Tcp(stream))
        }
        TransportEndpoint::MeshRelay { .. } => Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "mesh relay transport requires a mesh routing capability provider",
        )),
    }
}

/// Bind a server listener to a `TransportEndpoint`.
///
/// All `#[cfg(unix)]` conditionals for server bind live here — callers
/// operate on `TransportListener` without knowing the platform underneath.
///
/// # Errors
///
/// Returns IO errors from the underlying bind, or `Unsupported` when
/// the endpoint requires a transport unavailable on this platform.
pub fn bind_transport(endpoint: &TransportEndpoint) -> std::io::Result<TransportListener> {
    match endpoint {
        #[cfg(unix)]
        TransportEndpoint::Uds { path } => {
            let listener = tokio::net::UnixListener::bind(path)?;
            Ok(TransportListener::Unix(listener))
        }
        #[cfg(not(unix))]
        TransportEndpoint::Uds { path } => Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("UDS not available on this platform for {path}"),
        )),
        TransportEndpoint::Tcp { host, port } => {
            let addr: std::net::SocketAddr = format!("{host}:{port}")
                .parse()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
            let std_listener = std::net::TcpListener::bind(addr)?;
            std_listener.set_nonblocking(true)?;
            Ok(TransportListener::Tcp(tokio::net::TcpListener::from_std(
                std_listener,
            )?))
        }
        TransportEndpoint::MeshRelay { .. } => Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "mesh relay transport does not support server bind",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connect_transport_rejects_mesh_relay() {
        let ep = TransportEndpoint::MeshRelay {
            peer_id: "east".into(),
            capability: "shader.compile".into(),
        };
        let err = connect_transport(&ep).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
        assert!(
            err.to_string()
                .contains("mesh routing capability provider"),
            "unexpected error message: {err}"
        );
    }

    #[tokio::test]
    async fn connect_transport_tcp_nonexistent_fails() {
        let ep = TransportEndpoint::Tcp {
            host: "127.0.0.1".into(),
            port: 1,
        };
        assert!(connect_transport(&ep).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn connect_transport_uds_nonexistent_fails() {
        let ep = TransportEndpoint::Uds {
            path: "/nonexistent/test.sock".into(),
        };
        assert!(connect_transport(&ep).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn transport_stream_read_write_uds_roundtrip() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let dir = tempfile::tempdir().expect("tempdir");
        let sock = dir.path().join("g66-stream-test.sock");
        let listener = tokio::net::UnixListener::bind(&sock).expect("bind");
        let tl = TransportListener::Unix(listener);

        let connect_path = sock.to_str().unwrap().to_owned();
        let client = tokio::spawn(async move {
            let ep = TransportEndpoint::Uds { path: connect_path };
            let mut stream = connect_transport(&ep).await.unwrap();
            assert!(stream.is_local());
            stream.write_all(b"hello").await.unwrap();
            stream.shutdown().await.unwrap();
        });

        let mut server_stream = tl.accept().await.unwrap();
        assert!(server_stream.is_local());
        let mut buf = Vec::new();
        server_stream.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"hello");

        client.await.unwrap();
    }

    #[tokio::test]
    async fn transport_stream_read_write_tcp_roundtrip() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let tcp_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = tcp_listener.local_addr().unwrap();
        let tl = TransportListener::Tcp(tcp_listener);

        let client = tokio::spawn(async move {
            let ep = TransportEndpoint::Tcp {
                host: addr.ip().to_string(),
                port: addr.port(),
            };
            let mut stream = connect_transport(&ep).await.unwrap();
            assert!(!stream.is_local());
            stream.write_all(b"g66").await.unwrap();
            stream.shutdown().await.unwrap();
        });

        let mut server_stream = tl.accept().await.unwrap();
        assert!(!server_stream.is_local());
        let mut buf = Vec::new();
        server_stream.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"g66");

        client.await.unwrap();
    }

    #[tokio::test]
    async fn transport_listener_debug_format() {
        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tl = TransportListener::Tcp(tcp);
        let s = format!("{tl:?}");
        assert!(s.contains("Tcp"), "debug output should mention Tcp: {s}");
    }
}
