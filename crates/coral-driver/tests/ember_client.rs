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
use std::time::Duration;

use coral_driver::vfio::ReceivedVfioFds;
use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, recvmsg};

fn ember_socket_path() -> String {
    std::env::var("CORALREEF_EMBER_SOCKET")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let runtime_dir =
                std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
            format!("{runtime_dir}/biomeos/coral-ember-default.sock")
        })
}

fn simple_rpc(
    method: &str,
    params: serde_json::Value,
    timeout: Duration,
) -> Result<serde_json::Value, String> {
    let socket_path = ember_socket_path();
    let stream = UnixStream::connect(&socket_path).map_err(|e| format!("connect to ember: {e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("set timeout: {e}"))?;

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1,
    });
    let line = format!("{req}\n");
    (&stream)
        .write_all(line.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let mut buf = [0u8; 16384];
    let n = std::io::Read::read(&mut &stream, &mut buf).map_err(|e| format!("read: {e}"))?;

    let resp: serde_json::Value =
        serde_json::from_slice(&buf[..n]).map_err(|e| format!("parse: {e}"))?;

    if let Some(err) = resp.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(format!("{method}: {msg}"));
    }

    resp.get("result")
        .cloned()
        .ok_or_else(|| format!("{method}: response has no result"))
}

/// Request a PCI device reset from Ember (which runs as root).
///
/// `method` is one of: `"auto"`, `"sbr"`, `"bridge-sbr"`, `"remove-rescan"`.
pub fn device_reset(bdf: &str, method: &str) -> Result<(), String> {
    simple_rpc(
        "ember.device_reset",
        serde_json::json!({"bdf": bdf, "method": method}),
        Duration::from_secs(30),
    )?;
    Ok(())
}

/// List BDFs of all devices currently held by ember.
pub fn list() -> Result<Vec<String>, String> {
    let result = simple_rpc("ember.list", serde_json::json!({}), Duration::from_secs(5))?;
    let devices = result
        .get("devices")
        .and_then(|v| v.as_array())
        .ok_or("ember.list: missing devices array")?;
    Ok(devices
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect())
}

/// Get ember daemon status (held devices + uptime).
pub fn status() -> Result<serde_json::Value, String> {
    simple_rpc(
        "ember.status",
        serde_json::json!({}),
        Duration::from_secs(5),
    )
}

/// Release ember's hold on a device's VFIO fds (for VM passthrough or swap).
pub fn release(bdf: &str) -> Result<(), String> {
    simple_rpc(
        "ember.release",
        serde_json::json!({"bdf": bdf}),
        Duration::from_secs(10),
    )?;
    Ok(())
}

/// Swap a device to a different driver personality via ember.
pub fn swap(bdf: &str, target: &str) -> Result<serde_json::Value, String> {
    simple_rpc(
        "ember.swap",
        serde_json::json!({"bdf": bdf, "target": target}),
        Duration::from_secs(60),
    )
}

/// Read a single BAR0 MMIO register via ember (sysfs resource0 mmap).
/// Returns the raw u32 value. `offset` is in bytes.
pub fn mmio_read(bdf: &str, offset: u32) -> Result<u32, String> {
    let result = simple_rpc(
        "ember.mmio.read",
        serde_json::json!({"bdf": bdf, "offset": offset}),
        Duration::from_secs(5),
    )?;
    let value_str = result
        .get("value")
        .and_then(|v| v.as_str())
        .ok_or("ember.mmio.read: missing value")?;
    u32::from_str_radix(value_str.trim_start_matches("0x"), 16)
        .map_err(|e| format!("ember.mmio.read: parse hex: {e}"))
}

/// Snapshot FECS falcon state via ember BAR0 reads.
pub fn fecs_state(bdf: &str) -> Result<serde_json::Value, String> {
    simple_rpc(
        "ember.fecs.state",
        serde_json::json!({"bdf": bdf}),
        Duration::from_secs(5),
    )
}

/// Get kernel livepatch status (loaded, enabled, transition, patched_funcs).
pub fn livepatch_status() -> Result<serde_json::Value, String> {
    simple_rpc(
        "ember.livepatch.status",
        serde_json::json!({}),
        Duration::from_secs(5),
    )
}

/// Enable the kernel livepatch (modprobe if needed, enable, wait for transition).
pub fn livepatch_enable() -> Result<serde_json::Value, String> {
    simple_rpc(
        "ember.livepatch.enable",
        serde_json::json!({}),
        Duration::from_secs(15),
    )
}

/// Disable the kernel livepatch.
pub fn livepatch_disable() -> Result<serde_json::Value, String> {
    simple_rpc(
        "ember.livepatch.disable",
        serde_json::json!({}),
        Duration::from_secs(15),
    )
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
