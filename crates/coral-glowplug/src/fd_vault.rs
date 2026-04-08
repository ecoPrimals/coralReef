// SPDX-License-Identifier: AGPL-3.0-only
//! FdVault — stashes duplicate VFIO fds from ember so GPU bindings survive
//! ember death.
//!
//! When ember checkpoints its fds via `ember.checkpoint_fds`, glowplug
//! receives kernel-dup'd copies via `SCM_RIGHTS`. These duplicates keep the
//! VFIO binding alive in the kernel — if ember subsequently dies (crash,
//! SIGKILL, timeout), the kernel sees that at least one fd is still open
//! and does NOT trigger a PM reset. Glowplug can then spawn a new ember
//! with `--resurrect` and pass the fds back.

use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;

use rustix::io::{IoSlice, IoSliceMut};
use rustix::net::{
    RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
    SendAncillaryMessage, SendFlags, recvmsg, sendmsg,
};

use crate::error::EmberError;

/// Maximum number of fds receivable in a single checkpoint (generous for
/// multi-GPU setups: 8 GPUs × 3 fds each = 24).
const MAX_CHECKPOINT_FDS: usize = 32;

/// Per-device fd set in the vault, mirroring the device's VFIO backend.
#[derive(Debug)]
pub enum VaultedFds {
    /// Legacy VFIO: container, group, device.
    Legacy {
        /// Container fd.
        container: OwnedFd,
        /// Group fd.
        group: OwnedFd,
        /// Device fd.
        device: OwnedFd,
    },
    /// Modern iommufd: iommufd fd, device fd, plus ioas_id metadata.
    Iommufd {
        /// `/dev/iommu` fd.
        iommufd: OwnedFd,
        /// VFIO cdev device fd.
        device: OwnedFd,
        /// IOAS id from the checkpoint manifest.
        ioas_id: u32,
    },
}

/// Manifest entry for one device in the checkpoint response.
#[derive(Debug, serde::Deserialize)]
struct DeviceManifest {
    bdf: String,
    num_fds: usize,
    backend: String,
    #[serde(default)]
    ioas_id: Option<u32>,
}

/// Vault holding backup VFIO fds for all ember-managed devices.
///
/// Thread-safe: uses interior `RwLock` so the health monitor can read
/// while the lifecycle manager can update.
#[derive(Debug, Default)]
pub struct FdVault {
    entries: std::sync::RwLock<HashMap<String, VaultedFds>>,
}

impl FdVault {
    /// Create an empty vault.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of devices currently vaulted.
    pub fn device_count(&self) -> usize {
        self.entries
            .read()
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// Check if a specific BDF has vaulted fds.
    pub fn has_device(&self, bdf: &str) -> bool {
        self.entries
            .read()
            .map(|m| m.contains_key(bdf))
            .unwrap_or(false)
    }

    /// List all BDFs currently in the vault.
    pub fn bdfs(&self) -> Vec<String> {
        self.entries
            .read()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Evict (release) vaulted fds for a specific BDF.
    ///
    /// Must be called BEFORE a warm cycle so the kernel's VFIO refcount drops
    /// to zero, allowing clean driver unbind. Returns true if the BDF was
    /// present and evicted.
    pub fn evict(&self, bdf: &str) -> bool {
        match self.entries.write() {
            Ok(mut map) => {
                let had = map.remove(bdf).is_some();
                if had {
                    tracing::info!(bdf, "fd vault: evicted (VFIO fds released for warm cycle)");
                }
                had
            }
            Err(_) => false,
        }
    }

    /// Checkpoint: connect to ember, request all fds, stash them.
    ///
    /// Replaces any previously vaulted fds (the old `OwnedFd`s are dropped,
    /// but that only decrements the kernel refcount — ember's copy is the
    /// primary one keeping the binding alive until ember dies).
    pub fn checkpoint_from_ember(&self, ember_socket: &str) -> Result<usize, EmberError> {
        let stream = UnixStream::connect(ember_socket).map_err(EmberError::Connect)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;

        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "ember.checkpoint_fds",
            "params": {},
            "id": 1,
        });
        std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())?;

