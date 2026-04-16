// SPDX-License-Identifier: AGPL-3.0-or-later
//! JSON-RPC 2.0 IPC handler and SCM_RIGHTS fd passing.

mod fd;
mod handlers_device;
mod handlers_journal;
mod handlers_devinit;
mod handlers_mmio;
mod handlers_sovereign;
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

const MAX_REQUEST_SIZE: usize = 4096;

fn health_check_response(
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    started_at: std::time::Instant,
) -> serde_json::Value {
    let device_count = held.read().map(|m| m.len()).unwrap_or(0);
    serde_json::json!({
        "name": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
        "healthy": device_count > 0,
        "status": if device_count > 0 { "operational" } else { "degraded" },
        "device_count": device_count,
        "uptime_secs": started_at.elapsed().as_secs(),
    })
}

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
) -> Result<(), EmberIpcError> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(EmberIpcError::from)?;

    let mut buf = [0u8; MAX_REQUEST_SIZE];
    let n = stream.read(&mut buf).map_err(EmberIpcError::from)?;
    if n == 0 {
        return Ok(());
    }

    let outcome = crate::btsp::guard_from_first_byte(Some(buf[0]));
    if !outcome.should_accept() {
        tracing::warn!(?outcome, "BTSP rejected Unix connection");
        return Ok(());
    }

    let line = std::str::from_utf8(&buf[..n]).map_err(EmberIpcError::from)?;
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
        "mmio.read32" => {
            handlers_mmio::read32(stream, held, id, params)?;
        }
        "mmio.write32" => {
            handlers_mmio::write32(stream, held, id, params)?;
        }
        "mmio.batch" => {
            handlers_mmio::batch(stream, held, id, params)?;
        }
        "mmio.pramin.read32" => {
            handlers_mmio::pramin_read32(stream, held, id, params)?;
        }
        "mmio.bar0.probe" => {
            handlers_mmio::bar0_probe(stream, held, id, params)?;
        }
        "mmio.falcon.status" => {
            handlers_mmio::falcon_status(stream, held, id, params)?;
        }
        "ember.sovereign.init" => {
            handlers_sovereign::sovereign_init(stream, held, id, params)?;
        }
        "ember.devinit.status" => {
            handlers_devinit::devinit_status(stream, held, id, params)?;
        }
        "ember.devinit.execute" => {
            handlers_devinit::devinit_execute(stream, held, id, params)?;
        }
        "ember.vbios.read" => {
            handlers_devinit::vbios_read(stream, held, id, params)?;
        }
        "health.check" => {
            let resp = health_check_response(held, started_at);
            jsonrpc::write_jsonrpc_ok(stream, id, resp).map_err(EmberIpcError::from)?;
        }
        "health.liveness" => {
            jsonrpc::write_jsonrpc_ok(stream, id, serde_json::json!({ "alive": true }))
                .map_err(EmberIpcError::from)?;
        }
        "health.readiness" => {
            let device_count = held.read().map(|m| m.len()).unwrap_or(0);
            jsonrpc::write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({
                    "ready": device_count > 0,
                    "name": env!("CARGO_PKG_NAME"),
                }),
            )
            .map_err(EmberIpcError::from)?;
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
) -> Result<(), EmberIpcError> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(EmberIpcError::from)?;

    let mut buf = [0u8; MAX_REQUEST_SIZE];
    let n = stream.read(&mut buf).map_err(EmberIpcError::from)?;
    if n == 0 {
        return Ok(());
    }

    let outcome = crate::btsp::guard_from_first_byte(Some(buf[0]));
    if !outcome.should_accept() {
        tracing::warn!(?outcome, "BTSP rejected TCP connection");
        return Ok(());
    }

    let line = std::str::from_utf8(&buf[..n]).map_err(EmberIpcError::from)?;
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
        "mmio.read32" => {
            handlers_mmio::read32(stream, held, id, params)?;
        }
        "mmio.write32" => {
            handlers_mmio::write32(stream, held, id, params)?;
        }
        "mmio.batch" => {
            handlers_mmio::batch(stream, held, id, params)?;
        }
        "mmio.pramin.read32" => {
            handlers_mmio::pramin_read32(stream, held, id, params)?;
        }
        "mmio.bar0.probe" => {
            handlers_mmio::bar0_probe(stream, held, id, params)?;
        }
        "mmio.falcon.status" => {
            handlers_mmio::falcon_status(stream, held, id, params)?;
        }
        "ember.sovereign.init" => {
            handlers_sovereign::sovereign_init(stream, held, id, params)?;
        }
        "ember.devinit.status" => {
            handlers_devinit::devinit_status(stream, held, id, params)?;
        }
        "ember.devinit.execute" => {
            handlers_devinit::devinit_execute(stream, held, id, params)?;
        }
        "ember.vbios.read" => {
            handlers_devinit::vbios_read(stream, held, id, params)?;
        }
        "health.check" => {
            let resp = health_check_response(held, started_at);
            jsonrpc::write_jsonrpc_ok(stream, id, resp).map_err(EmberIpcError::from)?;
        }
        "health.liveness" => {
            jsonrpc::write_jsonrpc_ok(stream, id, serde_json::json!({ "alive": true }))
                .map_err(EmberIpcError::from)?;
        }
        "health.readiness" => {
            let device_count = held.read().map(|m| m.len()).unwrap_or(0);
            jsonrpc::write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({
                    "ready": device_count > 0,
                    "name": env!("CARGO_PKG_NAME"),
                }),
            )
            .map_err(EmberIpcError::from)?;
        }
        other => {
            write_jsonrpc_error(stream, id, -32601, &format!("method not found: {other}"))
                .map_err(EmberIpcError::from)?;
        }
    }

    Ok(())
}
