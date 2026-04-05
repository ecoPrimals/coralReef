// SPDX-License-Identifier: AGPL-3.0-only
//! Ember FD-sharing client for diagnostic binaries.
//!
//! Connects to coral-ember's Unix socket, requests VFIO FDs via
//! `SCM_RIGHTS`, and builds a [`VfioDevice`] + [`MappedBar`] for
//! direct BAR0 access. The ember keeps the original FDs alive, so
//! dropping the session closes only the dup'd copies — no VFIO
//! group reset fires.

use std::borrow::Cow;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;

use rustix::io::IoSliceMut;
use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, recvmsg};

use crate::error::{DriverError, DriverResult};
use crate::vfio::device::MappedBar;
use crate::vfio::{ReceivedVfioFds, VfioDevice};

const MAX_RESPONSE: usize = 4096;

/// Ember socket path, matching the canonical wateringHole formula used by
/// `coral-ember`'s server side. `$CORALREEF_EMBER_SOCKET` overrides; default
/// is `$XDG_RUNTIME_DIR/biomeos/coral-ember-{family}.sock`.
fn default_socket() -> String {
    if let Ok(p) = std::env::var("CORALREEF_EMBER_SOCKET") {
        if !p.is_empty() {
            return p;
        }
    }
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let family = std::env::var("BIOMEOS_FAMILY_ID")
        .or_else(|_| std::env::var("CORALREEF_FAMILY_ID"))
        .or_else(|_| std::env::var("FAMILY_ID"))
        .unwrap_or_else(|_| "default".to_string());
    format!("{runtime_dir}/biomeos/coral-ember-{family}.sock")
}

/// A VFIO session obtained from coral-ember via FD sharing.
///
/// Provides direct BAR0 access through the ember-held VFIO device.
/// Dropping this struct closes only the dup'd FD copies — ember's
/// originals keep the VFIO group alive (no bus reset).
pub struct EmberSession {
    /// The VFIO device built from ember's dup'd FDs.
    pub device: VfioDevice,
    /// BAR0 mmap for direct MMIO read/write.
    pub bar0: MappedBar,
}

impl std::fmt::Debug for EmberSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmberSession")
            .field("bdf", &self.device.bdf())
            .finish_non_exhaustive()
    }
}

impl EmberSession {
    /// Connect to ember and obtain BAR0 access for `bdf`.
    ///
    /// # Errors
    ///
    /// Returns `DriverError` if ember is unreachable, the BDF is not
    /// held by ember, or BAR0 mapping fails.
    pub fn connect(bdf: &str) -> DriverResult<Self> {
        let socket_path = default_socket();
        let stream = UnixStream::connect(&socket_path).map_err(|e| {
            DriverError::DeviceNotFound(Cow::Owned(format!("ember socket {socket_path}: {e}")))
        })?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .map_err(|e| DriverError::DeviceNotFound(Cow::Owned(e.to_string())))?;

        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "ember.vfio_fds",
            "params": { "bdf": bdf },
            "id": 1,
        });
        let payload = format!("{req}\n");
        std::io::Write::write_all(&mut &stream, payload.as_bytes())
            .map_err(|e| DriverError::DeviceNotFound(Cow::Owned(format!("ember send: {e}"))))?;

        let mut buf = [0u8; MAX_RESPONSE];
        let (n, fds) = recv_with_fds(&stream, &mut buf)?;

        let resp: serde_json::Value = serde_json::from_slice(&buf[..n]).map_err(|e| {
            DriverError::DeviceNotFound(Cow::Owned(format!("ember response parse: {e}")))
        })?;

        if let Some(err) = resp.get("error") {
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown ember error");
            return Err(DriverError::DeviceNotFound(Cow::Owned(format!(
                "ember error: {msg}"
            ))));
        }

        let result = resp
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let backend = result
            .get("backend")
            .and_then(|b| b.as_str())
            .unwrap_or("legacy");

        let received = match backend {
            "iommufd" => {
                if fds.len() < 2 {
                    return Err(DriverError::DeviceNotFound(Cow::Owned(format!(
                        "ember: expected 2 fds for iommufd, got {}",
                        fds.len()
                    ))));
                }
                let ioas_id = result.get("ioas_id").and_then(|v| v.as_u64()).ok_or(
                    DriverError::DeviceNotFound(Cow::Borrowed(
                        "ember: iommufd response missing ioas_id",
                    )),
                )? as u32;
                let mut it = fds.into_iter();
                ReceivedVfioFds::Iommufd {
                    iommufd: it.next().expect("checked len"),
                    device: it.next().expect("checked len"),
                    ioas_id,
                }
            }
            _ => {
                if fds.len() < 3 {
                    return Err(DriverError::DeviceNotFound(Cow::Owned(format!(
                        "ember: expected 3 fds for legacy, got {}",
                        fds.len()
                    ))));
                }
                let mut it = fds.into_iter();
                ReceivedVfioFds::Legacy {
                    container: it.next().expect("checked len"),
                    group: it.next().expect("checked len"),
                    device: it.next().expect("checked len"),
                }
            }
        };

        let device = VfioDevice::from_received(bdf, received)?;
        let bar0 = device.map_bar(0)?;

        Ok(Self { device, bar0 })
    }
}

fn recv_with_fds(sock: impl AsFd, buf: &mut [u8]) -> DriverResult<(usize, Vec<OwnedFd>)> {
    const MAX_SCM_FDS: usize = 3;

    let mut iov = [IoSliceMut::new(buf)];
    let mut recv_space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(MAX_SCM_FDS))];
    let mut control = RecvAncillaryBuffer::new(&mut recv_space);

    let msg = recvmsg(sock, &mut iov, &mut control, RecvFlags::empty())
        .map_err(|e| DriverError::DeviceNotFound(Cow::Owned(format!("ember recvmsg: {e}"))))?;

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
    fn connect_fails_gracefully_when_no_ember() {
        // SAFETY: single-threaded test; no other thread reads this env var concurrently.
        unsafe {
            std::env::set_var("CORALREEF_EMBER_SOCKET", "/tmp/nonexistent-ember-test.sock");
        }
        let result = EmberSession::connect("0000:99:00.0");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("ember") || err.contains("socket") || err.contains("No such file"),
            "unexpected error: {err}"
        );
    }
}