        let mut buf = [0u8; 8192];
        let (n, fds) = recv_with_fds_large(&stream, &mut buf)?;

        let resp: serde_json::Value = serde_json::from_slice(&buf[..n])
            .map_err(EmberError::Parse)?;

        if let Some(err) = resp.get("error") {
            let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(-1) as i32;
            let message = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
                .to_string();
            return Err(EmberError::Rpc { code, message });
        }

        let result = resp
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let manifest: Vec<DeviceManifest> = serde_json::from_value(
            result.get("devices").cloned().unwrap_or_default(),
        )
        .map_err(EmberError::Parse)?;

        let total_expected: usize = manifest.iter().map(|d| d.num_fds).sum();
        if fds.len() < total_expected {
            return Err(EmberError::FdCount {
                expected: total_expected,
                received: fds.len(),
            });
        }

        let mut new_entries = HashMap::new();
        let mut fd_iter = fds.into_iter();

        for dev in &manifest {
            let vaulted = match dev.backend.as_str() {
                "iommufd" => {
                    let iommufd = fd_iter.next().ok_or(EmberError::FdCount {
                        expected: dev.num_fds,
                        received: 0,
                    })?;
                    let device = fd_iter.next().ok_or(EmberError::FdCount {
                        expected: dev.num_fds,
                        received: 1,
                    })?;
                    VaultedFds::Iommufd {
                        iommufd,
                        device,
                        ioas_id: dev.ioas_id.unwrap_or(0),
                    }
                }
                _ => {
                    let container = fd_iter.next().ok_or(EmberError::FdCount {
                        expected: dev.num_fds,
                        received: 0,
                    })?;
                    let group = fd_iter.next().ok_or(EmberError::FdCount {
                        expected: dev.num_fds,
                        received: 1,
                    })?;
                    let device = fd_iter.next().ok_or(EmberError::FdCount {
                        expected: dev.num_fds,
                        received: 2,
                    })?;
                    VaultedFds::Legacy {
                        container,
                        group,
                        device,
                    }
                }
            };

            new_entries.insert(dev.bdf.clone(), vaulted);
        }

        let count = new_entries.len();
        let mut lock = self.entries.write().map_err(|_| {
            EmberError::Io(std::io::Error::other("fd vault lock poisoned"))
        })?;
        *lock = new_entries;
        drop(lock);

        tracing::info!(
            devices = count,
            total_fds = total_expected,
            "fd vault: checkpoint complete"
        );

        Ok(count)
    }

    /// Take vaulted fds for a device, removing them from the vault.
    ///
    /// Used during ember resurrection: glowplug hands these back to the
    /// new ember process via SCM_RIGHTS.
    pub fn take(&self, bdf: &str) -> Option<VaultedFds> {
        self.entries
            .write()
            .ok()
            .and_then(|mut m| m.remove(bdf))
    }

    /// Prepare vaulted fds for restoration to a resurrecting ember.
    ///
    /// Takes (removes) the vaulted fds for each requested BDF and returns
    /// them as a flat `Vec<OwnedFd>` in manifest order, plus a JSON manifest
    /// describing the fd layout so the receiver can reconstruct `VfioDevice`s.
    pub fn restore_for_bdfs(
        &self,
        bdfs: &[String],
    ) -> Result<(Vec<OwnedFd>, serde_json::Value), String> {
        let mut all_fds = Vec::new();
        let mut manifest = Vec::new();

        for bdf in bdfs {
            let vaulted = self
                .take(bdf)
                .ok_or_else(|| format!("no vaulted fds for BDF {bdf}"))?;
            match vaulted {
                VaultedFds::Legacy {
                    container,
                    group,
                    device,
                } => {
                    manifest.push(serde_json::json!({
                        "bdf": bdf,
                        "backend": "legacy",
                        "num_fds": 3,
                    }));
                    all_fds.push(container);
                    all_fds.push(group);
                    all_fds.push(device);
                }
                VaultedFds::Iommufd {
                    iommufd,
                    device,
                    ioas_id,
                } => {
                    manifest.push(serde_json::json!({
                        "bdf": bdf,
                        "backend": "iommufd",
                        "num_fds": 2,
                        "ioas_id": ioas_id,
                    }));
                    all_fds.push(iommufd);
                    all_fds.push(device);
                }
            }
        }

        Ok((all_fds, serde_json::json!({ "devices": manifest })))
    }

    /// Remove all vaulted fds (e.g. after a full system reset).
    pub fn clear(&self) {
        if let Ok(mut m) = self.entries.write() {
            m.clear();
        }
    }
}

