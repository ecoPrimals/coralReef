// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC handlers for device, swap, reset, ring metadata, and status.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, RwLock};

use coral_driver::gsp::RegisterAccess;

use crate::hold::HeldDevice;
use crate::journal::{Journal, JournalEntry};
use crate::swap;
use crate::sysfs;

use super::fd::send_with_fds;
use super::helpers::{require_managed_bdf, try_reset_methods};
use super::jsonrpc::{ipc_io_error_string, make_jsonrpc_ok, write_jsonrpc_error, write_jsonrpc_ok};

pub(crate) fn vfio_fds(
    stream: &mut UnixStream,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    managed_bdfs: &HashSet<String>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), String> {
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

    send_with_fds(&*stream, resp_bytes.as_bytes(), &fds).map_err(|e| format!("sendmsg: {e}"))?;
    tracing::debug!(bdf, backend = ?kind, "sent VFIO fds to client");
    Ok(())
}

/// `ember.vfio_fds` over TCP (`SCM_RIGHTS` requires a Unix domain socket).
pub(crate) fn vfio_fds_unavailable(
    stream: &mut impl Write,
    id: serde_json::Value,
) -> Result<(), String> {
    write_jsonrpc_error(
        stream,
        id,
        -32603,
        "ember.vfio_fds requires Unix socket transport (SCM_RIGHTS)",
    )
    .map_err(ipc_io_error_string)
}

pub(crate) fn list(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
) -> Result<(), String> {
    let map = held.read().map_err(|e| format!("lock poisoned: {e}"))?;
    let devices: Vec<String> = map.keys().cloned().collect();
    drop(map);
    write_jsonrpc_ok(stream, id, serde_json::json!({"devices": devices}))
        .map_err(ipc_io_error_string)
}

pub(crate) fn release(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    managed_bdfs: &HashSet<String>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), String> {
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
                .map_err(ipc_io_error_string)
        }
        None => {
            drop(map);
            write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("device {bdf} not held by ember"),
            )
            .map_err(ipc_io_error_string)
        }
    }
}

pub(crate) fn reacquire(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    managed_bdfs: &HashSet<String>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), String> {
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
        write_jsonrpc_ok(stream, id, serde_json::json!({"bdf": bdf})).map_err(ipc_io_error_string)
    } else {
        // Use guarded open: reacquire can be called for any device including
        // cold-sensitive ones. The guarded path protects the IPC handler
        // thread from entering D-state on unresponsive hardware.
        let lifecycle = crate::vendor_lifecycle::detect_lifecycle(bdf);
        let open_result = if lifecycle.is_cold_sensitive() {
            crate::guarded_open::guarded_vfio_open(bdf, crate::guarded_open::GUARDED_OPEN_TIMEOUT)
                .map_err(|e| e.to_string())
        } else {
            coral_driver::vfio::VfioDevice::open(bdf).map_err(|e| e.to_string())
        };
        match open_result {
            Ok(device) => {
                let req_eventfd = crate::arm_req_irq(&device, bdf);
                tracing::info!(
                    bdf,
                    backend = ?device.backend_kind(),
                    device_fd = device.device_fd(),
                    req_armed = req_eventfd.is_some(),
                    "VFIO device reacquired by ember after swap"
                );
                map.insert(
                    bdf.to_string(),
                    HeldDevice {
                        bdf: bdf.to_string(),
                        device,
                        ring_meta: crate::hold::RingMeta::default(),
                        req_eventfd,
                    },
                );
                drop(map);
                write_jsonrpc_ok(stream, id, serde_json::json!({"bdf": bdf}))
                    .map_err(ipc_io_error_string)
            }
            Err(e) => {
                drop(map);
                tracing::error!(bdf, error = %e, "failed to reacquire VFIO device");
                write_jsonrpc_error(stream, id, -32000, &format!("reacquire failed: {e}"))
                    .map_err(ipc_io_error_string)
            }
        }
    }
}

pub(crate) fn swap(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    managed_bdfs: &HashSet<String>,
    id: serde_json::Value,
    params: &serde_json::Value,
    journal: Option<&Arc<Journal>>,
) -> Result<(), String> {
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
            if let Some(j) = journal
                && let Err(e) = j.append(&JournalEntry::Swap(obs.clone()))
            {
                tracing::warn!(error = %e, "journal append failed for swap");
            }
            let obs_json = serde_json::to_value(&obs).unwrap_or_else(|e| {
                serde_json::json!({"bdf": bdf, "to_personality": obs.to_personality, "error": e.to_string()})
            });
            write_jsonrpc_ok(stream, id, obs_json).map_err(ipc_io_error_string)
        }
        Err(e) => {
            drop(map);
            write_jsonrpc_error(stream, id, -32000, &e).map_err(ipc_io_error_string)
        }
    }
}

pub(crate) fn device_reset(
    stream: &mut impl Write,
    managed_bdfs: &HashSet<String>,
    id: serde_json::Value,
    params: &serde_json::Value,
    journal: Option<&Arc<Journal>>,
) -> Result<(), String> {
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
        "auto" => try_reset_methods(bdf, &methods).map_err(|e| e.to_string()),
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
            .map_err(ipc_io_error_string)
        }
        Err(e) => {
            tracing::error!(bdf, method, error = %e, duration_ms, "PCI device reset failed");
            write_jsonrpc_error(stream, id, -32000, &format!("reset failed: {e}"))
                .map_err(ipc_io_error_string)
        }
    }
}

