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

    // Crash-resilient trace for PRAMIN operations — survives system lockups.
    fn pt(msg: &str) {
        use std::io::Write as W;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/var/lib/coralreef/traces/ember_pramin_trace.log")
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(f, "[{ts}] {msg}");
            let _ = f.sync_all();
        }
    }

    let _ = std::fs::remove_file("/var/lib/coralreef/traces/ember_pramin_trace.log");
    pt(&format!("pramin_write ENTER bdf={bdf} vram_addr={vram_addr:#x} data_len={}", data.len()));

    let mut bytes_written: usize = 0;
    let write_error = {
        let bar0 = dev.bar0.as_ref().unwrap();
        let mut err: Option<String> = None;

        // Step 1: Read BAR0_WINDOW (save)
        pt("LIVENESS: reading BAR0_WINDOW (0x1700)");
        let saved_window = bar0.read_u32(0x1700).unwrap_or(0xDEAD_DEAD);
        pt(&format!("LIVENESS: BAR0_WINDOW={saved_window:#010x}"));

        // Step 2: Set window to target VRAM page
        let window_val = (vram_addr >> 16) as u32;
        pt(&format!("LIVENESS: writing BAR0_WINDOW={window_val:#010x}"));
        let _ = bar0.write_u32(0x1700, window_val);

        // Step 3: Read PRAMIN probe (the critical read)
        pt("LIVENESS: reading PRAMIN probe @ 0x700000");
        let probe = bar0.read_u32(0x0070_0000).unwrap_or(0xDEAD_DEAD);
        pt(&format!("LIVENESS: probe={probe:#010x}"));

        // Step 4: Restore window
        pt("LIVENESS: restoring BAR0_WINDOW");
        let _ = bar0.write_u32(0x1700, saved_window);
        pt("LIVENESS: restored OK");

        if probe == 0xDEAD_DEAD || probe == 0xFFFF_FFFF {
            err = Some(format!(
                "PRAMIN liveness check FAILED: read returned {probe:#010x} — BAR0 unresponsive"
            ));
        } else if probe & 0xFFF0_0000 == 0xBAD0_0000 {
            err = Some(format!(
                "PRAMIN liveness check FAILED: read returned {probe:#010x} — VRAM dead \
                 (FBPA/memory controller uninitialized). Warm GPU with nouveau first."
            ));
        }
        if let Some(ref e) = err {
            pt(&format!("LIVENESS: FAILED — {e}"));
            tracing::error!(bdf, vram_addr = format_args!("{vram_addr:#x}"),
                probe = format_args!("{probe:#010x}"),
                "PRAMIN liveness check failed — aborting bulk write");
        } else {
            pt("LIVENESS: PASSED — VRAM alive");
        }

        for (chunk_idx, chunk) in data.chunks(4096).enumerate() {
            if err.is_some() {
                break;
            }
            let offset = chunk_idx * 4096;

            if chunk_idx > 0 && chunk_idx % 4 == 0 {
                pt(&format!("CHUNK {chunk_idx}: PRI drain"));
                let _ = bar0.write_u32(0x0012_004C, 0x2);
                std::thread::sleep(std::time::Duration::from_micros(100));
            }

            pt(&format!("CHUNK {chunk_idx}: PraminRegion::new(vram={:#x}, len={})",
                vram_addr + offset as u32, chunk.len()));

            match PraminRegion::new(bar0, vram_addr + offset as u32, chunk.len()) {
                Ok(mut rgn) => {
                    pt(&format!("CHUNK {chunk_idx}: region OK — writing {} words", chunk.len() / 4));

                    for (i, word) in chunk.chunks(4).enumerate() {
                        let val = le_bytes_to_u32(word);
                        if rgn.write_u32(i * 4, val).is_err() {
                            err = Some(format!(
                                "PRAMIN write failed at vram_addr={:#x} chunk={chunk_idx} word={i}",
                                vram_addr
                            ));
                            pt(&format!("CHUNK {chunk_idx}: WRITE FAILED word {i}"));
                            break;
                        }
                        // Trace progress every 128 words (every 512 bytes)
                        if i > 0 && i % 128 == 0 {
                            pt(&format!("CHUNK {chunk_idx}: wrote {i}/{}",  chunk.len() / 4));
                        }
                    }

                    if err.is_none() {
                        // Force a read-back to flush posted writes and verify
                        let rb = rgn.read_u32(0).unwrap_or(0xDEAD_DEAD);
                        pt(&format!("CHUNK {chunk_idx}: done, readback[0]={rb:#010x}"));
                        bytes_written += chunk.len();
                    }
                }
                Err(e) => {
                    pt(&format!("CHUNK {chunk_idx}: PraminRegion FAILED: {e}"));
                    err = Some(format!("PRAMIN region open failed at offset {offset:#x}: {e}"));
                    break;
                }
            }
            pt(&format!("CHUNK {chunk_idx}: PraminRegion dropped (window restored)"));
        }

        pt(&format!("pramin_write DONE bytes={bytes_written} err={}", err.as_deref().unwrap_or("none")));
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

        // PRAMIN liveness check before bulk read
        {
            let saved_window = bar0.read_u32(0x1700).unwrap_or(0xDEAD_DEAD);
            let _ = bar0.write_u32(0x1700, (vram_addr >> 16) as u32);
            let probe = bar0.read_u32(0x0070_0000).unwrap_or(0xDEAD_DEAD);
            let _ = bar0.write_u32(0x1700, saved_window);

            if probe == 0xDEAD_DEAD || probe == 0xFFFF_FFFF {
                err = Some(format!(
                    "PRAMIN liveness check FAILED: read returned {probe:#010x} — BAR0 unresponsive"
                ));
            } else if probe & 0xFFF0_0000 == 0xBAD0_0000 {
                err = Some(format!(
                    "PRAMIN liveness check FAILED: read returned {probe:#010x} — VRAM dead"
                ));
            }
            if err.is_some() {
                tracing::error!(
                    bdf,
                    vram_addr = format_args!("{vram_addr:#x}"),
                    probe = format_args!("{probe:#010x}"),
                    "PRAMIN liveness check failed — aborting bulk read"
                );
            }
        }

        for (chunk_idx, _) in (0..length).step_by(4096).enumerate() {
            if err.is_some() {
                break;
            }
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
