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
use coral_ember::observation::SwapObservation;

use crate::error::EmberError;

/// Default ember socket path, overridable via `$CORALREEF_EMBER_SOCKET`.
///
/// Follows wateringHole IPC standard: `$XDG_RUNTIME_DIR/biomeos/coral-ember-<family>.sock`.
fn default_ember_socket() -> String {
    if let Ok(p) = std::env::var("CORALREEF_EMBER_SOCKET") {
        return p;
    }
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let family = std::env::var("CORALREEF_FAMILY_ID")
        .or_else(|_| std::env::var("FAMILY_ID"))
        .unwrap_or_else(|_| "default".to_string());
    format!("{runtime_dir}/biomeos/coral-ember-{family}.sock")
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
    /// Socket path is resolved from `$CORALREEF_EMBER_SOCKET` (fallback: `$XDG_RUNTIME_DIR/biomeos/coral-ember-<family>.sock`).
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

    /// Ask ember to perform a PCI device reset via sysfs.
    ///
    /// This is the SBR path — writes `1` to the sysfs `reset` file for the
    /// device. Required for GPUs without FLR support (e.g. GV100 Titan V).
    pub fn device_reset(&self, bdf: &str, method: &str) -> Result<(), EmberError> {
        let stream = UnixStream::connect(&self.socket_path).map_err(EmberError::Connect)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;

        let req = make_rpc_request(
            "ember.device_reset",
            serde_json::json!({"bdf": bdf, "method": method}),
        );
        std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())?;

        let mut buf = [0u8; MAX_RESPONSE_SIZE];
        let n = std::io::Read::read(&mut &stream, &mut buf)?;
        parse_rpc_response(&buf[..n])?;
        Ok(())
    }

    /// Ask ember to perform a full driver swap (unbind current, bind target).
    ///
    /// Ember handles all sysfs writes and VFIO fd lifecycle. Returns a
    /// [`SwapObservation`] with timing, trace artifacts, and health status.
    ///
    /// Retries transient I/O errors (EAGAIN, EWOULDBLOCK, EINTR) up to 3
    /// times with backoff. Driver swaps can stall briefly while the kernel
    /// settles sysfs state after unbind.
    pub fn swap_device(&self, bdf: &str, target: &str) -> Result<SwapObservation, EmberError> {
        self.swap_device_traced(bdf, target, false)
    }

    /// Like [`swap_device`](Self::swap_device) but with optional mmiotrace capture.
    pub fn swap_device_traced(
        &self,
        bdf: &str,
        target: &str,
        trace: bool,
    ) -> Result<SwapObservation, EmberError> {
        const MAX_RETRIES: u32 = 3;
        const SWAP_TIMEOUT_SECS: u64 = 60;

        let mut last_err = None;
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let backoff = std::time::Duration::from_millis(500 * u64::from(attempt));
                tracing::debug!(
                    bdf,
                    target,
                    attempt,
                    backoff_ms = backoff.as_millis(),
                    "ember swap_device: retrying after transient I/O error"
                );
                std::thread::sleep(backoff);
            }

            match self.try_swap_device(bdf, target, trace, SWAP_TIMEOUT_SECS) {
                Ok(obs) => return Ok(obs),
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
        trace: bool,
        timeout_secs: u64,
    ) -> Result<SwapObservation, EmberError> {
        let stream = UnixStream::connect(&self.socket_path).map_err(EmberError::Connect)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(timeout_secs)))?;

        let mut params = serde_json::json!({"bdf": bdf, "target": target});
        if trace {
            params["trace"] = serde_json::json!(true);
        }
        let req = make_rpc_request("ember.swap", params);
        std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())?;

        let mut buf = vec![0u8; 8192];
        let n = read_full_response(&stream, &mut buf)?;
        let result = parse_rpc_response(&buf[..n])?;
        serde_json::from_value::<SwapObservation>(result.clone()).or_else(|_| {
            // Backward-compat: old ember returning {"bdf", "personality"}
            let personality = result
                .get("personality")
                .or_else(|| result.get("to_personality"))
                .and_then(|v| v.as_str())
                .unwrap_or(target);
            Ok(SwapObservation {
                bdf: bdf.to_string(),
                from_personality: None,
                to_personality: personality.to_string(),
                timestamp_epoch_ms: 0,
                timing: coral_ember::observation::SwapTiming {
                    prepare_ms: 0,
                    unbind_ms: 0,
                    bind_ms: 0,
                    stabilize_ms: 0,
                    total_ms: 0,
                },
                trace_path: None,
                health: coral_ember::observation::HealthResult::Ok,
                lifecycle_description: "unknown (legacy response)".to_string(),
                reset_method_used: None,
                firmware_pre: None,
                firmware_post: None,
            })
        })
    }

    /// Query the experiment journal.
    pub fn journal_query(
        &self,
        filter: &coral_ember::journal::JournalFilter,
    ) -> Result<Vec<coral_ember::journal::JournalEntry>, EmberError> {
        let result = self.simple_rpc(
            "ember.journal.query",
            serde_json::to_value(filter).unwrap_or_default(),
        )?;
        let entries = result
            .get("entries")
            .cloned()
            .unwrap_or(serde_json::Value::Array(vec![]));
        serde_json::from_value(entries).map_err(EmberError::Parse)
    }

    /// Get aggregate journal statistics.
    pub fn journal_stats(
        &self,
        bdf: Option<&str>,
    ) -> Result<coral_ember::journal::JournalStats, EmberError> {
        let params = match bdf {
            Some(b) => serde_json::json!({"bdf": b}),
            None => serde_json::json!({}),
        };
        let result = self.simple_rpc("ember.journal.stats", params)?;
        serde_json::from_value(result).map_err(EmberError::Parse)
    }

    /// Append an entry to the experiment journal (e.g. boot attempt results).
    pub fn journal_append(
        &self,
        entry: &coral_ember::journal::JournalEntry,
    ) -> Result<(), EmberError> {
        let params = serde_json::to_value(entry).map_err(EmberError::Parse)?;
        self.simple_rpc("ember.journal.append", params)?;
        Ok(())
    }

    /// Get ring metadata for a held device.
    pub fn ring_meta_get(&self, bdf: &str) -> Result<coral_ember::RingMeta, EmberError> {
        let result = self.simple_rpc("ember.ring_meta.get", serde_json::json!({"bdf": bdf}))?;
        serde_json::from_value(result).map_err(EmberError::Parse)
    }

    /// Set ring metadata for a held device.
    pub fn ring_meta_set(&self, bdf: &str, meta: &coral_ember::RingMeta) -> Result<(), EmberError> {
        let meta_val = serde_json::to_value(meta).map_err(EmberError::Parse)?;
        self.simple_rpc(
            "ember.ring_meta.set",
            serde_json::json!({"bdf": bdf, "ring_meta": meta_val}),
        )?;
        Ok(())
    }

    /// Read a single BAR0 register via ember's mmap-based MMIO access.
    pub fn mmio_read(&self, bdf: &str, offset: u32) -> Result<u32, EmberError> {
        let result = self.simple_rpc(
            "ember.mmio.read",
            serde_json::json!({"bdf": bdf, "offset": format!("{offset:#x}")}),
        )?;
        let hex = result
            .get("value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EmberError::Rpc {
                code: -32000,
                message: "mmio.read response missing 'value'".into(),
            })?;
        parse_hex_u32(hex).map_err(|e| EmberError::Rpc {
            code: -32000,
            message: format!("mmio.read: {e}"),
        })
    }

    /// Read a structured FECS register snapshot via ember.
    pub fn fecs_state(&self, bdf: &str) -> Result<serde_json::Value, EmberError> {
        self.simple_rpc("ember.fecs.state", serde_json::json!({"bdf": bdf}))
    }

    /// Query livepatch module status.
    pub fn livepatch_status(&self) -> Result<serde_json::Value, EmberError> {
        self.simple_rpc("ember.livepatch.status", serde_json::json!({}))
    }

    /// Enable the livepatch module (loading it if necessary).
    pub fn livepatch_enable(&self) -> Result<serde_json::Value, EmberError> {
        self.simple_rpc("ember.livepatch.enable", serde_json::json!({}))
    }

    /// Disable the livepatch module.
    pub fn livepatch_disable(&self) -> Result<serde_json::Value, EmberError> {
        self.simple_rpc("ember.livepatch.disable", serde_json::json!({}))
    }

    /// Generic one-shot RPC call to ember.
    fn simple_rpc(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, EmberError> {
        let stream = UnixStream::connect(&self.socket_path).map_err(EmberError::Connect)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
        let req = make_rpc_request(method, params);
        std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())?;
        let mut buf = vec![0u8; 65536];
        let n = read_full_response(&stream, &mut buf)?;
        parse_rpc_response(&buf[..n])
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

fn parse_hex_u32(s: &str) -> Result<u32, String> {
    coral_driver::parse_hex_u32(s)
}

// ── BootJournal bridge ───────────────────────────────────────────────

/// Bridges coral-driver's [`BootJournal`](coral_driver::nv::vfio_compute::acr_boot::BootJournal) trait to Ember's JSONL journal
/// via [`EmberClient::journal_append`].
///
/// Constructed with a BDF and ember socket path so the solver can journal
/// every boot attempt without knowing about Ember's existence.
pub struct EmberBootJournal {
    bdf: String,
    socket_path: String,
}

impl EmberBootJournal {
    /// Create a journal bridge for a specific device.
    pub fn new(bdf: impl Into<String>, socket_path: impl Into<String>) -> Self {
        Self {
            bdf: bdf.into(),
            socket_path: socket_path.into(),
        }
    }

    /// Create using the default ember socket path.
    pub fn with_default_socket(bdf: impl Into<String>) -> Self {
        Self::new(bdf, default_ember_socket())
    }
}

impl coral_driver::nv::vfio_compute::acr_boot::BootJournal for EmberBootJournal {
    fn record_boot_attempt(
        &self,
        result: &coral_driver::nv::vfio_compute::acr_boot::AcrBootResult,
    ) {
        let entry = coral_ember::journal::JournalEntry::BootAttempt {
            bdf: self.bdf.clone(),
            strategy: result.strategy.to_string(),
            success: result.success,
            sec2_exci: result.sec2_after.exci,
            fecs_pc: result.fecs_pc_after,
            gpccs_exci: result.gpccs_exci_after,
            notes: result.notes.clone(),
            timestamp_epoch_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        };
        let client = EmberClient {
            socket_path: self.socket_path.clone(),
        };
        if let Err(e) = client.journal_append(&entry) {
            tracing::warn!(bdf = %self.bdf, strategy = result.strategy, "ember journal write failed: {e}");
        }
    }
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
#[path = "ember_test_server.rs"]
mod tests;