/// Send data with ancillary `SCM_RIGHTS` file descriptors via `sendmsg`.
///
/// Mirrors the identical helper in `coral-ember::ipc::fd` — duplicated to
/// avoid a cross-crate dependency for a 10-line function.
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

/// Handle a `vault.restore_fds` JSON-RPC request on a synchronous Unix stream.
///
/// Reads the request, extracts BDFs, takes the vaulted fds, and sends the
/// JSON response + fds back via SCM_RIGHTS in a single `sendmsg`.
pub fn handle_vault_restore(stream: &UnixStream, vault: &FdVault) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    let n = std::io::Read::read(&mut &*stream, &mut buf)?;
    if n == 0 {
        return Err(std::io::Error::other("empty vault restore request"));
    }

    let req: serde_json::Value = serde_json::from_slice(&buf[..n])
        .map_err(|e| std::io::Error::other(format!("parse vault request: {e}")))?;

    let bdfs: Vec<String> = req
        .get("params")
        .and_then(|p| p.get("bdfs"))
        .and_then(|b| b.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);

    match vault.restore_for_bdfs(&bdfs) {
        Ok((fds, result)) => {
            let response = serde_json::json!({
                "jsonrpc": "2.0",
                "result": result,
                "id": id,
            });
            let response_bytes = format!("{response}\n");
            let borrowed: Vec<BorrowedFd<'_>> = fds.iter().map(AsFd::as_fd).collect();
            send_with_fds(&*stream, response_bytes.as_bytes(), &borrowed)?;
            tracing::info!(
                bdf_count = bdfs.len(),
                fd_count = fds.len(),
                "vault restore: fds sent to resurrecting ember"
            );
            Ok(())
        }
        Err(msg) => {
            let response = serde_json::json!({
                "jsonrpc": "2.0",
                "error": { "code": -32000, "message": msg },
                "id": id,
            });
            std::io::Write::write_all(&mut &*stream, format!("{response}\n").as_bytes())
        }
    }
}

/// Receive data + SCM_RIGHTS fds, sized for checkpoint payloads.
fn recv_with_fds_large(sock: impl AsFd, buf: &mut [u8]) -> std::io::Result<(usize, Vec<OwnedFd>)> {
    let mut iov = [IoSliceMut::new(buf)];
    let mut recv_space =
        [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(MAX_CHECKPOINT_FDS))];
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
    fn empty_vault() {
        let vault = FdVault::new();
        assert_eq!(vault.device_count(), 0);
        assert!(!vault.has_device("0000:03:00.0"));
        assert!(vault.bdfs().is_empty());
    }

    #[test]
    fn take_from_empty_vault_returns_none() {
        let vault = FdVault::new();
        assert!(vault.take("0000:03:00.0").is_none());
    }

    #[test]
    fn clear_empty_vault_is_safe() {
        let vault = FdVault::new();
        vault.clear();
        assert_eq!(vault.device_count(), 0);
    }

    #[test]
    fn restore_for_bdfs_missing_returns_error() {
        let vault = FdVault::new();
        let result = vault.restore_for_bdfs(&["0000:03:00.0".into()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no vaulted fds"));
    }

    #[test]
    fn restore_for_bdfs_empty_list_succeeds() {
        let vault = FdVault::new();
        let (fds, manifest) = vault.restore_for_bdfs(&[]).unwrap();
        assert!(fds.is_empty());
        let devices = manifest.get("devices").unwrap().as_array().unwrap();
        assert!(devices.is_empty());
    }
}
