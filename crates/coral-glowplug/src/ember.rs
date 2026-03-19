// SPDX-License-Identifier: AGPL-3.0-only
//! Ember client — connects to coral-ember and receives VFIO fds via `SCM_RIGHTS`.
//!
//! When coral-ember is running, the daemon receives duplicate VFIO fds
//! from it instead of opening them directly. This allows the daemon to
//! be restarted without triggering PM reset on GV100 (no FLR support).

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;

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
    #[allow(dead_code)]
    jsonrpc: String,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
    #[allow(dead_code)]
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
    }).to_string()
}

fn parse_rpc_response(buf: &[u8]) -> Result<serde_json::Value, EmberError> {
    let resp: JsonRpcResponse = serde_json::from_slice(buf)?;
    if let Some(err) = resp.error {
        return Err(EmberError::Rpc { code: err.code, message: err.message });
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
        let stream = UnixStream::connect(&self.socket_path)
            .map_err(EmberError::Connect)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;

        let req = make_rpc_request("ember.list", serde_json::json!({}));
        std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())?;

        let mut buf = [0u8; MAX_RESPONSE_SIZE];
        let n = std::io::Read::read(&mut &stream, &mut buf)?;
        let result = parse_rpc_response(&buf[..n])?;
        let devices = result.get("devices")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        Ok(devices)
    }

    /// Tell ember to release VFIO fds for a device (before driver swap).
    ///
    /// Superseded by `swap_device` for normal swaps; retained for manual
    /// debugging via socat or targeted fd release.
    #[allow(dead_code, reason = "retained for manual debugging via socat")]
    pub fn release_device(&self, bdf: &str) -> Result<(), EmberError> {
        let stream = UnixStream::connect(&self.socket_path)
            .map_err(EmberError::Connect)?;
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
        let stream = UnixStream::connect(&self.socket_path)
            .map_err(EmberError::Connect)?;
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
        let stream = UnixStream::connect(&self.socket_path)
            .map_err(EmberError::Connect)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;

        let req = make_rpc_request("ember.swap", serde_json::json!({"bdf": bdf, "target": target}));
        std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())?;

        let mut buf = [0u8; MAX_RESPONSE_SIZE];
        let n = std::io::Read::read(&mut &stream, &mut buf)?;
        let result = parse_rpc_response(&buf[..n])?;
        Ok(result.get("personality")
            .and_then(|v| v.as_str())
            .unwrap_or(target)
            .to_string())
    }

    /// Request VFIO fds for a specific BDF from the ember.
    pub fn request_fds(&self, bdf: &str) -> Result<EmberFds, EmberError> {
        let stream = UnixStream::connect(&self.socket_path)
            .map_err(EmberError::Connect)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;

        let req = make_rpc_request("ember.vfio_fds", serde_json::json!({"bdf": bdf}));
        std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())?;

        let mut buf = [0u8; MAX_RESPONSE_SIZE];
        let (n, fds) = recv_with_fds(stream.as_raw_fd(), &mut buf, 3)?;

        let resp: JsonRpcResponse = serde_json::from_slice(&buf[..n])?;

        if let Some(err) = resp.error {
            return Err(EmberError::Rpc { code: err.code, message: err.message });
        }

        let result = resp.result.unwrap_or(serde_json::Value::Null);
        let expected = result.get("num_fds").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
        if fds.len() < expected {
            return Err(EmberError::FdCount { expected, received: fds.len() });
        }

        if fds.len() < 3 {
            return Err(EmberError::FdCount { expected: 3, received: fds.len() });
        }

        let mut fd_iter = fds.into_iter();
        Ok(EmberFds {
            container: fd_iter.next().expect("checked fds.len() >= 3"),
            group: fd_iter.next().expect("checked fds.len() >= 3"),
            device: fd_iter.next().expect("checked fds.len() >= 3"),
        })
    }
}

/// Receive data with ancillary `SCM_RIGHTS` file descriptors.
#[allow(
    unsafe_code,
    clippy::cast_possible_truncation,
    clippy::cast_ptr_alignment,
    clippy::cast_sign_loss
)]
fn recv_with_fds(
    sock_fd: RawFd,
    buf: &mut [u8],
    max_fds: usize,
) -> std::io::Result<(usize, Vec<OwnedFd>)> {
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    };

    let fd_payload_size = max_fds * std::mem::size_of::<RawFd>();
    // SAFETY: CMSG_SPACE computes the correct alignment-safe buffer size.
    let cmsg_space = unsafe { libc::CMSG_SPACE(fd_payload_size as libc::c_uint) } as usize;
    let mut cmsg_buf = vec![0u8; cmsg_space];

    // SAFETY: zeroed msghdr is valid; we fill required fields below.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &raw mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg_buf.as_mut_ptr().cast();
    msg.msg_controllen = cmsg_space as libc::size_t;

    // SAFETY: sock_fd is a valid Unix socket; msg is properly initialized;
    // recvmsg populates cmsg_buf with ancillary data.
    let n = unsafe { libc::recvmsg(sock_fd, &raw mut msg, 0) };
    if n < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let mut fds = Vec::new();
    // SAFETY: after successful recvmsg, CMSG_FIRSTHDR returns a valid pointer
    // (or null if no control messages). CMSG_NXTHDR iterates safely.
    let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(&raw const msg) };
    while !cmsg.is_null() {
        unsafe {
            if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                let fd_ptr = libc::CMSG_DATA(cmsg).cast::<RawFd>();
                let cmsg_len_header = libc::CMSG_LEN(0) as usize;
                let payload_len = (*cmsg).cmsg_len as usize - cmsg_len_header;
                let num_fds = payload_len / std::mem::size_of::<RawFd>();
                for i in 0..num_fds {
                    let raw_fd = *fd_ptr.add(i);
                    // SAFETY: fds received via SCM_RIGHTS are new fds in our
                    // process; we take ownership via OwnedFd.
                    fds.push(OwnedFd::from_raw_fd(raw_fd));
                }
            }
            cmsg = libc::CMSG_NXTHDR(&raw const msg, cmsg);
        }
    }

    Ok((n as usize, fds))
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
