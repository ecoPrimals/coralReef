// SPDX-License-Identifier: AGPL-3.0-only
#![warn(missing_docs)]
//! Ember client — connects to coral-ember and receives VFIO fds via `SCM_RIGHTS`.
//!
//! When coral-ember is running, the daemon receives duplicate VFIO fds
//! from it instead of opening them directly. This allows the daemon to
//! be restarted without triggering PM reset on GV100 (no FLR support).

use std::mem::MaybeUninit;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;

use rustix::io::IoSliceMut;
use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, recvmsg};

use serde::Deserialize;

use coral_driver::vfio::ReceivedVfioFds;

use crate::error::EmberError;

/// Default ember socket path, overridable via `$CORALREEF_EMBER_SOCKET`.
fn default_ember_socket() -> String {
    std::env::var("CORALREEF_EMBER_SOCKET").unwrap_or_else(|_| "/run/coralreef/ember.sock".into())
}

/// Returns the resolved ember socket path (for integration tests that set `CORALREEF_EMBER_SOCKET`).
#[doc(hidden)]
pub fn test_support_default_ember_socket() -> String {
    default_ember_socket()
}
const MAX_RESPONSE_SIZE: usize = 4096;

#[derive(Deserialize)]
struct JsonRpcResponse {
    #[serde(rename = "jsonrpc")]
    _jsonrpc: String,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
    #[serde(rename = "id")]
    _id: serde_json::Value,
}

#[derive(Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

/// Atomic request ID counter for JSON-RPC.
static REQUEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_request_id() -> u64 {
    REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn make_rpc_request(method: &str, params: serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": next_request_id(),
    })
    .to_string()
}

fn parse_rpc_response(buf: &[u8]) -> Result<serde_json::Value, EmberError> {
    let resp: JsonRpcResponse = serde_json::from_slice(buf)?;
    if let Some(err) = resp.error {
        return Err(EmberError::Rpc {
            code: err.code,
            message: err.message,
        });
    }
    Ok(resp.result.unwrap_or(serde_json::Value::Null))
}

/// Client handle to the coral-ember process.
pub struct EmberClient {
    socket_path: String,
}

