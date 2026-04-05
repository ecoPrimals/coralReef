// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC 2.0 IPC handler and SCM_RIGHTS fd passing.

mod fd;
mod handlers_device;
mod handlers_journal;
pub(crate) mod handlers_mmio;
pub(crate) mod handlers_policy;
mod helpers;
mod jsonrpc;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, RwLock};

use crate::error::EmberIpcError;
use crate::hold::HeldDevice;
use crate::journal::Journal;

pub use fd::send_with_fds;
pub use jsonrpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};

use jsonrpc::write_jsonrpc_error;

/// Max request size — 2MB to accommodate base64-encoded PRAMIN payloads
/// (a 200KB WPR blob encodes to ~270KB base64 + JSON overhead).
const MAX_REQUEST_SIZE: usize = 2 * 1024 * 1024;

/// Handle one JSON-RPC request on `stream` (read one line, dispatch, write response).
///
/// For `ember.vfio_fds`, sends the JSON line first, then passes fds via `SCM_RIGHTS`.
///
/// # Errors
///
/// Returns `Err` for transport failures (I/O, UTF-8, lock poison). Application faults are encoded in
/// JSON-RPC error responses on the stream.
pub fn handle_client(
    stream: &mut UnixStream,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    managed_bdfs: &HashSet<String>,
    started_at: std::time::Instant,
    journal: Option<&Arc<Journal>>,
    policies: &handlers_policy::PolicyStore,
) -> Result<(), EmberIpcError> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(EmberIpcError::from)?;

    let line = read_request_line(stream)?;
    let line = line.trim();
    if line.is_empty() {
        return Ok(());
    }

    let req: JsonRpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            write_jsonrpc_error(
                stream,
                serde_json::Value::Null,
                -32700,
                &format!("parse error: {e}"),
            )
            .map_err(EmberIpcError::from)?;
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
        .map_err(EmberIpcError::from)?;
        return Ok(());
    }

    let id = req.id;
    let params = &req.params;

    match req.method.as_str() {
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
            handlers_device::device_reset(stream, managed_bdfs, id, params, journal)?;
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
        "ember.prepare_dma" => {
            handlers_device::prepare_dma(stream, held, id, params)?;
        }
        "ember.cleanup_dma" => {
            handlers_device::cleanup_dma(stream, held, id, params)?;
        }
        // MMIO Gateway — Layer 1: low-level register access
        "ember.mmio.read" => {
            handlers_mmio::mmio_read(stream, held, id, params)?;
        }
        "ember.mmio.write" => {
            handlers_mmio::mmio_write(stream, held, id, params)?;
        }
        "ember.mmio.batch" => {
            handlers_mmio::mmio_batch(stream, held, id, params)?;
        }
        "ember.pramin.write" => {
            handlers_mmio::pramin_write(stream, held, id, params)?;
        }
        "ember.pramin.read" => {
            handlers_mmio::pramin_read(stream, held, id, params)?;
        }
        // MMIO Gateway — Layer 2: high-level experiment RPCs
        "ember.sec2.prepare_physical" => {
            handlers_mmio::sec2_prepare_physical(stream, held, id, params)?;
        }
        "ember.falcon.upload_imem" => {
            handlers_mmio::falcon_upload_imem(stream, held, id, params)?;
        }
        "ember.falcon.upload_dmem" => {
            handlers_mmio::falcon_upload_dmem(stream, held, id, params)?;
        }
        "ember.falcon.start_cpu" => {
            handlers_mmio::falcon_start_cpu(stream, held, id, params)?;
        }
        "ember.falcon.poll" => {
            handlers_mmio::falcon_poll(stream, held, id, params)?;
        }
        "ember.mmio.circuit_breaker" => {
            handlers_mmio::circuit_breaker(stream, held, id, params)?;
        }
        "ember.policy.get" => {
            handlers_policy::get(stream, policies, id, params)?;
        }
        "ember.policy.set" => {
            handlers_policy::set(stream, policies, id, params)?;
        }
        "ember.policy.list" => {
            handlers_policy::list(stream, policies, id)?;
        }
        "ember.policy.matrix" => {
            handlers_policy::matrix(stream, id, params)?;
        }
        other => {
            write_jsonrpc_error(stream, id, -32601, &format!("method not found: {other}"))
                .map_err(EmberIpcError::from)?;
        }
    }

    Ok(())
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
    policies: &handlers_policy::PolicyStore,
) -> Result<(), EmberIpcError> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(EmberIpcError::from)?;

    let line = read_request_line(stream)?;
    let line = line.trim();
    if line.is_empty() {
        return Ok(());
    }

    let req: JsonRpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            write_jsonrpc_error(
                stream,
                serde_json::Value::Null,
                -32700,
                &format!("parse error: {e}"),
            )
            .map_err(EmberIpcError::from)?;
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
        .map_err(EmberIpcError::from)?;
        return Ok(());
    }

    let id = req.id;
    let params = &req.params;

    match req.method.as_str() {
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
            handlers_device::device_reset(stream, managed_bdfs, id, params, journal)?;
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
        "ember.prepare_dma" => {
            handlers_device::prepare_dma(stream, held, id, params)?;
        }
        "ember.cleanup_dma" => {
            handlers_device::cleanup_dma(stream, held, id, params)?;
        }
        // MMIO Gateway RPCs available over TCP too
        "ember.mmio.read" => {
            handlers_mmio::mmio_read(stream, held, id, params)?;
        }
        "ember.mmio.write" => {
            handlers_mmio::mmio_write(stream, held, id, params)?;
        }
        "ember.mmio.batch" => {
            handlers_mmio::mmio_batch(stream, held, id, params)?;
        }
        "ember.pramin.write" => {
            handlers_mmio::pramin_write(stream, held, id, params)?;
        }
        "ember.pramin.read" => {
            handlers_mmio::pramin_read(stream, held, id, params)?;
        }
        "ember.sec2.prepare_physical" => {
            handlers_mmio::sec2_prepare_physical(stream, held, id, params)?;
        }
        "ember.falcon.upload_imem" => {
            handlers_mmio::falcon_upload_imem(stream, held, id, params)?;
        }
        "ember.falcon.upload_dmem" => {
            handlers_mmio::falcon_upload_dmem(stream, held, id, params)?;
        }
        "ember.falcon.start_cpu" => {
            handlers_mmio::falcon_start_cpu(stream, held, id, params)?;
        }
        "ember.falcon.poll" => {
            handlers_mmio::falcon_poll(stream, held, id, params)?;
        }
        "ember.mmio.circuit_breaker" => {
            handlers_mmio::circuit_breaker(stream, held, id, params)?;
        }
        "ember.policy.get" => {
            handlers_policy::get(stream, policies, id, params)?;
        }
        "ember.policy.set" => {
            handlers_policy::set(stream, policies, id, params)?;
        }
        "ember.policy.list" => {
            handlers_policy::list(stream, policies, id)?;
        }
        "ember.policy.matrix" => {
            handlers_policy::matrix(stream, id, params)?;
        }
        other => {
            write_jsonrpc_error(stream, id, -32601, &format!("method not found: {other}"))
                .map_err(EmberIpcError::from)?;
        }
    }

    Ok(())
}

/// Read a newline-delimited JSON-RPC request from any `Read` stream.
/// Supports payloads up to `MAX_REQUEST_SIZE` by reading in chunks until `\n`.
fn read_request_line(stream: &mut impl Read) -> Result<String, EmberIpcError> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 8192];
    loop {
        let n = stream.read(&mut chunk).map_err(EmberIpcError::from)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.contains(&b'\n') || buf.len() >= MAX_REQUEST_SIZE {
            break;
        }
    }
    let s = std::str::from_utf8(&buf).map_err(EmberIpcError::from)?;
    Ok(s.to_string())
}
