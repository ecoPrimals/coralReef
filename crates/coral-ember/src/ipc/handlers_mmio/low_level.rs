// SPDX-License-Identifier: AGPL-3.0-only
//! Low-level BAR0 register read/write/batch handlers.
//!
//! All operations use fork isolation — a stalled PCIe MMIO cannot freeze
//! the main ember thread. Each fork child combines PRI ring drain + BOOT0
//! preflight with the actual register operation in a single fork, so the
//! parent never touches BAR0 directly.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use coral_driver::vfio::device::dma_safety;

use crate::error::EmberIpcError;
use crate::hold::HeldDevice;
use crate::isolation::{self, ForkResult};

use super::super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};
use super::{map_bar0_if_needed, preflight_gate, update_fault_counter, require_bdf, require_held_mut, require_offset};

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

    if let Err(e) = preflight_gate(dev) {
        drop(map);
        return write_jsonrpc_error(stream, id, -32011, &e).map_err(EmberIpcError::from);
    }

    let bar0 = dev.bar0.as_ref().unwrap();
    let bar0_ptr = bar0.base_ptr() as usize;
    let bar0_size = bar0.size();
    let bdf_owned = bdf.to_string();
    drop(map);

    let fork_result = isolation::fork_isolated_mmio(
        &bdf_owned,
        isolation::OperationTier::RegisterIo.timeout(),
        |pipe_fd| {
            let bar0 = unsafe {
                coral_driver::vfio::device::MappedBar::from_raw(bar0_ptr as *mut u8, bar0_size)
            };
            let _ = bar0.write_u32(0x0012_004C, 0x2); // PRI ring ACK
            let boot0 = bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);
            let value = bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);
            // 8 bytes: boot0 (4 LE) + value (4 LE)
            let mut buf = [0u8; 8];
            buf[..4].copy_from_slice(&boot0.to_le_bytes());
            buf[4..].copy_from_slice(&value.to_le_bytes());
            unsafe { libc::write(pipe_fd, buf.as_ptr().cast(), 8); }
            std::mem::forget(bar0);
        },
    );

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    match fork_result {
        ForkResult::Ok(data) if data.len() >= 8 => {
            let boot0 = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            let value = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            if let Some(dev) = map.get_mut(&bdf_owned) {
                if let Err(e) = update_fault_counter(dev, boot0) {
                    drop(map);
                    return write_jsonrpc_error(stream, id, -32011, &e)
                        .map_err(EmberIpcError::from);
                }
            }
            drop(map);
            write_jsonrpc_ok(stream, id, serde_json::json!({"value": value}))
                .map_err(EmberIpcError::from)
        }
        ForkResult::Ok(_) => {
            if let Some(dev) = map.get_mut(&bdf_owned) {
                dev.mmio_fault_count += 1;
            }
            drop(map);
            write_jsonrpc_error(stream, id, -32000, &format!("{bdf_owned}: fork child returned truncated data"))
                .map_err(EmberIpcError::from)
        }
        ForkResult::Timeout => {
            if let Some(dev) = map.get_mut(&bdf_owned) {
                dev.emergency_quiesce();
            }
            drop(map);
            crate::hold::check_voluntary_death(held);
            write_jsonrpc_error(
                stream, id, -32099,
                &format!("{bdf_owned}: mmio_read at {offset:#x} timed out — device faulted."),
            ).map_err(EmberIpcError::from)
        }
        ForkResult::ChildFailed { status } => {
            if let Some(dev) = map.get_mut(&bdf_owned) {
                dev.emergency_quiesce();
            }
            drop(map);
            crate::hold::check_voluntary_death(held);
            write_jsonrpc_error(
                stream, id, -32098,
                &format!("{bdf_owned}: mmio_read child failed (status={status})."),
            ).map_err(EmberIpcError::from)
        }
        ForkResult::ForkFailed(e) | ForkResult::PipeFailed(e) => {
            drop(map);
            write_jsonrpc_error(stream, id, -32000, &format!("{bdf_owned}: fork/pipe failed: {e}"))
                .map_err(EmberIpcError::from)
        }
    }
}

