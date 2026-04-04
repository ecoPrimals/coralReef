// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC handlers for device, swap, reset, ring metadata, and status.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, RwLock};

use crate::error::{EmberIpcError, SwapError};
use crate::hold::HeldDevice;
use crate::journal::{Journal, JournalEntry};
use crate::swap;
use crate::sysfs;

use super::fd::send_with_fds;
use super::helpers::{finish_managed_bdf_early, require_managed_bdf, try_reset_methods};
use super::jsonrpc::{make_jsonrpc_ok, write_jsonrpc_error, write_jsonrpc_ok};

pub(crate) fn vfio_fds(
    stream: &mut UnixStream,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    managed_bdfs: &HashSet<String>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;
    finish_managed_bdf_early(require_managed_bdf(bdf, managed_bdfs, stream, &id))?;
    let map = held.read().map_err(|_| EmberIpcError::LockPoisoned)?;
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
            .map_err(EmberIpcError::from)?;
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
        serde_json::to_string(&resp)
            .map_err(|e| EmberIpcError::JsonSerialize(format!("serialize: {e}")))?
    );

    send_with_fds(&*stream, resp_bytes.as_bytes(), &fds).map_err(EmberIpcError::from)?;
    tracing::debug!(bdf, backend = ?kind, "sent VFIO fds to client");
    Ok(())
}

/// `ember.vfio_fds` over TCP (`SCM_RIGHTS` requires a Unix domain socket).
pub(crate) fn vfio_fds_unavailable(
    stream: &mut impl Write,
    id: serde_json::Value,
) -> Result<(), EmberIpcError> {
    write_jsonrpc_error(
        stream,
        id,
        -32603,
        "ember.vfio_fds requires Unix socket transport (SCM_RIGHTS)",
    )
    .map_err(EmberIpcError::from)
}

pub(crate) fn list(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
) -> Result<(), EmberIpcError> {
    let map = held.read().map_err(|_| EmberIpcError::LockPoisoned)?;
    let devices: Vec<String> = map.keys().cloned().collect();
    drop(map);
    write_jsonrpc_ok(stream, id, serde_json::json!({"devices": devices}))
        .map_err(EmberIpcError::from)
}

pub(crate) fn release(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    managed_bdfs: &HashSet<String>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;
    finish_managed_bdf_early(require_managed_bdf(bdf, managed_bdfs, stream, &id))?;
    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    match map.remove(bdf) {
        Some(device) => {
            drop(device);
            tracing::info!(bdf, "ember released VFIO fds for swap");
            drop(map);
            write_jsonrpc_ok(stream, id, serde_json::json!({"bdf": bdf}))
                .map_err(EmberIpcError::from)
        }
        None => {
            drop(map);
            write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("device {bdf} not held by ember"),
            )
            .map_err(EmberIpcError::from)
        }
    }
}

pub(crate) fn reacquire(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    managed_bdfs: &HashSet<String>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;
    finish_managed_bdf_early(require_managed_bdf(bdf, managed_bdfs, stream, &id))?;
    if sysfs::is_d3cold(bdf) {
        write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("{bdf} is D3cold — cannot reacquire"),
        )
        .map_err(EmberIpcError::from)?;
        return Ok(());
    }
    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    if map.contains_key(bdf) {
        tracing::warn!(bdf, "device already held — skipping reacquire");
        drop(map);
        write_jsonrpc_ok(stream, id, serde_json::json!({"bdf": bdf})).map_err(EmberIpcError::from)
    } else {
        match coral_driver::vfio::VfioDevice::open(bdf) {
            Ok(device) => {
                // Bus master stays OFF — ember never does DMA.
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
                        experiment_dirty: false,
                        dma_prepare_state: None,
                    },
                );
                drop(map);
                write_jsonrpc_ok(stream, id, serde_json::json!({"bdf": bdf}))
                    .map_err(EmberIpcError::from)
            }
            Err(e) => {
                drop(map);
                tracing::error!(bdf, error = %e, "failed to reacquire VFIO device");
                write_jsonrpc_error(stream, id, -32000, &format!("reacquire failed: {e}"))
                    .map_err(EmberIpcError::from)
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
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;
    let target = params
        .get("target")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'target' parameter"))?;
    let enable_trace = params
        .get("trace")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    finish_managed_bdf_early(require_managed_bdf(bdf, managed_bdfs, stream, &id))?;
    if sysfs::is_d3cold(bdf) {
        write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("{bdf} is D3cold — cannot swap"),
        )
        .map_err(EmberIpcError::from)?;
        return Ok(());
    }
    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
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
            write_jsonrpc_ok(stream, id, obs_json).map_err(EmberIpcError::from)
        }
        Err(e) => {
            drop(map);
            write_jsonrpc_error(stream, id, -32000, &e.to_string()).map_err(EmberIpcError::from)
        }
    }
}

