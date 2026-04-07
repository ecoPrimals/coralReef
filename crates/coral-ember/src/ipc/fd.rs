// SPDX-License-Identifier: AGPL-3.0-only
//! Ancillary `SCM_RIGHTS` fd passing via `sendmsg`/`recvmsg`.

use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use rustix::io::IoSlice;
use rustix::net::{
    RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags,
    SendAncillaryBuffer, SendAncillaryMessage, SendFlags,
    recvmsg, sendmsg,
};

/// Send data with ancillary `SCM_RIGHTS` file descriptors (`rustix::net::sendmsg`).
pub fn send_with_fds(
    stream: impl AsFd,
    data: &[u8],
    fds: &[BorrowedFd<'_>],
) -> std::io::Result<()> {
    let iov = [IoSlice::new(data)];
    let mut space = vec![MaybeUninit::uninit(); SendAncillaryMessage::ScmRights(fds).size()];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !control.push(SendAncillaryMessage::ScmRights(fds)) {
        return Err(std::io::Error::other(
            "ancillary buffer too small for SCM_RIGHTS",
        ));
    }

    sendmsg(stream, &iov, &mut control, SendFlags::empty())?;
    Ok(())
}

/// Receive data and any ancillary `SCM_RIGHTS` file descriptors via `recvmsg`.
///
/// Returns `(bytes_read, received_fds)`. The received fds are `OwnedFd` —
/// the caller takes ownership.
pub fn recv_with_fds(
    stream: impl AsFd,
    buf: &mut [u8],
    max_fds: usize,
) -> std::io::Result<(usize, Vec<OwnedFd>)> {
    let mut iov = [rustix::io::IoSliceMut::new(buf)];
    let fd_space = max_fds * std::mem::size_of::<OwnedFd>() + 64;
    let mut ancillary_space = vec![MaybeUninit::uninit(); fd_space];
    let mut control = RecvAncillaryBuffer::new(&mut ancillary_space);

    let result = recvmsg(&stream, &mut iov, &mut control, RecvFlags::empty())?;
    let bytes = result.bytes;

    let mut fds = Vec::new();
    for msg in control.drain() {
        if let RecvAncillaryMessage::ScmRights(rights) = msg {
            fds.extend(rights);
        }
    }

    Ok((bytes, fds))
}
