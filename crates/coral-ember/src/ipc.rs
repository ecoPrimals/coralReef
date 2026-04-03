// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC 2.0 IPC handler and SCM_RIGHTS fd passing.

mod fd;
mod handlers_deploy;
mod handlers_device;
mod handlers_journal;
mod handlers_livepatch;
mod helpers;
mod jsonrpc;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, RwLock};

use crate::hold::HeldDevice;
use crate::journal::Journal;

pub use fd::send_with_fds;
pub use jsonrpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};

use jsonrpc::{ipc_io_error_string, write_jsonrpc_error};

const MAX_REQUEST_SIZE: usize = 4096;

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
    stream: &mut UnixStream,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    managed_bdfs: &HashSet<String>,
    started_at: std::time::Instant,
    journal: Option<&Arc<Journal>>,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let mut buf = [0u8; MAX_REQUEST_SIZE];
    let n = stream.read(&mut buf).map_err(|e| format!("read: {e}"))?;
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
        "ember.shutdown" => {
            handlers_device::shutdown(stream, held, id)?;
        }
        "ember.vfio_fds" => {
            handlers_device::vfio_fds(stream, held, managed_bdfs, id, params)?;
        }
        "ember.list" => {
            handlers_device::list(stream, held, id)?;
        }
        "ember.release" => {
            handlers_device::release(stream, held, managed_bdfs, id, params)?;
        }
        "ember.reacquire" => {
            handlers_device::reacquire(stream, held, managed_bdfs, id, params)?;
        }
        "ember.swap" => {
            handlers_device::swap(stream, held, managed_bdfs, id, params, journal)?;
        }
        "ember.device_reset" => {
            handlers_device::device_reset(stream, held, managed_bdfs, id, params, journal)?;
        }
        "ember.status" => {
            handlers_device::status(stream, held, id, started_at)?;
        }
        "ember.journal.query" => {
            handlers_journal::query(stream, id, params, journal)?;
        }
        "ember.journal.stats" => {
            handlers_journal::stats(stream, id, params, journal)?;
        }
        "ember.journal.append" => {
            handlers_journal::append(stream, id, params, journal)?;
        }
        "ember.ring_meta.get" => {
            handlers_device::ring_meta_get(stream, held, id, params)?;
        }
        "ember.ring_meta.set" => {
            handlers_device::ring_meta_set(stream, held, id, params)?;
        }
        "ember.mmio.read" => {
            handlers_device::mmio_read(stream, id, params)?;
        }
        "ember.mmio.write" => {
            handlers_device::mmio_write(stream, id, params)?;
            mark_experiment_dirty(held, params);
        }
        "ember.fecs.state" => {
            handlers_device::fecs_state(stream, id, params)?;
        }
        "ember.livepatch.status" => {
            handlers_livepatch::status(stream, id, params)?;
        }
        "ember.livepatch.enable" => {
            handlers_livepatch::enable(stream, id, params)?;
        }
        "ember.livepatch.disable" => {
            handlers_livepatch::disable(stream, id, params)?;
        }
        "ember.deploy" => {
            handlers_deploy::deploy(stream, id, params)?;
        }
        other => {
            write_jsonrpc_error(stream, id, -32601, &format!("method not found: {other}"))
                .map_err(ipc_io_error_string)?;
        }
    }

    Ok(())
}

/// Mark a device as experiment-dirty after a BAR0 write (Exp 138).
///
/// This triggers extra safety checks (PRAMIN restore, BAR0 health gate)
/// before the next driver swap, reducing D-state risk from experiments.
fn mark_experiment_dirty(
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    params: &serde_json::Value,
) {
    let Some(bdf) = params.get("bdf").and_then(|v| v.as_str()) else {
        return;
    };
    let Ok(mut held_map) = held.write() else {
        return;
    };
    if let Some(device) = held_map.get_mut(bdf) {
        if !device.experiment_dirty {
            tracing::info!(bdf, "marking device experiment-dirty after mmio write");
        }
        device.experiment_dirty = true;
    }
}

/// Same JSON-RPC surface as [`handle_client`], but over TCP (`ember.vfio_fds` cannot pass fds).
///
/// # Errors
///
/// Same as [`handle_client`].
pub fn handle_client_tcp(
    stream: &mut TcpStream,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    managed_bdfs: &HashSet<String>,
    started_at: std::time::Instant,
    journal: Option<&Arc<Journal>>,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let mut buf = [0u8; MAX_REQUEST_SIZE];
    let n = stream.read(&mut buf).map_err(|e| format!("read: {e}"))?;
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
        "ember.shutdown" => {
            handlers_device::shutdown(stream, held, id)?;
        }
        "ember.vfio_fds" => {
            handlers_device::vfio_fds_unavailable(stream, id)?;
        }
        "ember.list" => {
            handlers_device::list(stream, held, id)?;
        }
        "ember.release" => {
            handlers_device::release(stream, held, managed_bdfs, id, params)?;
        }
        "ember.reacquire" => {
            handlers_device::reacquire(stream, held, managed_bdfs, id, params)?;
        }
        "ember.swap" => {
            handlers_device::swap(stream, held, managed_bdfs, id, params, journal)?;
        }
        "ember.device_reset" => {
            handlers_device::device_reset(stream, held, managed_bdfs, id, params, journal)?;
        }
        "ember.status" => {
            handlers_device::status(stream, held, id, started_at)?;
        }
        "ember.journal.query" => {
            handlers_journal::query(stream, id, params, journal)?;
        }
        "ember.journal.stats" => {
            handlers_journal::stats(stream, id, params, journal)?;
        }
        "ember.journal.append" => {
            handlers_journal::append(stream, id, params, journal)?;
        }
        "ember.ring_meta.get" => {
            handlers_device::ring_meta_get(stream, held, id, params)?;
        }
        "ember.ring_meta.set" => {
            handlers_device::ring_meta_set(stream, held, id, params)?;
        }
        "ember.mmio.read" => {
            handlers_device::mmio_read(stream, id, params)?;
        }
        "ember.mmio.write" => {
            handlers_device::mmio_write(stream, id, params)?;
            mark_experiment_dirty(held, params);
        }
        "ember.fecs.state" => {
            handlers_device::fecs_state(stream, id, params)?;
        }
        "ember.livepatch.status" => {
            handlers_livepatch::status(stream, id, params)?;
        }
        "ember.livepatch.enable" => {
            handlers_livepatch::enable(stream, id, params)?;
        }
        "ember.livepatch.disable" => {
            handlers_livepatch::disable(stream, id, params)?;
        }
        "ember.deploy" => {
            handlers_deploy::deploy(stream, id, params)?;
        }
        other => {
            write_jsonrpc_error(stream, id, -32601, &format!("method not found: {other}"))
                .map_err(ipc_io_error_string)?;
        }
    }

    Ok(())
}
