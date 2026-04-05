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

    let value = dev.bar0.as_ref().unwrap().read_u32(offset).unwrap_or(0xDEAD_DEAD);
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

    let write_result = dev.bar0.as_ref().unwrap().write_u32(offset, value);
    dev.experiment_dirty = true;
    drop(map);

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

    let bar0 = dev.bar0.as_ref().unwrap();
    let mut results = Vec::with_capacity(ops.len());
    let mut had_write = false;

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

    if had_write {
        dev.experiment_dirty = true;
    }
    drop(map);

    write_jsonrpc_ok(stream, id, serde_json::json!({"results": results}))
        .map_err(EmberIpcError::from)
}