pub(crate) fn status(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    started_at: std::time::Instant,
) -> Result<(), String> {
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
    .map_err(ipc_io_error_string)
}

pub(crate) fn ring_meta_get(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), String> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or("missing 'bdf' parameter")?;
    let map = held.read().map_err(|e| format!("lock poisoned: {e}"))?;
    if let Some(device) = map.get(bdf) {
        let meta_json = serde_json::to_value(&device.ring_meta).unwrap_or_default();
        drop(map);
        write_jsonrpc_ok(stream, id, meta_json).map_err(ipc_io_error_string)
    } else {
        drop(map);
        write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: not held by ember"))
            .map_err(ipc_io_error_string)
    }
}

pub(crate) fn ring_meta_set(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), String> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or("missing 'bdf' parameter")?;
    let meta_val = params
        .get("ring_meta")
        .ok_or("missing 'ring_meta' parameter")?;
    let meta: crate::hold::RingMeta =
        serde_json::from_value(meta_val.clone()).map_err(|e| format!("invalid ring_meta: {e}"))?;
    let mut map = held.write().map_err(|e| format!("lock poisoned: {e}"))?;
    if let Some(device) = map.get_mut(bdf) {
        device.ring_meta = meta;
        drop(map);
        write_jsonrpc_ok(stream, id, serde_json::json!({"ok": true})).map_err(ipc_io_error_string)
    } else {
        drop(map);
        write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: not held by ember"))
            .map_err(ipc_io_error_string)
    }
}

/// `ember.mmio.read` — read a single BAR0 register via mmap.
pub(crate) fn mmio_read(
    stream: &mut impl Write,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), String> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or("missing 'bdf' parameter")?;
    let offset = parse_hex_or_dec(params.get("offset"), "offset")?;

    let resource0 = format!(
        "{}/resource0",
        coral_driver::linux_paths::sysfs_pci_device_path(bdf)
    );
    let bar0 = match coral_driver::nv::bar0::Bar0Access::open_resource_readonly(&resource0) {
        Ok(b) => b,
        Err(e) => {
            return write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("BAR0 open failed for {bdf}: {e}"),
            )
            .map_err(ipc_io_error_string);
        }
    };

    match bar0.read_u32(offset) {
        Ok(value) => write_jsonrpc_ok(
            stream,
            id,
            serde_json::json!({
                "value": format!("{value:#010x}"),
                "offset": format!("{offset:#010x}"),
            }),
        )
        .map_err(ipc_io_error_string),
        Err(e) => write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("mmio read at {offset:#x}: {e}"),
        )
        .map_err(ipc_io_error_string),
    }
}

/// `ember.fecs.state` — structured FECS register snapshot via BAR0 mmap.
pub(crate) fn fecs_state(
    stream: &mut impl Write,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), String> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or("missing 'bdf' parameter")?;

    let resource0 = format!(
        "{}/resource0",
        coral_driver::linux_paths::sysfs_pci_device_path(bdf)
    );
    let bar0 = match coral_driver::nv::bar0::Bar0Access::open_resource_readonly(&resource0) {
        Ok(b) => b,
        Err(e) => {
            return write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("BAR0 open failed for {bdf}: {e}"),
            )
            .map_err(ipc_io_error_string);
        }
    };

    use coral_driver::nv::bar0::{FECS_CPUCTL, FECS_EXCI, FECS_MB0, FECS_MB1, FECS_PC, FECS_SCTL};

    let read = |off: u32| -> u32 { bar0.read_u32(off).unwrap_or(0xDEAD_DEAD) };

    let cpuctl = read(FECS_CPUCTL);
    let sctl = read(FECS_SCTL);
    let pc = read(FECS_PC);
    let mb0 = read(FECS_MB0);
    let mb1 = read(FECS_MB1);
    let exci = read(FECS_EXCI);

    let halted = cpuctl & (1 << 4) != 0;
    let stopped = cpuctl & (1 << 5) != 0;
    let hs_mode = sctl & 0x2 != 0;

    write_jsonrpc_ok(
        stream,
        id,
        serde_json::json!({
            "cpuctl": format!("{cpuctl:#010x}"),
            "sctl": format!("{sctl:#010x}"),
            "pc": format!("{pc:#010x}"),
            "mb0": format!("{mb0:#010x}"),
            "mb1": format!("{mb1:#010x}"),
            "exci": format!("{exci:#010x}"),
            "halted": halted,
            "stopped": stopped,
            "hs_mode": hs_mode,
        }),
    )
    .map_err(ipc_io_error_string)
}

pub(crate) fn parse_hex_or_dec(val: Option<&serde_json::Value>, name: &str) -> Result<u32, String> {
    let v = val.ok_or(format!("missing '{name}' parameter"))?;
    if let Some(n) = v.as_u64() {
        return u32::try_from(n).map_err(|_| format!("{name} exceeds u32"));
    }
    if let Some(s) = v.as_str() {
        return coral_driver::parse_hex_u32(s).map_err(|e| format!("{name}: {e}"));
    }
    Err(format!("{name}: expected number or hex string"))
}
