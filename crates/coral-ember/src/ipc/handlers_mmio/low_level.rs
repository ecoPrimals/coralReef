// SPDX-License-Identifier: AGPL-3.0-only
//! Low-level BAR0 register read/write/batch handlers.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use coral_driver::vfio::device::dma_safety;

use crate::error::EmberIpcError;
use crate::hold::HeldDevice;

use super::super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};
use super::{map_bar0_if_needed, preflight_check, require_bdf, require_held_mut, require_offset};

/// `ember.mmio.read` — validated single BAR0 register read.
///
/// Params: `{bdf, offset}` (offset as integer)
/// Result: `{value}` (u32 as integer)
pub(crate) fn mmio_read(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;
    let offset = require_offset(params)?;

    if dma_safety::is_poisonous_read(offset) {
        return write_jsonrpc_error(
            stream,
            id,
            -32010,
            &format!("offset {offset:#x} is a known poisonous register — read blocked"),
        )
        .map_err(EmberIpcError::from);
    }

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = require_held_mut(&mut map, bdf, stream, &id)?;

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
            .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_check(dev) {
        drop(map);
        return write_jsonrpc_error(stream, id, -32011, &e).map_err(EmberIpcError::from);
    }

    let bar0 = dev.bar0.as_ref().unwrap();
    let bdf_owned = bdf.to_string();
    let (value, watchdog_fired) = crate::isolation::with_mmio_watchdog(
        &bdf_owned,
        crate::isolation::OperationTier::RegisterIo.timeout(),
        || bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD),
    );

    if watchdog_fired {
        tracing::error!(bdf = bdf_owned, offset, "mmio_read: watchdog fired — bus-reset triggered");
        dev.emergency_quiesce();
        drop(map);
        crate::hold::check_voluntary_death(held);
        return write_jsonrpc_error(
            stream, id, -32099,
            &format!("{bdf_owned}: mmio_read at {offset:#x} triggered watchdog bus-reset. Device faulted."),
        ).map_err(EmberIpcError::from);
    }
    drop(map);

    write_jsonrpc_ok(stream, id, serde_json::json!({"value": value})).map_err(EmberIpcError::from)
}

/// `ember.mmio.write` — validated single BAR0 register write.
///
/// Params: `{bdf, offset, value}` (both as integers)
/// Result: `{ok: true}`
pub(crate) fn mmio_write(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;
    let offset = require_offset(params)?;
    let value = params
        .get("value")
        .and_then(|v| v.as_u64())
        .ok_or(EmberIpcError::InvalidRequest("missing 'value' parameter"))? as u32;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = require_held_mut(&mut map, bdf, stream, &id)?;

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
            .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_check(dev) {
        drop(map);
        return write_jsonrpc_error(stream, id, -32011, &e).map_err(EmberIpcError::from);
    }

    let bdf_owned = bdf.to_string();
    let bar0 = dev.bar0.as_ref().unwrap();
    let (write_result, watchdog_fired) = crate::isolation::with_mmio_watchdog(
        &bdf_owned,
        crate::isolation::OperationTier::RegisterIo.timeout(),
        || bar0.write_u32(offset, value),
    );
    if watchdog_fired {
        dev.emergency_quiesce();
    }
    dev.experiment_dirty = true;
    drop(map);
    if watchdog_fired {
        crate::hold::check_voluntary_death(held);
    }

    match write_result {
        Ok(()) => write_jsonrpc_ok(stream, id, serde_json::json!({"ok": true}))
            .map_err(EmberIpcError::from),
        Err(e) => write_jsonrpc_error(stream, id, -32000, &format!("write failed: {e}"))
            .map_err(EmberIpcError::from),
    }
}

/// `ember.mmio.batch` — batch of reads/writes in one IPC round-trip.
///
/// Params: `{bdf, ops: [{type:"r"|"w", offset, value?}]}`
/// Result: `{results: [value_or_ok]}` — for reads: u32 value, for writes: `true`.
pub(crate) fn mmio_batch(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;
    let ops = params
        .get("ops")
        .and_then(|v| v.as_array())
        .ok_or(EmberIpcError::InvalidRequest("missing 'ops' array"))?
        .clone();

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = require_held_mut(&mut map, bdf, stream, &id)?;

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
            .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_check(dev) {
        drop(map);
        return write_jsonrpc_error(stream, id, -32011, &e).map_err(EmberIpcError::from);
    }

    let bdf_owned = bdf.to_string();
    let mut results = Vec::with_capacity(ops.len());
    let mut had_write = false;

    let bar0 = dev.bar0.as_ref().unwrap();
    let (_, watchdog_fired) = crate::isolation::with_mmio_watchdog(
        &bdf_owned,
        crate::isolation::OperationTier::RegisterIo.timeout(),
        || {
            for (i, op) in ops.iter().enumerate() {
                let op_type = op.get("type").and_then(|v| v.as_str()).unwrap_or("r");
                let offset = match op.get("offset").and_then(|v| v.as_u64()) {
                    Some(o) => o as usize,
                    None => {
                        results.push(serde_json::json!({"error": format!("op[{i}]: missing offset")}));
                        continue;
                    }
                };

                match op_type {
                    "r" => {
                        if dma_safety::is_poisonous_read(offset) {
                            results.push(serde_json::json!({"error": "poisonous", "offset": offset}));
                            continue;
                        }
                        let val = bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);
                        results.push(serde_json::json!(val));
                    }
                    "w" => {
                        let value = op.get("value").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let _ = bar0.write_u32(offset, value);
                        had_write = true;
                        results.push(serde_json::json!(true));
                    }
                    other => {
                        results.push(serde_json::json!({"error": format!("unknown op type: {other}")}));
                    }
                }
            }
        },
    );

    let quiesced = watchdog_fired;
    if watchdog_fired {
        dev.emergency_quiesce();
    }
    if had_write {
        dev.experiment_dirty = true;
    }
    drop(map);
    if quiesced {
        crate::hold::check_voluntary_death(held);
    }

    write_jsonrpc_ok(stream, id, serde_json::json!({"results": results}))
        .map_err(EmberIpcError::from)
}