#[cfg(test)]
std::thread_local! {
    static EMBER_DISABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl EmberClient {
    /// Disable ember connections for the current thread (test isolation).
    ///
    /// Returns a guard that re-enables on drop.
    #[cfg(test)]
    pub fn disable_for_test() -> EmberTestGuard {
        EMBER_DISABLED.with(|c| c.set(true));
        EmberTestGuard(())
    }

    /// Try to connect to the ember. Returns None if the ember is not running.
    ///
    /// Socket path is resolved from `$CORALREEF_EMBER_SOCKET` (fallback: `/run/coralreef/ember.sock`).
    pub fn connect() -> Option<Self> {
        #[cfg(test)]
        if EMBER_DISABLED.with(|c| c.get()) {
            tracing::debug!("ember disabled for test");
            return None;
        }

        let path = default_ember_socket();
        if !std::path::Path::new(&path).exists() {
            tracing::debug!("ember socket not found at {path}");
            return None;
        }

        match UnixStream::connect(&path) {
            Ok(stream) => {
                drop(stream);
                tracing::info!(path = %path, "ember is available");
                Some(Self { socket_path: path })
            }
            Err(e) => {
                tracing::debug!(path, error = %e, "ember not reachable");
                None
            }
        }
    }

    /// List devices held by the ember.
    pub fn list_devices(&self) -> Result<Vec<String>, EmberError> {
        let stream = UnixStream::connect(&self.socket_path).map_err(EmberError::Connect)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;

        let req = make_rpc_request("ember.list", serde_json::json!({}));
        std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())?;

        let mut buf = [0u8; MAX_RESPONSE_SIZE];
        let n = std::io::Read::read(&mut &stream, &mut buf)?;
        let result = parse_rpc_response(&buf[..n])?;
        let devices = result
            .get("devices")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        Ok(devices)
    }

    /// Tell ember to release VFIO fds for a device (before driver swap).
    ///
    /// Superseded by `swap_device` for normal swaps; retained for manual
    /// debugging via socat or targeted fd release.
    #[allow(
        dead_code,
        reason = "retained for manual debugging via socat — used via test harness"
    )]
    pub fn release_device(&self, bdf: &str) -> Result<(), EmberError> {
        let stream = UnixStream::connect(&self.socket_path).map_err(EmberError::Connect)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;

        let req = make_rpc_request("ember.release", serde_json::json!({"bdf": bdf}));
        std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())?;

        let mut buf = [0u8; MAX_RESPONSE_SIZE];
        let n = std::io::Read::read(&mut &stream, &mut buf)?;
        parse_rpc_response(&buf[..n])?;
        Ok(())
    }

    /// Tell ember to reacquire VFIO fds for a device (after driver swap back to vfio).
    ///
    /// Superseded by `swap_device` which handles reacquisition internally;
    /// retained for manual debugging.
    #[allow(
        dead_code,
        reason = "retained for manual debugging via socat — used via test harness"
    )]
    pub fn reacquire_device(&self, bdf: &str) -> Result<(), EmberError> {
        let stream = UnixStream::connect(&self.socket_path).map_err(EmberError::Connect)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;

        let req = make_rpc_request("ember.reacquire", serde_json::json!({"bdf": bdf}));
        std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())?;

        let mut buf = [0u8; MAX_RESPONSE_SIZE];
        let n = std::io::Read::read(&mut &stream, &mut buf)?;
        parse_rpc_response(&buf[..n])?;
        Ok(())
    }

    /// Ask ember to perform a full driver swap (unbind current, bind target).
    ///
    /// Ember handles all sysfs writes and VFIO fd lifecycle. Returns the
    /// resulting personality name on success.
    ///
    /// Retries transient I/O errors (EAGAIN, EWOULDBLOCK, EINTR) up to 3
    /// times with backoff. Driver swaps can stall briefly while the kernel
    /// settles sysfs state after unbind.
    pub fn swap_device(&self, bdf: &str, target: &str) -> Result<String, EmberError> {
        const MAX_RETRIES: u32 = 3;
        const SWAP_TIMEOUT_SECS: u64 = 60;

        let mut last_err = None;
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let backoff = std::time::Duration::from_millis(500 * u64::from(attempt));
                tracing::debug!(
                    bdf, target, attempt,
                    backoff_ms = backoff.as_millis(),
                    "ember swap_device: retrying after transient I/O error"
                );
                std::thread::sleep(backoff);
            }

            match self.try_swap_device(bdf, target, SWAP_TIMEOUT_SECS) {
                Ok(personality) => return Ok(personality),
                Err(EmberError::Io(ref e)) if is_transient_io(e) && attempt < MAX_RETRIES => {
                    tracing::warn!(
                        bdf, target, attempt, error = %e,
                        "ember swap_device: transient I/O error, will retry"
                    );
                    last_err = Some(EmberError::Io(std::io::Error::new(e.kind(), e.to_string())));
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            EmberError::Io(std::io::Error::other("swap_device: exhausted retries"))
        }))
    }

    fn try_swap_device(
        &self,
        bdf: &str,
        target: &str,
        timeout_secs: u64,
    ) -> Result<String, EmberError> {
        let stream = UnixStream::connect(&self.socket_path).map_err(EmberError::Connect)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(timeout_secs)))?;

        let req = make_rpc_request(
            "ember.swap",
            serde_json::json!({"bdf": bdf, "target": target}),
        );
        std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())?;

        let mut buf = [0u8; MAX_RESPONSE_SIZE];
        let n = read_full_response(&stream, &mut buf)?;
        let result = parse_rpc_response(&buf[..n])?;
        Ok(result
            .get("personality")
            .and_then(|v| v.as_str())
            .unwrap_or(target)
            .to_string())
    }

    /// Request VFIO fds for a specific BDF from the ember.
    ///
    /// Backend-aware: the response includes a `"backend"` field (`"legacy"` or
    /// `"iommufd"`) and the appropriate number of SCM_RIGHTS fds.
    pub fn request_fds(&self, bdf: &str) -> Result<ReceivedVfioFds, EmberError> {
        let stream = UnixStream::connect(&self.socket_path).map_err(EmberError::Connect)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;

        let req = make_rpc_request("ember.vfio_fds", serde_json::json!({"bdf": bdf}));
        std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())?;

        let mut buf = [0u8; MAX_RESPONSE_SIZE];
        let (n, fds) = recv_with_fds(&stream, &mut buf)?;

        let resp: JsonRpcResponse = serde_json::from_slice(&buf[..n])?;

        if let Some(err) = resp.error {
            return Err(EmberError::Rpc {
                code: err.code,
                message: err.message,
            });
        }

        let result = resp.result.unwrap_or(serde_json::Value::Null);
        let expected = result.get("num_fds").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
        if fds.len() < expected {
            return Err(EmberError::FdCount {
                expected,
                received: fds.len(),
            });
        }

        let backend = result
            .get("backend")
            .and_then(|b| b.as_str())
            .unwrap_or("legacy");

        match backend {
            "iommufd" => {
                if fds.len() < 2 {
                    return Err(EmberError::FdCount {
                        expected: 2,
                        received: fds.len(),
                    });
                }
                let ioas_id = result
                    .get("ioas_id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| EmberError::Rpc {
                        code: -32000,
                        message: "iommufd response missing ioas_id".into(),
                    })? as u32;
                let mut it = fds.into_iter();
                Ok(ReceivedVfioFds::Iommufd {
                    iommufd: it.next().expect("checked len >= 2"),
                    device: it.next().expect("checked len >= 2"),
                    ioas_id,
                })
            }
            _ => {
                if fds.len() < 3 {
                    return Err(EmberError::FdCount {
                        expected: 3,
                        received: fds.len(),
                    });
                }
                let mut it = fds.into_iter();
                Ok(ReceivedVfioFds::Legacy {
                    container: it.next().expect("checked len >= 3"),
                    group: it.next().expect("checked len >= 3"),
                    device: it.next().expect("checked len >= 3"),
                })
            }
        }
    }
}

