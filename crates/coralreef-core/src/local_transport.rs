// SPDX-License-Identifier: AGPL-3.0-or-later
//! Platform-agnostic local transport for primal-to-primal IPC.
//!
//! Silicon Atheism Phase 2: abstraction over gating. Callers use
//! [`connect_local`] / [`connect_local_sync`] / [`is_local_socket_alive`]
//! instead of raw `UnixStream::connect()` — platform dispatch is centralized
//! here. When Windows named-pipe or Android binder backends are added, only
//! this module changes.
//!
//! On Unix: connects via Unix domain socket.
//! On non-Unix: returns [`std::io::ErrorKind::Unsupported`].

use std::path::Path;

/// Async-connect to a local socket path.
///
/// Unix: `tokio::net::UnixStream::connect`.
/// Non-Unix: returns [`std::io::ErrorKind::Unsupported`].
///
/// # Errors
///
/// Returns an IO error if the socket does not exist, connection is refused,
/// or (on non-Unix) local sockets are unsupported.
#[cfg(unix)]
pub async fn connect_local(
    path: &Path,
) -> std::io::Result<tokio::net::UnixStream> {
    tokio::net::UnixStream::connect(path).await
}

/// Async-connect to a local socket path (non-Unix stub).
///
/// # Errors
///
/// Always returns [`std::io::ErrorKind::Unsupported`] on non-Unix platforms.
#[cfg(not(unix))]
pub async fn connect_local(
    path: &Path,
) -> std::io::Result<tokio::net::TcpStream> {
    let _ = path;
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "local socket connections not available on this platform",
    ))
}

/// Sync-connect to a local socket path.
///
/// Used by provenance signing where async is not available.
/// Unix: `std::os::unix::net::UnixStream::connect`.
/// Non-Unix: returns [`std::io::ErrorKind::Unsupported`].
///
/// # Errors
///
/// Returns an IO error if the socket does not exist, connection is refused,
/// or (on non-Unix) local sockets are unsupported.
#[cfg(unix)]
pub fn connect_local_sync(
    path: &Path,
) -> std::io::Result<std::os::unix::net::UnixStream> {
    std::os::unix::net::UnixStream::connect(path)
}

/// Sync-connect to a local socket path (non-Unix stub).
///
/// # Errors
///
/// Always returns [`std::io::ErrorKind::Unsupported`] on non-Unix platforms.
#[cfg(not(unix))]
pub fn connect_local_sync(
    path: &Path,
) -> std::io::Result<std::net::TcpStream> {
    let _ = path;
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "local socket connections not available on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_local_sync_nonexistent_fails() {
        let r = connect_local_sync(Path::new("/nonexistent/coralreef-test.sock"));
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn connect_local_nonexistent_fails() {
        let r = connect_local(Path::new("/nonexistent/coralreef-test.sock")).await;
        assert!(r.is_err());
    }
}
