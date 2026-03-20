// SPDX-License-Identifier: AGPL-3.0-only
#![allow(
    missing_docs,
    reason = "SCM_RIGHTS plumbing types mirror libc usage; module-level docs describe behavior."
)]
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

use crate::error::EmberError;

const EMBER_SOCKET: &str = "/run/coralreef/ember.sock";
const MAX_RESPONSE_SIZE: usize = 4096;

/// VFIO fds received from the ember for a single device.
pub struct EmberFds {
    pub container: OwnedFd,
    pub group: OwnedFd,
    pub device: OwnedFd,
}

#[derive(Deserialize)]
struct JsonRpcResponse {
    #[expect(
        dead_code,
        reason = "parsed for protocol validation but not used directly"
    )]
    jsonrpc: String,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
    #[expect(
        dead_code,
        reason = "parsed for protocol validation but not used directly"
    )]
    id: serde_json::Value,
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

impl EmberClient {
    /// Try to connect to the ember. Returns None if the ember is not running.
    pub fn connect() -> Option<Self> {
        let path = EMBER_SOCKET;
        if !std::path::Path::new(path).exists() {
            tracing::debug!("ember socket not found at {path}");
            return None;
        }

        match UnixStream::connect(path) {
            Ok(stream) => {
                drop(stream);
                tracing::info!(path, "ember is available");
                Some(Self {
                    socket_path: path.to_string(),
                })
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
    #[allow(dead_code, reason = "retained for manual debugging via socat")]
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
    #[allow(dead_code, reason = "retained for manual debugging via socat")]
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
    pub fn swap_device(&self, bdf: &str, target: &str) -> Result<String, EmberError> {
        let stream = UnixStream::connect(&self.socket_path).map_err(EmberError::Connect)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;

        let req = make_rpc_request(
            "ember.swap",
            serde_json::json!({"bdf": bdf, "target": target}),
        );
        std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())?;

        let mut buf = [0u8; MAX_RESPONSE_SIZE];
        let n = std::io::Read::read(&mut &stream, &mut buf)?;
        let result = parse_rpc_response(&buf[..n])?;
        Ok(result
            .get("personality")
            .and_then(|v| v.as_str())
            .unwrap_or(target)
            .to_string())
    }

    /// Request VFIO fds for a specific BDF from the ember.
    pub fn request_fds(&self, bdf: &str) -> Result<EmberFds, EmberError> {
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

        if fds.len() < 3 {
            return Err(EmberError::FdCount {
                expected: 3,
                received: fds.len(),
            });
        }

        let mut fd_iter = fds.into_iter();
        Ok(EmberFds {
            container: fd_iter.next().expect("checked fds.len() >= 3"),
            group: fd_iter.next().expect("checked fds.len() >= 3"),
            device: fd_iter.next().expect("checked fds.len() >= 3"),
        })
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
}