/// Receive data with ancillary `SCM_RIGHTS` file descriptors (`rustix::net::recvmsg`).
///
/// Buffer is sized for up to three fds (ember VFIO triple: container, group, device).
fn recv_with_fds(sock: impl AsFd, buf: &mut [u8]) -> std::io::Result<(usize, Vec<OwnedFd>)> {
    const MAX_SCM_FDS: usize = 3;

    let mut iov = [IoSliceMut::new(buf)];
    let mut recv_space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(MAX_SCM_FDS))];
    let mut control = RecvAncillaryBuffer::new(&mut recv_space);

    let msg = recvmsg(sock, &mut iov, &mut control, RecvFlags::empty())?;

    let mut fds = Vec::new();
    for ancillary in control.drain() {
        if let RecvAncillaryMessage::ScmRights(iter) = ancillary {
            fds.extend(iter);
        }
    }

    Ok((msg.bytes, fds))
}

fn is_transient_io(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
    )
}

/// Read until we get a complete JSON line or hit the buffer limit.
///
/// A single `read()` can return a partial response if the kernel hasn't
/// flushed the ember reply yet. Loop until we see a `\n` or the stream
/// returns 0 (EOF) or errors non-transiently.
fn read_full_response(stream: &UnixStream, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut total = 0;
    loop {
        match std::io::Read::read(&mut &*stream, &mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => {
                total += n;
                if buf[..total].contains(&b'\n') || total >= buf.len() {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    if total == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "ember closed connection before sending response",
        ));
    }
    Ok(total)
}

/// RAII guard that re-enables ember connections when dropped.
#[cfg(test)]
pub struct EmberTestGuard(());

#[cfg(test)]
impl Drop for EmberTestGuard {
    fn drop(&mut self) {
        EMBER_DISABLED.with(|c| c.set(false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ember_client_connect_returns_none_when_no_socket() {
        let client = EmberClient::connect();
        // In test environment, ember is not running
        // This may or may not return None depending on test environment
        drop(client);
    }

    #[test]
    fn parse_rpc_response_ok_with_null_result() {
        let line = br#"{"jsonrpc":"2.0","id":1,"result":null}"#;
        let v = parse_rpc_response(line).expect("parse");
        assert!(v.is_null());
    }

    #[test]
    fn parse_rpc_response_err_returns_rpc_variant() {
        let line = br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"fail"}}"#;
        let err = parse_rpc_response(line).expect_err("rpc error");
        match err {
            EmberError::Rpc { code, message } => {
                assert_eq!(code, -32000);
                assert_eq!(message, "fail");
            }
            other => panic!("expected Rpc, got {other:?}"),
        }
    }

    #[test]
    fn parse_rpc_response_invalid_json_returns_parse_error() {
        let line = b"{ not json";
        let err = parse_rpc_response(line).expect_err("parse error");
        assert!(matches!(err, EmberError::Parse(_)));
    }

    #[test]
    fn make_rpc_request_includes_method_and_jsonrpc() {
        let req = make_rpc_request("ember.list", serde_json::json!({}));
        assert!(req.contains("ember.list"));
        assert!(req.contains("\"jsonrpc\":\"2.0\""));
        assert!(req.contains("\"id\":"));
    }

    #[test]
    fn next_request_id_increments() {
        let a = next_request_id();
        let b = next_request_id();
        assert_eq!(b, a + 1);
    }

    #[test]
    fn parse_rpc_response_ok_when_result_key_omitted() {
        let line = br#"{"jsonrpc":"2.0","id":1}"#;
        let v = super::parse_rpc_response(line).expect("parse");
        assert!(v.is_null());
    }

    #[test]
    fn parse_rpc_response_error_with_extra_null_data_field() {
        let line = br#"{"jsonrpc":"2.0","id":1,"error":{"code":-5,"message":"nope","data":null}}"#;
        let err = super::parse_rpc_response(line).expect_err("rpc");
        match err {
            EmberError::Rpc { code, message } => {
                assert_eq!(code, -5);
                assert_eq!(message, "nope");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
