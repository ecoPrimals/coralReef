// SPDX-License-Identifier: AGPL-3.0-only
//! Lightweight ember client for hardware tests.
//!
//! Requests VFIO fds from coral-ember via SCM_RIGHTS so tests can construct
//! NvVfioComputeDevice without competing with ember for /dev/vfio/*.
#![allow(dead_code, unsafe_code)]

use std::io::Write;
use std::os::fd::{FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;

const EMBER_SOCKET: &str = "/run/coralreef/ember.sock";

pub struct EmberFds {
    pub container: OwnedFd,
    pub group: OwnedFd,
    pub device: OwnedFd,
}

pub fn request_fds(bdf: &str) -> Result<EmberFds, String> {
    let stream = UnixStream::connect(EMBER_SOCKET).map_err(|e| format!("connect to ember: {e}"))?;
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
    let (n, fds) = recv_with_fds(std::os::fd::AsRawFd::as_raw_fd(&stream), &mut buf, 3)
        .map_err(|e| format!("recvmsg: {e}"))?;

    let resp: serde_json::Value =
        serde_json::from_slice(&buf[..n]).map_err(|e| format!("parse: {e}"))?;

    if resp.get("error").is_some() {
        let err = resp["error"]["message"].as_str().unwrap_or("unknown");
        return Err(format!("ember: {err}"));
    }

    if fds.len() < 3 {
        return Err(format!("need 3 fds, got {}", fds.len()));
    }

    let mut it = fds.into_iter();
    Ok(EmberFds {
        container: it.next().unwrap(),
        group: it.next().unwrap(),
        device: it.next().unwrap(),
    })
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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
    let cmsg_space = unsafe { libc::CMSG_SPACE(fd_payload_size as libc::c_uint) } as usize;
    let mut cmsg_buf = vec![0u8; cmsg_space];

    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &raw mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg_buf.as_mut_ptr().cast();
    msg.msg_controllen = cmsg_space as libc::size_t;

    let n = unsafe { libc::recvmsg(sock_fd, &raw mut msg, 0) };
    if n < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let mut fds = Vec::new();
    let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(&raw const msg) };
    while !cmsg.is_null() {
        unsafe {
            if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                let fd_ptr = libc::CMSG_DATA(cmsg).cast::<RawFd>();
                let cmsg_len_header = libc::CMSG_LEN(0) as usize;
                let payload_len = (*cmsg).cmsg_len as usize - cmsg_len_header;
                let num_fds = payload_len / std::mem::size_of::<RawFd>();
                for i in 0..num_fds {
                    fds.push(OwnedFd::from_raw_fd(*fd_ptr.add(i)));
                }
            }
            cmsg = libc::CMSG_NXTHDR(&raw const msg, cmsg);
        }
    }

    Ok((n as usize, fds))
}