/// `ember.mmio.write` — validated single BAR0 register write.
///
/// Params: `{bdf, offset, value}` (both as integers)
/// Result: `{ok: true}`
///
/// Respects the per-device [`TeardownPolicy`]: when `BlockTeardown` or
/// `BlockAndLog` is active, writes that would destroy the GPU security
/// context (PMU halt, DMEM scrub, FECS clear, PMC strip) are rejected
/// with error code `-32012`.
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

    if dev.teardown_policy.blocks() && dma_safety::is_teardown_write(offset, value) {
        if dev.teardown_policy.logs() {
            tracing::warn!(
                bdf = %dev.bdf,
                offset = format_args!("{offset:#x}"),
                value = format_args!("{value:#010x}"),
                "MMIO write firewall: BLOCKED teardown write"
            );
        }
        drop(map);
        return write_jsonrpc_error(
            stream,
            id,
            -32012,
            &format!(
                "teardown write blocked: offset {offset:#x} value {value:#010x} \
                 would destroy GPU security context"
            ),
        )
        .map_err(EmberIpcError::from);
    }

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
            .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_gate(dev) {
        drop(map);
        return write_jsonrpc_error(stream, id, -32011, &e).map_err(EmberIpcError::from);
    }

    let bar0 = dev.bar0.as_ref().unwrap();
    let bar0_ptr = bar0.base_ptr() as usize;
    let bar0_size = bar0.size();
    let bdf_owned = bdf.to_string();
    drop(map);

    let fork_result = isolation::fork_isolated_mmio(
        &bdf_owned,
        isolation::OperationTier::RegisterIo.timeout(),
        |pipe_fd| {
            let bar0 = unsafe {
                coral_driver::vfio::device::MappedBar::from_raw(bar0_ptr as *mut u8, bar0_size)
            };
            let _ = bar0.write_u32(0x0012_004C, 0x2); // PRI ring ACK
            let boot0 = bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);
            let write_ok = bar0.write_u32(offset, value).is_ok();
            // 5 bytes: boot0 (4 LE) + status (1: 0=ok, 1=fail)
            let mut buf = [0u8; 5];
            buf[..4].copy_from_slice(&boot0.to_le_bytes());
            buf[4] = if write_ok { 0 } else { 1 };
            unsafe { libc::write(pipe_fd, buf.as_ptr().cast(), 5); }
            std::mem::forget(bar0);
        },
    );

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    match fork_result {
        ForkResult::Ok(data) if data.len() >= 5 => {
            let boot0 = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            let write_ok = data[4] == 0;
            if let Some(dev) = map.get_mut(&bdf_owned) {
                if let Err(e) = update_fault_counter(dev, boot0) {
                    drop(map);
                    return write_jsonrpc_error(stream, id, -32011, &e)
                        .map_err(EmberIpcError::from);
                }
                dev.experiment_dirty = true;
            }
            drop(map);
            if write_ok {
                write_jsonrpc_ok(stream, id, serde_json::json!({"ok": true}))
                    .map_err(EmberIpcError::from)
            } else {
                write_jsonrpc_error(stream, id, -32000, &format!("{bdf_owned}: write failed"))
                    .map_err(EmberIpcError::from)
            }
        }
        ForkResult::Ok(_) => {
            if let Some(dev) = map.get_mut(&bdf_owned) {
                dev.mmio_fault_count += 1;
            }
            drop(map);
            write_jsonrpc_error(stream, id, -32000, &format!("{bdf_owned}: fork child returned truncated data"))
                .map_err(EmberIpcError::from)
        }
        ForkResult::Timeout => {
            if let Some(dev) = map.get_mut(&bdf_owned) {
                dev.emergency_quiesce();
            }
            drop(map);
            crate::hold::check_voluntary_death(held);
            write_jsonrpc_error(
                stream, id, -32099,
                &format!("{bdf_owned}: mmio_write at {offset:#x} timed out — device faulted."),
            ).map_err(EmberIpcError::from)
        }
        ForkResult::ChildFailed { status } => {
            if let Some(dev) = map.get_mut(&bdf_owned) {
                dev.emergency_quiesce();
            }
            drop(map);
            crate::hold::check_voluntary_death(held);
            write_jsonrpc_error(
                stream, id, -32098,
                &format!("{bdf_owned}: mmio_write child failed (status={status})."),
            ).map_err(EmberIpcError::from)
        }
        ForkResult::ForkFailed(e) | ForkResult::PipeFailed(e) => {
            drop(map);
            write_jsonrpc_error(stream, id, -32000, &format!("{bdf_owned}: fork/pipe failed: {e}"))
                .map_err(EmberIpcError::from)
        }
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

    let batch_policy = dev.teardown_policy;

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
            .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_gate(dev) {
        drop(map);
        return write_jsonrpc_error(stream, id, -32011, &e).map_err(EmberIpcError::from);
    }

    let bar0 = dev.bar0.as_ref().unwrap();
    let bar0_ptr = bar0.base_ptr() as usize;
    let bar0_size = bar0.size();
    let bdf_owned = bdf.to_string();
    drop(map);

    let fork_result = isolation::fork_isolated_mmio(
        &bdf_owned,
        isolation::OperationTier::RegisterIo.timeout(),
        |pipe_fd| {
            let bar0 = unsafe {
                coral_driver::vfio::device::MappedBar::from_raw(bar0_ptr as *mut u8, bar0_size)
            };
            let _ = bar0.write_u32(0x0012_004C, 0x2); // PRI ring ACK
            let boot0 = bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);

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
                        if batch_policy.blocks() && dma_safety::is_teardown_write(offset, value) {
                            results.push(serde_json::json!({
                                "error": "teardown_blocked",
                                "offset": offset,
                                "value": value,
                            }));
                            continue;
                        }
                        let _ = bar0.write_u32(offset, value);
                        had_write = true;
                        results.push(serde_json::json!(true));
                    }
                    other => {
                        results.push(serde_json::json!({"error": format!("unknown op type: {other}")}));
                    }
                }
            }

            let json = serde_json::json!({
                "boot0": boot0,
                "results": results,
                "had_write": had_write,
            });
            if let Ok(bytes) = serde_json::to_vec(&json) {
                unsafe { libc::write(pipe_fd, bytes.as_ptr().cast(), bytes.len()); }
            }
            std::mem::forget(bar0);
        },
    );

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    match fork_result {
        ForkResult::Ok(data) => {
            let parsed: serde_json::Value = serde_json::from_slice(&data)
                .unwrap_or(serde_json::json!({"boot0": 0xFFFFFFFFu32, "results": []}));
            let boot0 = parsed.get("boot0").and_then(|v| v.as_u64()).unwrap_or(0xFFFF_FFFF) as u32;
            let results = parsed.get("results").cloned().unwrap_or(serde_json::json!([]));
            let had_write = parsed.get("had_write").and_then(|v| v.as_bool()).unwrap_or(false);

            if let Some(dev) = map.get_mut(&bdf_owned) {
                if let Err(e) = update_fault_counter(dev, boot0) {
                    drop(map);
                    return write_jsonrpc_error(stream, id, -32011, &e)
                        .map_err(EmberIpcError::from);
                }
                if had_write {
                    dev.experiment_dirty = true;
                }
            }
            drop(map);
            write_jsonrpc_ok(stream, id, serde_json::json!({"results": results}))
                .map_err(EmberIpcError::from)
        }
        ForkResult::Timeout => {
            if let Some(dev) = map.get_mut(&bdf_owned) {
                dev.emergency_quiesce();
            }
            drop(map);
            crate::hold::check_voluntary_death(held);
            write_jsonrpc_error(
                stream, id, -32099,
                &format!("{bdf_owned}: mmio_batch timed out — device faulted."),
            ).map_err(EmberIpcError::from)
        }
        ForkResult::ChildFailed { status } => {
            if let Some(dev) = map.get_mut(&bdf_owned) {
                dev.emergency_quiesce();
            }
            drop(map);
            crate::hold::check_voluntary_death(held);
            write_jsonrpc_error(
                stream, id, -32098,
                &format!("{bdf_owned}: mmio_batch child failed (status={status})."),
            ).map_err(EmberIpcError::from)
        }
        ForkResult::ForkFailed(e) | ForkResult::PipeFailed(e) => {
            drop(map);
            write_jsonrpc_error(stream, id, -32000, &format!("{bdf_owned}: fork/pipe failed: {e}"))
                .map_err(EmberIpcError::from)
        }
    }
}
