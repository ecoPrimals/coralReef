// SPDX-License-Identifier: AGPL-3.0-only
//! Lightweight ember client for hardware tests.
//!
//! Requests VFIO fds from coral-ember via SCM_RIGHTS so tests can construct
//! `NvVfioComputeDevice` without competing with ember for `/dev/vfio/*`.
//!
//! Backend-aware: handles both legacy (3 fds) and iommufd (2 fds + ioas_id)
//! responses from ember.
#![allow(dead_code)]

use std::io::Write;
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;

use coral_driver::vfio::ReceivedVfioFds;
use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, recvmsg};

const DEFAULT_EMBER_SOCKET: &str = "/run/coralreef/ember.sock";

fn ember_socket_path() -> String {
    std::env::var("CORALREEF_EMBER_SOCKET").unwrap_or_else(|_| DEFAULT_EMBER_SOCKET.to_string())
}

/// Request a PCI device reset from Ember (which runs as root).
///
/// `method` is one of: `"auto"`, `"sbr"`, `"bridge-sbr"`, `"remove-rescan"`.
/// Bridge-SBR resets all devices behind the parent PCI bridge — the only
/// reset mechanism available on GV100 Titan V (no FLR capability).
///
/// After a bridge-SBR, existing BAR mappings become invalid. Callers must
/// re-acquire VFIO fds and re-map BARs.
pub fn device_reset(bdf: &str, method: &str) -> Result<(), String> {
    let socket_path = ember_socket_path();
    let stream = UnixStream::connect(&socket_path).map_err(|e| format!("connect to ember: {e}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "ember.device_reset",
        "params": {"bdf": bdf, "method": method},
        "id": 2
    });
    let req_bytes = format!("{req}\n");
    (&stream)
        .write_all(req_bytes.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let mut buf = [0u8; 4096];
    let n = std::io::Read::read(&mut &stream, &mut buf).map_err(|e| format!("read: {e}"))?;

    let resp: serde_json::Value =
        serde_json::from_slice(&buf[..n]).map_err(|e| format!("parse: {e}"))?;

    if let Some(err) = resp.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(format!("ember.device_reset: {msg}"));
    }

    Ok(())
}

/// Ask ember to safely prepare a device for DMA experiments.
///
/// Ember maps BAR0 server-side, quiesces stale DMA engines (PFIFO reset,
/// scheduler stop, blind PRI ring ACK), masks AER, and enables bus mastering.
/// Call [`cleanup_dma`] when the experiment is done.
pub fn prepare_dma(bdf: &str) -> Result<serde_json::Value, String> {
    let socket_path = ember_socket_path();
    let stream =
        UnixStream::connect(&socket_path).map_err(|e| format!("connect to ember: {e}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "ember.prepare_dma",
        "params": {"bdf": bdf},
        "id": 3
    });
    let req_bytes = format!("{req}\n");
    (&stream)
        .write_all(req_bytes.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let mut buf = [0u8; 4096];
    let n = std::io::Read::read(&mut &stream, &mut buf).map_err(|e| format!("read: {e}"))?;

    let resp: serde_json::Value =
        serde_json::from_slice(&buf[..n]).map_err(|e| format!("parse: {e}"))?;

    if let Some(err) = resp.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(format!("ember.prepare_dma: {msg}"));
    }

    Ok(resp.get("result").cloned().unwrap_or_default())
}

/// Ask ember to clean up after a DMA experiment — disables bus master, restores AER.
pub fn cleanup_dma(bdf: &str) -> Result<(), String> {
    let socket_path = ember_socket_path();
    let stream =
        UnixStream::connect(&socket_path).map_err(|e| format!("connect to ember: {e}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "ember.cleanup_dma",
        "params": {"bdf": bdf},
        "id": 4
    });
    let req_bytes = format!("{req}\n");
    (&stream)
        .write_all(req_bytes.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let mut buf = [0u8; 4096];
    let n = std::io::Read::read(&mut &stream, &mut buf).map_err(|e| format!("read: {e}"))?;

    let resp: serde_json::Value =
        serde_json::from_slice(&buf[..n]).map_err(|e| format!("parse: {e}"))?;

    if let Some(err) = resp.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(format!("ember.cleanup_dma: {msg}"));
    }

    Ok(())
}

pub fn request_fds(bdf: &str) -> Result<ReceivedVfioFds, String> {
    let socket_path = ember_socket_path();
    let stream = UnixStream::connect(&socket_path).map_err(|e| format!("connect to ember: {e}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "ember.vfio_fds",
        "params": {"bdf": bdf},
        "id": 1
    });
    let req_bytes = format!("{req}\n");
    (&stream)
        .write_all(req_bytes.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let mut buf = [0u8; 4096];
    let mut cmsg_space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(3))];
    let mut cmsg_buffer = RecvAncillaryBuffer::new(&mut cmsg_space);

    let result = recvmsg(
        &stream,
        &mut [rustix::io::IoSliceMut::new(&mut buf)],
        &mut cmsg_buffer,
        RecvFlags::empty(),
    )
    .map_err(|e| format!("recvmsg: {e}"))?;

    let n = result.bytes;

    let resp: serde_json::Value =
        serde_json::from_slice(&buf[..n]).map_err(|e| format!("parse: {e}"))?;

    if resp.get("error").is_some() {
        let err = resp["error"]["message"].as_str().unwrap_or("unknown");
        return Err(format!("ember: {err}"));
    }

    let mut fds: Vec<OwnedFd> = Vec::new();
    for msg in cmsg_buffer.drain() {
        if let RecvAncillaryMessage::ScmRights(rights) = msg {
            fds.extend(rights);
        }
    }

    let backend = resp
        .get("result")
        .and_then(|r| r.get("backend"))
        .and_then(|b| b.as_str())
        .unwrap_or("legacy");

    match backend {
        "iommufd" => {
            if fds.len() < 2 {
                return Err(format!("iommufd: need 2 fds, got {}", fds.len()));
            }
            let ioas_id = resp
                .get("result")
                .and_then(|r| r.get("ioas_id"))
                .and_then(|v| v.as_u64())
                .ok_or("iommufd response missing ioas_id")? as u32;
            let mut it = fds.into_iter();
            Ok(ReceivedVfioFds::Iommufd {
                iommufd: it.next().expect("checked len >= 2"),
                device: it.next().expect("checked len >= 2"),
                ioas_id,
            })
        }
        _ => {
            if fds.len() < 3 {
                return Err(format!("legacy: need 3 fds, got {}", fds.len()));
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
