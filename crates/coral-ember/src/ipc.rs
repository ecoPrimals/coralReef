// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC 2.0 IPC handler and SCM_RIGHTS fd passing.

use std::collections::{HashMap, HashSet};
use std::io::{ErrorKind, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, RwLock};

use rustix::io::IoSlice;
use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};

use serde::{Deserialize, Serialize};

use crate::hold::HeldDevice;
use crate::journal::{Journal, JournalEntry, JournalFilter};
use crate::swap;
use crate::sysfs;

const MAX_REQUEST_SIZE: usize = 4096;

/// Incoming JSON-RPC 2.0 request line (single object per connection read).
#[derive(Deserialize)]
pub struct JsonRpcRequest {
    /// Must be `"2.0"`.
    pub jsonrpc: String,
    /// Method name (e.g. `ember.list`).
    pub method: String,
    #[serde(default)]
    /// Method parameters object.
    pub params: serde_json::Value,
    /// Request correlation id.
    pub id: serde_json::Value,
}

/// JSON-RPC 2.0 response envelope.
#[derive(Serialize)]
pub struct JsonRpcResponse {
    /// Protocol version (`"2.0"`).
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Success payload.
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Error payload.
    pub error: Option<JsonRpcError>,
    /// Matches the request `id`.
    pub id: serde_json::Value,
}

/// JSON-RPC error object.
#[derive(Serialize)]
pub struct JsonRpcError {
    /// JSON-RPC error code.
    pub code: i32,
    /// Human-readable message.
    pub message: String,
}

fn make_jsonrpc_ok(id: serde_json::Value, result: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        result: Some(result),
        error: None,
        id,
    }
}

fn write_jsonrpc_ok(
    stream: &UnixStream,
    id: serde_json::Value,
    result: serde_json::Value,
) -> std::io::Result<()> {
    let resp = make_jsonrpc_ok(id, result);
    let json =
        serde_json::to_string(&resp).map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e))?;
    let mut w: &UnixStream = stream;
    w.write_all(format!("{json}\n").as_bytes())
}

fn write_jsonrpc_error(
    stream: &UnixStream,
    id: serde_json::Value,
    code: i32,
    message: &str,
) -> std::io::Result<()> {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0",
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.to_string(),
        }),
        id,
    };
    let json =
        serde_json::to_string(&resp).map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e))?;
    let mut w: &UnixStream = stream;
    w.write_all(format!("{json}\n").as_bytes())
}

fn ipc_io_error_string(e: std::io::Error) -> String {
    e.to_string()
}

/// Reject a BDF that is not in the managed set from `glowplug.toml`.
fn require_managed_bdf(
    bdf: &str,
    managed: &HashSet<String>,
    stream: &UnixStream,
    id: serde_json::Value,
) -> Result<(), Result<(), std::io::Error>> {
    if managed.contains(bdf) {
        return Ok(());
    }
    tracing::warn!(bdf, "BDF not in managed allowlist — rejecting RPC");
    let msg = format!(
        "BDF {bdf} is not managed by ember (not listed in glowplug.toml). \
         Only configured devices are accepted."
    );
    write_jsonrpc_error(stream, id, -32001, &msg).map_err(Err)?;
    Err(Ok(()))
}

/// Try reset methods in priority order until one succeeds.
fn try_reset_methods(
    bdf: &str,
    methods: &[crate::vendor_lifecycle::ResetMethod],
) -> Result<(), String> {
    let mut last_err = String::new();
    for m in methods {
        let label = match m {
            crate::vendor_lifecycle::ResetMethod::BridgeSbr => "bridge-sbr",
            crate::vendor_lifecycle::ResetMethod::SysfsSbr => "sbr",
            crate::vendor_lifecycle::ResetMethod::RemoveRescan => "remove-rescan",
            crate::vendor_lifecycle::ResetMethod::VfioFlr => {
                last_err =
                    "FLR not available via ember (use GlowPlug device.reset)".to_string();
                continue;
            }
        };
        tracing::info!(bdf, method = label, "trying reset method");
        let result = match m {
            crate::vendor_lifecycle::ResetMethod::BridgeSbr => sysfs::pci_bridge_reset(bdf),
            crate::vendor_lifecycle::ResetMethod::SysfsSbr => sysfs::pci_device_reset(bdf),
            crate::vendor_lifecycle::ResetMethod::RemoveRescan => sysfs::pci_remove_rescan(bdf),
            crate::vendor_lifecycle::ResetMethod::VfioFlr => unreachable!(),
        };
        match result {
            Ok(()) => return Ok(()),
            Err(e) => {
                tracing::warn!(bdf, method = label, error = %e, "reset method failed, trying next");
                last_err = format!("{label}: {e}");
            }
        }
    }
    Err(last_err)
}

