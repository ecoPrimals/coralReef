// SPDX-License-Identifier: AGPL-3.0-only
//! PRAMIN bulk VRAM read/write handlers.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use coral_driver::vfio::memory::{MemoryRegion, PraminRegion};

use crate::error::EmberIpcError;
use crate::hold::HeldDevice;

use super::super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};
use super::{
    base64_decode, base64_encode, le_bytes_to_u32, map_bar0_if_needed, preflight_check,
    require_bdf, require_held_mut,
};

/// `ember.pramin.write` — bulk VRAM write via PRAMIN window, server-side.
///
/// Params: `{bdf, vram_addr, data_b64}` — data is base64-encoded bytes
/// Result: `{ok, bytes_written}`
pub(crate) fn pramin_write(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;
    let vram_addr = params
        .get("vram_addr")
        .and_then(|v| v.as_u64())
        .ok_or(EmberIpcError::InvalidRequest("missing 'vram_addr'"))? as u32;
    let data_b64 = params
        .get("data_b64")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'data_b64'"))?;

    let data = base64_decode(data_b64).map_err(|e| {
        EmberIpcError::InvalidRequest(Box::leak(format!("base64 decode: {e}").into_boxed_str()))
    })?;

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

    let mut bytes_written: usize = 0;
    let write_error = {
        let bar0 = dev.bar0.as_ref().unwrap();
        let mut err: Option<String> = None;
        for (chunk_idx, chunk) in data.chunks(4096).enumerate() {
            let offset = chunk_idx * 4096;
            match PraminRegion::new(bar0, vram_addr + offset as u32, chunk.len()) {
                Ok(mut rgn) => {
                    for (i, word) in chunk.chunks(4).enumerate() {
                        let val = le_bytes_to_u32(word);
                        if rgn.write_u32(i * 4, val).is_err() {
                            err = Some(format!(
                                "PRAMIN write failed at vram_addr={:#x} chunk={chunk_idx} word={i}",
                                vram_addr
                            ));
                            break;
                        }
                    }
                    if err.is_some() {
                        break;
                    }
                    bytes_written += chunk.len();
                }
                Err(e) => {
                    err = Some(format!("PRAMIN region open failed at offset {offset:#x}: {e}"));
                    break;
                }
            }
        }
        err
    };

    dev.experiment_dirty = true;

    if let Some(err_msg) = write_error {
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &err_msg).map_err(EmberIpcError::from);
    }

    drop(map);

    tracing::info!(bdf, vram_addr = format_args!("{vram_addr:#x}"), bytes_written, "ember.pramin.write: complete");
    write_jsonrpc_ok(
        stream,
        id,
        serde_json::json!({"ok": true, "bytes_written": bytes_written}),
    )
    .map_err(EmberIpcError::from)
}

/// `ember.pramin.read` — bulk VRAM read via PRAMIN window, server-side.
///
/// Params: `{bdf, vram_addr, length}`
/// Result: `{data_b64}`
pub(crate) fn pramin_read(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;
    let vram_addr = params
        .get("vram_addr")
        .and_then(|v| v.as_u64())
        .ok_or(EmberIpcError::InvalidRequest("missing 'vram_addr'"))? as u32;
    let length = params
        .get("length")
        .and_then(|v| v.as_u64())
        .ok_or(EmberIpcError::InvalidRequest("missing 'length'"))? as usize;

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

    let read_result = {
        let bar0 = dev.bar0.as_ref().unwrap();
        let mut result_data = Vec::with_capacity(length);
        let mut err: Option<String> = None;
        for (chunk_idx, _) in (0..length).step_by(4096).enumerate() {
            let chunk_byte_offset = chunk_idx * 4096;
            let chunk_len = 4096.min(length - chunk_byte_offset);
            match PraminRegion::new(bar0, vram_addr + chunk_byte_offset as u32, chunk_len) {
                Ok(rgn) => {
                    let word_count = chunk_len.div_ceil(4);
                    for i in 0..word_count {
                        let val = rgn.read_u32(i * 4).unwrap_or(0xDEAD_DEAD);
                        result_data.extend_from_slice(&val.to_le_bytes());
                    }
                }
                Err(e) => {
                    err = Some(format!("PRAMIN region open failed: {e}"));
                    break;
                }
            }
        }
        result_data.truncate(length);
        (result_data, err)
    };

    drop(map);

    let (result_data, err) = read_result;
    if let Some(err_msg) = err {
        return write_jsonrpc_error(stream, id, -32000, &err_msg).map_err(EmberIpcError::from);
    }

    let encoded = base64_encode(&result_data);
    write_jsonrpc_ok(
        stream,
        id,
        serde_json::json!({"data_b64": encoded, "length": result_data.len()}),
    )
    .map_err(EmberIpcError::from)
}
