// SPDX-License-Identifier: AGPL-3.0-or-later
//! Platform-agnostic local transport for primal-to-primal IPC.
//!
//! Silicon Atheism Phase 2: abstraction over gating. All local socket
//! operations — client connect, server bind, symlink discovery — are
//! centralized here. When Windows named-pipe or Android binder backends
//! are added, only this module changes.
//!
//! On Unix: connects/binds via Unix domain socket.
//! On non-Unix: returns [`std::io::ErrorKind::Unsupported`].

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Client: connect
// ---------------------------------------------------------------------------

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
    Err(unsupported("local socket connections"))
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
    Err(unsupported("local socket connections"))
}

// ---------------------------------------------------------------------------
// Server: bind
// ---------------------------------------------------------------------------

/// Prepare a local socket path for binding: create parent dirs, remove stale.
///
/// # Errors
///
/// Returns an IO error if parent directory creation or stale socket removal
/// fails.
pub fn prepare_local_bind(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Bind a local (Unix domain) socket listener.
///
/// Caller should call [`prepare_local_bind`] first. On Unix this creates a
/// `tokio::net::UnixListener`; on non-Unix it returns `Unsupported`.
///
/// # Errors
///
/// Returns an IO error if the socket cannot be bound or (on non-Unix) local
/// sockets are unsupported.
#[cfg(unix)]
pub fn bind_local(path: &Path) -> std::io::Result<tokio::net::UnixListener> {
    tokio::net::UnixListener::bind(path)
}

/// Bind a local socket listener (non-Unix stub).
///
/// # Errors
///
/// Always returns [`std::io::ErrorKind::Unsupported`] on non-Unix platforms.
#[cfg(not(unix))]
pub fn bind_local(path: &Path) -> std::io::Result<std::net::TcpListener> {
    let _ = path;
    Err(unsupported("local socket server"))
}

// ---------------------------------------------------------------------------
// Discovery: capability-domain symlink
// ---------------------------------------------------------------------------

/// `true` when the bound socket path uses the shared ecosystem directory.
#[must_use]
pub fn path_in_ecosystem_namespace(socket_path: &Path) -> bool {
    socket_path
        .iter()
        .any(|c| c == std::ffi::OsStr::new(crate::config::ecosystem_namespace()))
}

/// After a successful bind, install `{domain}.sock` → instance socket (relative symlink).
///
/// Returns the symlink path when created, for shutdown cleanup. Skipped when
/// the socket is not under the ecosystem layout or when symlink creation fails.
#[must_use]
pub fn install_capability_symlink(bound_path: &Path) -> Option<PathBuf> {
    if !path_in_ecosystem_namespace(bound_path) {
        return None;
    }
    let parent = bound_path.parent()?;
    let link = parent.join(crate::config::capability_domain_socket_filename());
    if link.as_path() == bound_path {
        return None;
    }
    let target_name = bound_path.file_name()?;
    if link.exists() {
        let _ = std::fs::remove_file(&link);
    }
    #[cfg(unix)]
    match std::os::unix::fs::symlink(target_name, &link) {
        Ok(()) => return Some(link),
        Err(e) => {
            tracing::warn!(
                error = %e,
                link = %link.display(),
                target = %target_name.to_string_lossy(),
                "failed to create capability-domain symlink (non-fatal)"
            );
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (target_name, &link);
        tracing::debug!("capability-domain symlink skipped on non-Unix platform");
    }
    None
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[cfg(not(unix))]
fn unsupported(what: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!("{what} not available on this platform"),
    )
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

    #[test]
    fn prepare_local_bind_creates_parent_and_removes_stale() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock = dir.path().join("sub").join("test.sock");
        std::fs::create_dir_all(sock.parent().unwrap()).expect("mkdir");
        std::fs::write(&sock, "stale").expect("write");
        assert!(sock.exists());
        prepare_local_bind(&sock).expect("prepare");
        assert!(!sock.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bind_local_to_tempdir_succeeds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock = dir.path().join("bind-test.sock");
        let listener = bind_local(&sock);
        assert!(listener.is_ok(), "bind_local should succeed: {listener:?}");
    }

    #[test]
    fn path_in_ecosystem_namespace_detects_namespace() {
        let ns = crate::config::ecosystem_namespace();
        let p = PathBuf::from(format!("/run/user/1000/{ns}/coralreef.sock"));
        assert!(path_in_ecosystem_namespace(&p));
    }

    #[test]
    fn path_in_ecosystem_namespace_rejects_other() {
        let p = PathBuf::from("/tmp/test/coralreef.sock");
        assert!(!path_in_ecosystem_namespace(&p));
    }

    #[cfg(unix)]
    #[test]
    fn install_capability_symlink_creates_link() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ns = crate::config::ecosystem_namespace();
        let ns_dir = dir.path().join(ns);
        std::fs::create_dir_all(&ns_dir).expect("mkdir");
        let socket = ns_dir.join("coralreef-default.sock");
        std::fs::write(&socket, "").expect("create socket");

        let link = install_capability_symlink(&socket);
        assert!(link.is_some(), "symlink should be created");
        let link_path = link.unwrap();
        assert!(link_path.is_symlink(), "should be a symlink");
        let target = std::fs::read_link(&link_path).expect("read_link");
        assert_eq!(target, Path::new("coralreef-default.sock"));
    }

    #[test]
    fn install_capability_symlink_skips_non_ecosystem_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("coralreef.sock");
        std::fs::write(&socket, "").expect("create");
        let link = install_capability_symlink(&socket);
        assert!(link.is_none(), "should skip non-ecosystem paths");
    }

    #[cfg(unix)]
    #[test]
    fn install_capability_symlink_replaces_existing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ns = crate::config::ecosystem_namespace();
        let ns_dir = dir.path().join(ns);
        std::fs::create_dir_all(&ns_dir).expect("mkdir");

        let socket1 = ns_dir.join("first-instance.sock");
        std::fs::write(&socket1, "").expect("create1");
        let link1 = install_capability_symlink(&socket1);
        assert!(link1.is_some());

        let socket2 = ns_dir.join("second-instance.sock");
        std::fs::write(&socket2, "").expect("create2");
        let link2 = install_capability_symlink(&socket2);
        assert!(link2.is_some());
        let target = std::fs::read_link(link2.unwrap()).expect("read_link");
        assert_eq!(target, Path::new("second-instance.sock"));
    }
}
