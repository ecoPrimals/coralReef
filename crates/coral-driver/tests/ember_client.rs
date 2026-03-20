// SPDX-License-Identifier: AGPL-3.0-only
//! Lightweight ember client for hardware tests.
//!
//! Requests VFIO fds from coral-ember via SCM_RIGHTS so tests can construct
//! `NvVfioComputeDevice` without competing with ember for `/dev/vfio/*`.
//!
//! Evolved from `libc` raw `recvmsg` to `rustix::net` — zero libc, zero unsafe.
#![allow(dead_code)]

use std::io::Write;
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;

use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, recvmsg};

const DEFAULT_EMBER_SOCKET: &str = "/run/coralreef/ember.sock";

fn ember_socket_path() -> String {
    std::env::var("CORALREEF_EMBER_SOCKET").unwrap_or_else(|_| DEFAULT_EMBER_SOCKET.to_string())
}

pub struct EmberFds {
    pub container: OwnedFd,
    pub group: OwnedFd,
    pub device: OwnedFd,
}

pub fn request_fds(bdf: &str) -> Result<EmberFds, String> {
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

    if fds.len() < 3 {
        return Err(format!("need 3 fds, got {}", fds.len()));
    }

    let mut it = fds.into_iter();
    Ok(EmberFds {
        container: it
            .next()
            .expect("container fd present — length checked above"),
        group: it.next().expect("group fd present — length checked above"),
        device: it.next().expect("device fd present — length checked above"),
    })
}
