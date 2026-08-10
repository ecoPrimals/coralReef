// SPDX-License-Identifier: AGPL-3.0-or-later

use super::TransportEndpoint;

/// Synchronous connected byte pipe — the G66 transport abstraction for
/// blocking I/O paths (provenance signing, BTSP client handshake).
///
/// `#[cfg(unix)]` lives here, not in business logic.
pub enum SyncTransportStream {
    /// Unix domain socket connection.
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixStream),

    /// TCP connection.
    Tcp(std::net::TcpStream),
}

impl std::fmt::Debug for SyncTransportStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(unix)]
            Self::Unix(_) => f.write_str("SyncTransportStream::Unix(..)"),
            Self::Tcp(_) => f.write_str("SyncTransportStream::Tcp(..)"),
        }
    }
}

impl SyncTransportStream {
    /// Set the read timeout on the underlying stream.
    ///
    /// # Errors
    ///
    /// Returns an IO error if the underlying OS call fails.
    pub fn set_read_timeout(&self, dur: Option<std::time::Duration>) -> std::io::Result<()> {
        match self {
            #[cfg(unix)]
            Self::Unix(s) => s.set_read_timeout(dur),
            Self::Tcp(s) => s.set_read_timeout(dur),
        }
    }

    /// Set the write timeout on the underlying stream.
    ///
    /// # Errors
    ///
    /// Returns an IO error if the underlying OS call fails.
    pub fn set_write_timeout(&self, dur: Option<std::time::Duration>) -> std::io::Result<()> {
        match self {
            #[cfg(unix)]
            Self::Unix(s) => s.set_write_timeout(dur),
            Self::Tcp(s) => s.set_write_timeout(dur),
        }
    }
}

impl std::io::Read for SyncTransportStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            #[cfg(unix)]
            Self::Unix(s) => s.read(buf),
            Self::Tcp(s) => s.read(buf),
        }
    }
}

impl std::io::Write for SyncTransportStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            #[cfg(unix)]
            Self::Unix(s) => s.write(buf),
            Self::Tcp(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            #[cfg(unix)]
            Self::Unix(s) => s.flush(),
            Self::Tcp(s) => s.flush(),
        }
    }
}

impl std::io::Read for &SyncTransportStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            #[cfg(unix)]
            SyncTransportStream::Unix(s) => {
                let mut r: &std::os::unix::net::UnixStream = s;
                r.read(buf)
            }
            SyncTransportStream::Tcp(s) => {
                let mut r: &std::net::TcpStream = s;
                r.read(buf)
            }
        }
    }
}

impl std::io::Write for &SyncTransportStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            #[cfg(unix)]
            SyncTransportStream::Unix(s) => {
                let mut w: &std::os::unix::net::UnixStream = s;
                w.write(buf)
            }
            SyncTransportStream::Tcp(s) => {
                let mut w: &std::net::TcpStream = s;
                w.write(buf)
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            #[cfg(unix)]
            SyncTransportStream::Unix(s) => {
                let mut w: &std::os::unix::net::UnixStream = s;
                w.flush()
            }
            SyncTransportStream::Tcp(s) => {
                let mut w: &std::net::TcpStream = s;
                w.flush()
            }
        }
    }
}

/// Synchronous connect to a `TransportEndpoint`.
///
/// All `#[cfg(unix)]` conditionals for sync connection live here.
///
/// # Errors
///
/// Returns IO errors from the underlying connect, or `Unsupported` when
/// the endpoint requires a transport unavailable on this platform.
pub fn connect_transport_sync(
    endpoint: &TransportEndpoint,
) -> std::io::Result<SyncTransportStream> {
    match endpoint {
        #[cfg(unix)]
        TransportEndpoint::Uds { path } => {
            let stream = std::os::unix::net::UnixStream::connect(path)?;
            Ok(SyncTransportStream::Unix(stream))
        }
        #[cfg(not(unix))]
        TransportEndpoint::Uds { path } => Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("UDS not available on this platform for {path}"),
        )),
        TransportEndpoint::Tcp { host, port } => {
            let stream = std::net::TcpStream::connect(format!("{host}:{port}"))?;
            Ok(SyncTransportStream::Tcp(stream))
        }
        TransportEndpoint::MeshRelay { .. } => Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "mesh relay transport requires async runtime",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_transport_sync_rejects_mesh_relay() {
        let ep = TransportEndpoint::MeshRelay {
            peer_id: "east".into(),
            capability: "shader.compile".into(),
        };
        let err = connect_transport_sync(&ep).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    }

    #[test]
    fn connect_transport_sync_tcp_nonexistent_fails() {
        let ep = TransportEndpoint::Tcp {
            host: "127.0.0.1".into(),
            port: 1,
        };
        assert!(connect_transport_sync(&ep).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn connect_transport_sync_uds_nonexistent_fails() {
        let ep = TransportEndpoint::Uds {
            path: "/nonexistent/test.sock".into(),
        };
        assert!(connect_transport_sync(&ep).is_err());
    }

    #[test]
    fn sync_transport_stream_debug_format() {
        let tcp = std::net::TcpStream::connect("127.0.0.1:1");
        if let Ok(stream) = tcp {
            let sts = SyncTransportStream::Tcp(stream);
            let s = format!("{sts:?}");
            assert!(s.contains("Tcp"), "debug output should mention Tcp: {s}");
        }
    }
}