/// Handle one JSON-RPC request on `stream` (read one line, dispatch, write response).
///
/// For `ember.vfio_fds`, sends the JSON line first, then passes fds via `SCM_RIGHTS`.
///
/// # Errors
///
/// Returns `Err` when a required parameter is missing for a method that uses `?` (e.g. `ember.swap`
/// without `target`); socket write/serialize errors are returned as `Err` strings (including I/O
/// errors from writing JSON-RPC responses).
pub fn handle_client(
    stream: &UnixStream,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    managed_bdfs: &HashSet<String>,
    started_at: std::time::Instant,
    journal: Option<&Arc<Journal>>,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let mut buf = [0u8; MAX_REQUEST_SIZE];
    let n = std::io::Read::read(&mut &*stream, &mut buf).map_err(|e| format!("read: {e}"))?;
    if n == 0 {
        return Ok(());
    }

    let line = std::str::from_utf8(&buf[..n]).map_err(|e| format!("utf8: {e}"))?;
    let line = line.trim();

    let req: JsonRpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            write_jsonrpc_error(
                stream,
                serde_json::Value::Null,
                -32700,
                &format!("parse error: {e}"),
            )
            .map_err(ipc_io_error_string)?;
            return Ok(());
        }
    };

    if req.jsonrpc != "2.0" {
        write_jsonrpc_error(
            stream,
            req.id,
            -32600,
            &format!("invalid jsonrpc version: {}", req.jsonrpc),
        )
        .map_err(ipc_io_error_string)?;
        return Ok(());
    }

    let id = req.id;
    let params = &req.params;

    match req.method.as_str() {
        "ember.vfio_fds" => {
            let bdf = params
                .get("bdf")
                .and_then(|v| v.as_str())
                .ok_or("missing 'bdf' parameter")?;
            match require_managed_bdf(bdf, managed_bdfs, stream, id.clone()) {
                Ok(()) => {}
                Err(early) => return early.map_err(ipc_io_error_string),
            }
            let map = held.read().map_err(|e| format!("lock poisoned: {e}"))?;
            let dev = match map.get(bdf) {
                Some(d) => d,
                None => {
                    drop(map);
                    write_jsonrpc_error(
                        stream,
                        id,
                        -32000,
                        &format!("device {bdf} not held by ember"),
                    )
                    .map_err(ipc_io_error_string)?;
                    return Ok(());
                }
            };

            let fds = dev.device.sendable_fds();
            let kind = dev.device.backend_kind();

            let mut result = serde_json::json!({
                "bdf": bdf,
                "num_fds": fds.len(),
            });
            match kind {
                coral_driver::vfio::VfioBackendKind::Legacy => {
                    result["backend"] = serde_json::json!("legacy");
                }
                coral_driver::vfio::VfioBackendKind::Iommufd { ioas_id } => {
                    result["backend"] = serde_json::json!("iommufd");
                    result["ioas_id"] = serde_json::json!(ioas_id);
                }
            }

            let resp = make_jsonrpc_ok(id, result);
            let resp_bytes = format!(
                "{}\n",
                serde_json::to_string(&resp).map_err(|e| format!("serialize: {e}"))?
            );

            send_with_fds(stream, resp_bytes.as_bytes(), &fds)
                .map_err(|e| format!("sendmsg: {e}"))?;
            tracing::debug!(bdf, backend = ?kind, "sent VFIO fds to client");
        }
        "ember.list" => {
            let map = held.read().map_err(|e| format!("lock poisoned: {e}"))?;
            let devices: Vec<String> = map.keys().cloned().collect();
            drop(map);
            write_jsonrpc_ok(stream, id, serde_json::json!({"devices": devices}))
                .map_err(ipc_io_error_string)?;
        }
        "ember.release" => {
            let bdf = params
                .get("bdf")
                .and_then(|v| v.as_str())
                .ok_or("missing 'bdf' parameter")?;
            match require_managed_bdf(bdf, managed_bdfs, stream, id.clone()) {
                Ok(()) => {}
                Err(early) => return early.map_err(ipc_io_error_string),
            }
            let mut map = held.write().map_err(|e| format!("lock poisoned: {e}"))?;
            match map.remove(bdf) {
                Some(device) => {
                    drop(device);
                    tracing::info!(bdf, "ember released VFIO fds for swap");
                    drop(map);
                    write_jsonrpc_ok(stream, id, serde_json::json!({"bdf": bdf}))
                        .map_err(ipc_io_error_string)?;
                }
                None => {
                    drop(map);
                    write_jsonrpc_error(
                        stream,
                        id,
                        -32000,
                        &format!("device {bdf} not held by ember"),
                    )
                    .map_err(ipc_io_error_string)?;
                }
            }
        }
        "ember.reacquire" => {
            let bdf = params
                .get("bdf")
                .and_then(|v| v.as_str())
                .ok_or("missing 'bdf' parameter")?;
            match require_managed_bdf(bdf, managed_bdfs, stream, id.clone()) {
                Ok(()) => {}
                Err(early) => return early.map_err(ipc_io_error_string),
            }
            if sysfs::is_d3cold(bdf) {
                write_jsonrpc_error(
                    stream,
                    id,
                    -32000,
                    &format!("{bdf} is D3cold — cannot reacquire"),
                )
                .map_err(ipc_io_error_string)?;
                return Ok(());
            }
            let mut map = held.write().map_err(|e| format!("lock poisoned: {e}"))?;
            if map.contains_key(bdf) {
                tracing::warn!(bdf, "device already held — skipping reacquire");
                drop(map);
                write_jsonrpc_ok(stream, id, serde_json::json!({"bdf": bdf}))
                    .map_err(ipc_io_error_string)?;
            } else {
                match coral_driver::vfio::VfioDevice::open(bdf) {
                    Ok(device) => {
                        tracing::info!(
                            bdf,
                            backend = ?device.backend_kind(),
                            device_fd = device.device_fd(),
                            "VFIO device reacquired by ember after swap"
                        );
                        map.insert(
                            bdf.to_string(),
                            HeldDevice {
                                bdf: bdf.to_string(),
                                device,
                                ring_meta: crate::hold::RingMeta::default(),
                            },
                        );
                        drop(map);
                        write_jsonrpc_ok(stream, id, serde_json::json!({"bdf": bdf}))
                            .map_err(ipc_io_error_string)?;
                    }
                    Err(e) => {
                        drop(map);
                        tracing::error!(bdf, error = %e, "failed to reacquire VFIO device");
                        write_jsonrpc_error(stream, id, -32000, &format!("reacquire failed: {e}"))
                            .map_err(ipc_io_error_string)?;
                    }
                }
            }
        }
        "ember.swap" => {
            let bdf = params
                .get("bdf")
                .and_then(|v| v.as_str())
                .ok_or("missing 'bdf' parameter")?;
            let target = params
                .get("target")
                .and_then(|v| v.as_str())
                .ok_or("missing 'target' parameter")?;
            let enable_trace = params
                .get("trace")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            match require_managed_bdf(bdf, managed_bdfs, stream, id.clone()) {
                Ok(()) => {}
                Err(early) => return early.map_err(ipc_io_error_string),
            }
            if sysfs::is_d3cold(bdf) {
                write_jsonrpc_error(
                    stream,
                    id,
                    -32000,
                    &format!("{bdf} is D3cold — cannot swap"),
                )
                .map_err(ipc_io_error_string)?;
                return Ok(());
            }
            let mut map = held.write().map_err(|e| format!("lock poisoned: {e}"))?;
            match swap::handle_swap_device_with_journal(bdf, target, &mut map, enable_trace, journal) {
                Ok(obs) => {
                    drop(map);
                    if let Some(j) = journal {
                        if let Err(e) = j.append(&JournalEntry::Swap(obs.clone())) {
                            tracing::warn!(error = %e, "journal append failed for swap");
                        }
                    }
                    let obs_json = serde_json::to_value(&obs).unwrap_or_else(|e| {
                        serde_json::json!({"bdf": bdf, "to_personality": obs.to_personality, "error": e.to_string()})
                    });
                    write_jsonrpc_ok(stream, id, obs_json)
                        .map_err(ipc_io_error_string)?;
                }
                Err(e) => {
                    drop(map);
                    write_jsonrpc_error(stream, id, -32000, &e).map_err(ipc_io_error_string)?;
                }
            }
        }
        "ember.device_reset" => {
            let bdf = params
                .get("bdf")
                .and_then(|v| v.as_str())
                .ok_or("missing 'bdf' parameter")?;
            let method = params
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("auto");
            match require_managed_bdf(bdf, managed_bdfs, stream, id.clone()) {
                Ok(()) => {}
                Err(early) => return early.map_err(ipc_io_error_string),
            }
            if sysfs::is_d3cold(bdf) {
                write_jsonrpc_error(
                    stream,
                    id,
                    -32000,
                    &format!("{bdf} is D3cold — cannot reset"),
                )
                .map_err(ipc_io_error_string)?;
                return Ok(());
            }

            let lifecycle = crate::vendor_lifecycle::detect_lifecycle(bdf);
            let methods = lifecycle.available_reset_methods();
            tracing::info!(
                bdf,
                method,
                available = ?methods,
                "ember.device_reset: starting"
            );

            let reset_start = std::time::Instant::now();
            let result = match method {
                "sbr" => sysfs::pci_device_reset(bdf),
                "bridge-sbr" => sysfs::pci_bridge_reset(bdf),
                "remove-rescan" => sysfs::pci_remove_rescan(bdf),
                "auto" => try_reset_methods(bdf, &methods),
                other => Err(format!(
                    "unknown reset method: {other} (use 'auto', 'sbr', 'bridge-sbr', 'remove-rescan')"
                )),
            };
            let duration_ms = reset_start.elapsed().as_millis() as u64;

            let (success, error_msg) = match &result {
                Ok(()) => (true, None),
                Err(e) => (false, Some(e.clone())),
            };

            if let Some(j) = journal {
                let obs = crate::observation::ResetObservation {
                    bdf: bdf.to_string(),
                    method: method.to_string(),
                    success,
                    error: error_msg.clone(),
                    timestamp_epoch_ms: crate::observation::epoch_ms(),
                    duration_ms,
                };
                if let Err(e) = j.append(&JournalEntry::Reset(obs)) {
                    tracing::warn!(error = %e, "journal append failed for reset");
                }
            }

            match result {
                Ok(()) => {
                    tracing::info!(bdf, method, duration_ms, "PCI device reset complete");
                    write_jsonrpc_ok(
                        stream,
                        id,
                        serde_json::json!({
                            "bdf": bdf,
                            "reset": true,
                            "method": method,
                            "duration_ms": duration_ms,
                        }),
                    )
                    .map_err(ipc_io_error_string)?;
                }
                Err(e) => {
                    tracing::error!(bdf, method, error = %e, duration_ms, "PCI device reset failed");
                    write_jsonrpc_error(stream, id, -32000, &format!("reset failed: {e}"))
                        .map_err(ipc_io_error_string)?;
                }
            }
        }
        "ember.status" => {
            let map = held.read().map_err(|e| format!("lock poisoned: {e}"))?;
            let devices: Vec<String> = map.keys().cloned().collect();
            drop(map);
            write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({
                    "devices": devices,
                    "uptime_secs": started_at.elapsed().as_secs(),
                }),
            )
            .map_err(ipc_io_error_string)?;
        }
        "ember.journal.query" => {
            if let Some(j) = journal {
                let filter: JournalFilter = serde_json::from_value(params.clone()).unwrap_or_default();
                match j.query(&filter) {
                    Ok(entries) => {
                        write_jsonrpc_ok(stream, id, serde_json::json!({"entries": entries}))
                            .map_err(ipc_io_error_string)?;
                    }
                    Err(e) => {
                        write_jsonrpc_error(stream, id, -32000, &format!("journal query: {e}"))
                            .map_err(ipc_io_error_string)?;
                    }
                }
            } else {
                write_jsonrpc_error(stream, id, -32000, "journal not available")
                    .map_err(ipc_io_error_string)?;
            }
        }
        "ember.journal.stats" => {
            if let Some(j) = journal {
                let bdf = params.get("bdf").and_then(|v| v.as_str());
                match j.stats(bdf) {
                    Ok(stats) => {
                        let stats_json = serde_json::to_value(&stats).unwrap_or_default();
                        write_jsonrpc_ok(stream, id, stats_json)
                            .map_err(ipc_io_error_string)?;
                    }
                    Err(e) => {
                        write_jsonrpc_error(stream, id, -32000, &format!("journal stats: {e}"))
                            .map_err(ipc_io_error_string)?;
                    }
                }
            } else {
                write_jsonrpc_error(stream, id, -32000, "journal not available")
                    .map_err(ipc_io_error_string)?;
            }
        }
        "ember.journal.append" => {
            if let Some(j) = journal {
                match serde_json::from_value::<JournalEntry>(params.clone()) {
                    Ok(entry) => match j.append(&entry) {
                        Ok(()) => {
                            write_jsonrpc_ok(stream, id, serde_json::json!({"ok": true}))
                                .map_err(ipc_io_error_string)?;
                        }
                        Err(e) => {
                            write_jsonrpc_error(
                                stream,
                                id,
                                -32000,
                                &format!("journal append: {e}"),
                            )
                            .map_err(ipc_io_error_string)?;
                        }
                    },
                    Err(e) => {
                        write_jsonrpc_error(
                            stream,
                            id,
                            -32602,
                            &format!("invalid journal entry: {e}"),
                        )
                        .map_err(ipc_io_error_string)?;
                    }
                }
            } else {
                write_jsonrpc_error(stream, id, -32000, "journal not available")
                    .map_err(ipc_io_error_string)?;
            }
        }
        "ember.ring_meta.get" => {
            let bdf = params
                .get("bdf")
                .and_then(|v| v.as_str())
                .ok_or("missing 'bdf' parameter")?;
            let map = held.read().map_err(|e| format!("lock poisoned: {e}"))?;
            if let Some(device) = map.get(bdf) {
                let meta_json = serde_json::to_value(&device.ring_meta).unwrap_or_default();
                drop(map);
                write_jsonrpc_ok(stream, id, meta_json).map_err(ipc_io_error_string)?;
            } else {
                drop(map);
                write_jsonrpc_error(
                    stream,
                    id,
                    -32000,
                    &format!("{bdf}: not held by ember"),
                )
                .map_err(ipc_io_error_string)?;
            }
        }
        "ember.ring_meta.set" => {
            let bdf = params
                .get("bdf")
                .and_then(|v| v.as_str())
                .ok_or("missing 'bdf' parameter")?;
            let meta_val = params
                .get("ring_meta")
                .ok_or("missing 'ring_meta' parameter")?;
            let meta: crate::hold::RingMeta = serde_json::from_value(meta_val.clone())
                .map_err(|e| format!("invalid ring_meta: {e}"))?;
            let mut map = held.write().map_err(|e| format!("lock poisoned: {e}"))?;
            if let Some(device) = map.get_mut(bdf) {
                device.ring_meta = meta;
                drop(map);
                write_jsonrpc_ok(stream, id, serde_json::json!({"ok": true}))
                    .map_err(ipc_io_error_string)?;
            } else {
                drop(map);
                write_jsonrpc_error(
                    stream,
                    id,
                    -32000,
                    &format!("{bdf}: not held by ember"),
                )
                .map_err(ipc_io_error_string)?;
            }
        }
        other => {
            write_jsonrpc_error(stream, id, -32601, &format!("method not found: {other}"))
                .map_err(ipc_io_error_string)?;
        }
    }

    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::os::fd::AsFd;
    use std::os::unix::net::UnixStream;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    static IPC_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn empty_held() -> Arc<RwLock<HashMap<String, HeldDevice>>> {
        Arc::new(RwLock::new(HashMap::new()))
    }

    fn managed(bdfs: &[&str]) -> HashSet<String> {
        bdfs.iter().map(|s| (*s).to_string()).collect()
    }

    const TEST_BDF: &str = "0000:01:00.0";
    const BOGUS_BDF: &str = "9999:99:99.9";

    fn drain_json_line(stream: &mut UnixStream) -> serde_json::Value {
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            stream.read_exact(&mut byte).expect("read response byte");
            if byte[0] == b'\n' {
                break;
            }
            buf.push(byte[0]);
        }
        let s = std::str::from_utf8(&buf).expect("response is utf-8");
        serde_json::from_str(s).expect("drained line is json")
    }

    #[test]
    fn handle_client_empty_read_returns_ok() {
        let (a, b) = UnixStream::pair().expect("unix stream pair");
        drop(b);
        let held = empty_held();
        let m = managed(&[]);
        handle_client(&a, &held, &m, Instant::now(), None).expect("handle_client completes");
        drop(a);
    }

    #[test]
    fn handle_client_invalid_json_emits_parse_error() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        client
            .write_all(b"not json\n")
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[]);
        handle_client(&server, &held, &m, Instant::now(), None).expect("handle_client completes");
        let v = drain_json_line(&mut client);
        assert_eq!(
            v["error"]["code"].as_i64().expect("jsonrpc error code"),
            -32700
        );
    }

    #[test]
    fn handle_client_wrong_jsonrpc_version() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req = r#"{"jsonrpc":"1.0","method":"ember.list","id":1}"#;
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[]);
        handle_client(&server, &held, &m, Instant::now(), None).expect("handle_client completes");
        let v = drain_json_line(&mut client);
        assert_eq!(
            v["error"]["code"].as_i64().expect("jsonrpc error code"),
            -32600
        );
    }

    #[test]
    fn handle_client_ember_list_empty() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req = r#"{"jsonrpc":"2.0","method":"ember.list","id":7}"#;
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[]);
        handle_client(&server, &held, &m, Instant::now(), None).expect("handle_client completes");
        let v = drain_json_line(&mut client);
        assert_eq!(v["result"]["devices"], serde_json::json!([]));
    }

    #[test]
    fn handle_client_ember_status_reports_uptime() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req = r#"{"jsonrpc":"2.0","method":"ember.status","id":2}"#;
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let started = Instant::now() - Duration::from_secs(10);
        let held = empty_held();
        let m = managed(&[]);
        handle_client(&server, &held, &m, started, None).expect("handle_client completes");
        let v = drain_json_line(&mut client);
        let uptime = v["result"]["uptime_secs"].as_u64().expect("uptime field");
        assert!(uptime >= 10);
    }

    #[test]
    fn handle_client_unknown_method() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req = r#"{"jsonrpc":"2.0","method":"nope.not_found","id":3}"#;
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[]);
        handle_client(&server, &held, &m, Instant::now(), None).expect("handle_client completes");
        let v = drain_json_line(&mut client);
        assert_eq!(
            v["error"]["code"].as_i64().expect("jsonrpc error code"),
            -32601
        );
    }

    #[test]
    fn handle_client_ember_vfio_fds_missing_device() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req =
            r#"{"jsonrpc":"2.0","method":"ember.vfio_fds","params":{"bdf":"0000:01:00.0"},"id":4}"#;
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[TEST_BDF]);
        handle_client(&server, &held, &m, Instant::now(), None).expect("handle_client completes");
        let v = drain_json_line(&mut client);
        assert_eq!(
            v["error"]["code"].as_i64().expect("jsonrpc error code"),
            -32000
        );
    }

    #[test]
    fn handle_client_ember_vfio_fds_missing_bdf_errors() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req = r#"{"jsonrpc":"2.0","method":"ember.vfio_fds","params":{},"id":501}"#;
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[TEST_BDF]);
        let err =
            handle_client(&server, &held, &m, Instant::now(), None).expect_err("handler returns error");
        assert!(err.contains("bdf"), "{err}");
    }

    #[test]
    fn handle_client_ember_reacquire_missing_bdf_errors() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req = r#"{"jsonrpc":"2.0","method":"ember.reacquire","params":{},"id":502}"#;
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[TEST_BDF]);
        let err =
            handle_client(&server, &held, &m, Instant::now(), None).expect_err("handler returns error");
        assert!(err.contains("bdf"), "{err}");
    }

    #[test]
    fn handle_client_ember_release_missing_bdf_errors() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req = r#"{"jsonrpc":"2.0","method":"ember.release","params":{},"id":5}"#;
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[]);
        let err =
            handle_client(&server, &held, &m, Instant::now(), None).expect_err("handler returns error");
        assert!(err.contains("bdf"));
    }

    #[test]
    fn handle_client_ember_release_not_held() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req =
            r#"{"jsonrpc":"2.0","method":"ember.release","params":{"bdf":"0000:01:00.0"},"id":6}"#;
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[TEST_BDF]);
        handle_client(&server, &held, &m, Instant::now(), None).expect("handle_client completes");
        let v = drain_json_line(&mut client);
        assert_eq!(
            v["error"]["code"].as_i64().expect("jsonrpc error code"),
            -32000
        );
    }

    #[test]
    fn handle_client_ember_swap_missing_params() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req =
            r#"{"jsonrpc":"2.0","method":"ember.swap","params":{"bdf":"0000:01:00.0"},"id":8}"#;
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[TEST_BDF]);
        let err =
            handle_client(&server, &held, &m, Instant::now(), None).expect_err("handler returns error");
        assert!(err.contains("target"));
    }

    #[test]
    fn handle_client_ember_reacquire_open_failure_returns_jsonrpc_error() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req = r#"{"jsonrpc":"2.0","method":"ember.reacquire","params":{"bdf":"9999:99:99.9"},"id":11}"#;
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[BOGUS_BDF]);
        handle_client(&server, &held, &m, Instant::now(), None).expect("handle_client completes");
        let v = drain_json_line(&mut client);
        assert_eq!(
            v["error"]["code"].as_i64().expect("jsonrpc error code"),
            -32000
        );
        let msg = v["error"]["message"]
            .as_str()
            .expect("jsonrpc error message");
        assert!(msg.contains("reacquire"));
    }

    #[test]
    fn handle_client_ember_swap_unbound_success() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req = r#"{"jsonrpc":"2.0","method":"ember.swap","params":{"bdf":"9999:99:99.9","target":"unbound"},"id":42}"#;
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[BOGUS_BDF]);
        handle_client(&server, &held, &m, Instant::now(), None).expect("handle_client completes");
        let v = drain_json_line(&mut client);
        assert_eq!(v["result"]["to_personality"], "unbound");
        assert_eq!(v["id"], serde_json::json!(42));
    }

    #[test]
    fn handle_client_non_utf8_request_errors() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        client
            .write_all(&[0xff, 0xfe, b'\n'])
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[]);
        let err =
            handle_client(&server, &held, &m, Instant::now(), None).expect_err("handler returns error");
        assert!(err.contains("utf8"), "{err}");
    }

    #[test]
    fn send_with_fds_fails_when_peer_closed() {
        let (a, b) = UnixStream::pair().expect("unix stream pair");
        drop(b);
        let file = std::fs::File::open("/dev/null").expect("open /dev/null");
        let fds = [file.as_fd()];
        let err = send_with_fds(&a, b"{}", &fds).expect_err("broken pipe");
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn handle_client_ember_swap_reports_error_from_swap() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req = r#"{"jsonrpc":"2.0","method":"ember.swap","params":{"bdf":"9999:99:99.9","target":"bogus-target"},"id":9}"#;
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[BOGUS_BDF]);
        handle_client(&server, &held, &m, Instant::now(), None).expect("handle_client completes");
        let v = drain_json_line(&mut client);
        assert_eq!(
            v["error"]["code"].as_i64().expect("jsonrpc error code"),
            -32000
        );
        let msg = v["error"]["message"]
            .as_str()
            .expect("jsonrpc error message");
        assert!(
            msg.contains("preflight") || msg.contains("unknown target"),
            "expected preflight or unknown target error, got: {msg}"
        );
    }

    #[test]
    fn handle_client_swap_rejects_unmanaged_bdf() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req = r#"{"jsonrpc":"2.0","method":"ember.swap","params":{"bdf":"0000:ff:00.0","target":"vfio"},"id":99}"#;
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[TEST_BDF]);
        handle_client(&server, &held, &m, Instant::now(), None).expect("handle_client completes");
        let v = drain_json_line(&mut client);
        assert_eq!(
            v["error"]["code"].as_i64().expect("jsonrpc error code"),
            -32001
        );
        let msg = v["error"]["message"]
            .as_str()
            .expect("jsonrpc error message");
        assert!(msg.contains("not managed"), "{msg}");
    }

    #[test]
    fn handle_client_reacquire_rejects_unmanaged_bdf() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req = r#"{"jsonrpc":"2.0","method":"ember.reacquire","params":{"bdf":"0000:ff:00.0"},"id":100}"#;
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[TEST_BDF]);
        handle_client(&server, &held, &m, Instant::now(), None).expect("handle_client completes");
        let v = drain_json_line(&mut client);
        assert_eq!(
            v["error"]["code"].as_i64().expect("jsonrpc error code"),
            -32001
        );
    }

    #[test]
    fn handle_client_ember_vfio_fds_rejects_unmanaged_bdf() {
        let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req = r#"{"jsonrpc":"2.0","method":"ember.vfio_fds","params":{"bdf":"0000:ff:00.0"},"id":101}"#;
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let held = empty_held();
        let m = managed(&[TEST_BDF]);
        handle_client(&server, &held, &m, Instant::now(), None).expect("handle_client completes");
        let v = drain_json_line(&mut client);
        assert_eq!(
            v["error"]["code"].as_i64().expect("jsonrpc error code"),
            -32001
        );
        let msg = v["error"]["message"]
            .as_str()
            .expect("jsonrpc error message");
        assert!(msg.contains("not managed"), "{msg}");
    }

    #[test]
    fn jsonrpc_request_deserializes_omitted_params_as_null() {
        let json = r#"{"jsonrpc":"2.0","method":"ember.list","id":1}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).expect("deserialize JsonRpcRequest");
        assert_eq!(req.params, serde_json::Value::Null);
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.method, "ember.list");
    }

    #[test]
    fn write_jsonrpc_error_writes_line_client_can_parse() {
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        write_jsonrpc_error(
            &server,
            serde_json::json!("correlation-id"),
            -32000,
            "test failure message",
        )
        .expect("write_jsonrpc_error");
        let v = drain_json_line(&mut client);
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["error"]["code"], -32000);
        assert_eq!(v["error"]["message"], "test failure message");
        assert_eq!(v["id"], serde_json::json!("correlation-id"));
    }

    #[test]
    fn make_jsonrpc_ok_shape() {
        let v = make_jsonrpc_ok(serde_json::json!(1), serde_json::json!({"a": 1}));
        assert_eq!(v.jsonrpc, "2.0");
        assert!(v.error.is_none());
        assert_eq!(v.result, Some(serde_json::json!({"a": 1})));
    }

    #[test]
    fn send_with_fds_unix_stream() {
        let (a, _b) = UnixStream::pair().expect("unix stream pair");
        let file = std::fs::File::open("/dev/null").expect("open /dev/null");
        let fds = [file.as_fd()];
        send_with_fds(&a, b"ok", &fds).expect("send_with_fds with /dev/null fd");
    }

    #[test]
    #[ignore = "requires GPU bound to vfio-pci and a real BDF"]
    fn handle_client_ember_vfio_fds_with_hardware() {
        let bdf = std::env::var("CORAL_EMBER_TEST_BDF").expect("set CORAL_EMBER_TEST_BDF");
        let (server, mut client) = UnixStream::pair().expect("unix stream pair");
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"ember.vfio_fds","params":{{"bdf":"{bdf}"}},"id":1}}"#
        );
        client
            .write_all(req.as_bytes())
            .expect("write request to test socket");
        client
            .write_all(b"\n")
            .expect("write request to test socket");
        let device =
            coral_driver::vfio::VfioDevice::open(&bdf).expect("open vfio device for hw test");
        let mut map = HashMap::new();
        map.insert(
            bdf.clone(),
            crate::hold::HeldDevice {
                bdf: bdf.clone(),
                device,
                ring_meta: crate::hold::RingMeta::default(),
            },
        );
        let held = Arc::new(RwLock::new(map));
        let m = managed(&[&bdf]);
        handle_client(&server, &held, &m, Instant::now(), None).expect("handle_client completes");
    }
}