pub(crate) fn device_reset(
    stream: &mut impl Write,
    managed_bdfs: &HashSet<String>,
    id: serde_json::Value,
    params: &serde_json::Value,
    journal: Option<&Arc<Journal>>,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;
    let method = params
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("auto");
    finish_managed_bdf_early(require_managed_bdf(bdf, managed_bdfs, stream, &id))?;
    if sysfs::is_d3cold(bdf) {
        write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("{bdf} is D3cold — cannot reset"),
        )
        .map_err(EmberIpcError::from)?;
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
    let result: Result<(), SwapError> = match method {
        "sbr" => sysfs::pci_device_reset(bdf).map_err(Into::into),
        "bridge-sbr" => sysfs::pci_bridge_reset(bdf).map_err(Into::into),
        "remove-rescan" => sysfs::pci_remove_rescan(bdf).map_err(Into::into),
        "auto" => try_reset_methods(bdf, &methods).map_err(Into::into),
        other => Err(SwapError::InvalidResetMethod(format!(
            "{other} (use 'auto', 'sbr', 'bridge-sbr', 'remove-rescan')"
        ))),
    };
    let duration_ms = reset_start.elapsed().as_millis() as u64;

    if let Some(j) = journal {
        let obs = crate::observation::ResetObservation {
            bdf: bdf.to_string(),
            method: method.to_string(),
            success: result.is_ok(),
            error: result.as_ref().err().map(ToString::to_string),
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
            .map_err(EmberIpcError::from)
        }
        Err(e) => {
            tracing::error!(bdf, method, error = %e, duration_ms, "PCI device reset failed");
            write_jsonrpc_error(stream, id, -32000, &format!("reset failed: {e}"))
                .map_err(EmberIpcError::from)
        }
    }
}

pub(crate) fn status(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    started_at: std::time::Instant,
) -> Result<(), EmberIpcError> {
    let map = held.read().map_err(|_| EmberIpcError::LockPoisoned)?;
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
    .map_err(EmberIpcError::from)
}

pub(crate) fn ring_meta_get(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;
    let map = held.read().map_err(|_| EmberIpcError::LockPoisoned)?;
    if let Some(device) = map.get(bdf) {
        let meta_json = serde_json::to_value(&device.ring_meta).unwrap_or_default();
        drop(map);
        write_jsonrpc_ok(stream, id, meta_json).map_err(EmberIpcError::from)
    } else {
        drop(map);
        write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: not held by ember"))
            .map_err(EmberIpcError::from)
    }
}

pub(crate) fn ring_meta_set(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;
    let meta_val = params
        .get("ring_meta")
        .ok_or(EmberIpcError::InvalidRequest(
            "missing 'ring_meta' parameter",
        ))?;
    let meta: crate::hold::RingMeta = serde_json::from_value(meta_val.clone())
        .map_err(|e| EmberIpcError::JsonSerialize(format!("invalid ring_meta: {e}")))?;
    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    if let Some(device) = map.get_mut(bdf) {
        device.ring_meta = meta;
        drop(map);
        write_jsonrpc_ok(stream, id, serde_json::json!({"ok": true})).map_err(EmberIpcError::from)
    } else {
        drop(map);
        write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: not held by ember"))
            .map_err(EmberIpcError::from)
    }
}

/// Safely prepare a device for DMA experiments.
///
/// Maps BAR0 server-side, runs the centralized quiesce sequence
/// (PFIFO reset, scheduler stop, blind PRI ACK), masks AER, and
/// enables bus mastering. Stores the DMA state for later cleanup.
pub(crate) fn prepare_dma(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = match map.get_mut(bdf) {
        Some(d) => d,
        None => {
            drop(map);
            return write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("{bdf}: not held by ember"),
            )
            .map_err(EmberIpcError::from);
        }
    };

    if dev.dma_prepare_state.is_some() {
        drop(map);
        return write_jsonrpc_error(
            stream,
            id,
            -32001,
            &format!("{bdf}: DMA already prepared (call ember.cleanup_dma first)"),
        )
        .map_err(EmberIpcError::from);
    }

    let bar0 = match dev.device.map_bar(0) {
        Ok(b) => b,
        Err(e) => {
            drop(map);
            return write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("{bdf}: BAR0 map failed: {e}"),
            )
            .map_err(EmberIpcError::from);
        }
    };

    let result =
        coral_driver::vfio::device::dma_safety::prepare_dma(&bar0, &dev.device);
    drop(bar0);

    match result {
        Ok(state) => {
            let pmc_before = state.pmc_before;
            let pmc_after = state.pmc_after;
            dev.dma_prepare_state = Some(state);
            dev.experiment_dirty = true;
            drop(map);
            tracing::info!(bdf, "ember.prepare_dma: complete");
            write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({
                    "bdf": bdf,
                    "ok": true,
                    "pmc_before": format!("{pmc_before:#010x}"),
                    "pmc_after": format!("{pmc_after:#010x}"),
                }),
            )
            .map_err(EmberIpcError::from)
        }
        Err(e) => {
            drop(map);
            tracing::error!(bdf, error = %e, "ember.prepare_dma: failed");
            write_jsonrpc_error(stream, id, -32000, &format!("prepare_dma: {e}"))
                .map_err(EmberIpcError::from)
        }
    }
}

/// Clean up after a DMA experiment — disable bus master, restore AER masks.
pub(crate) fn cleanup_dma(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = match map.get_mut(bdf) {
        Some(d) => d,
        None => {
            drop(map);
            return write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("{bdf}: not held by ember"),
            )
            .map_err(EmberIpcError::from);
        }
    };

    let state = match dev.dma_prepare_state.take() {
        Some(s) => s,
        None => {
            drop(map);
            return write_jsonrpc_error(
                stream,
                id,
                -32001,
                &format!("{bdf}: no DMA prepare state (call ember.prepare_dma first)"),
            )
            .map_err(EmberIpcError::from);
        }
    };

    let result =
        coral_driver::vfio::device::dma_safety::cleanup_dma(&dev.device, &state);
    drop(map);

    match result {
        Ok(()) => {
            tracing::info!(bdf, "ember.cleanup_dma: complete");
            write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({"bdf": bdf, "ok": true}),
            )
            .map_err(EmberIpcError::from)
        }
        Err(e) => {
            tracing::error!(bdf, error = %e, "ember.cleanup_dma: failed");
            write_jsonrpc_error(stream, id, -32000, &format!("cleanup_dma: {e}"))
                .map_err(EmberIpcError::from)
        }
    }
}
