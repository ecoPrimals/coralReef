// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC 2.0 IPC handler and SCM_RIGHTS fd passing.

use std::collections::HashMap;
use std::io::Write;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::UnixStream;

use rustix::io::IoSlice;
use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};

use serde::{Deserialize, Serialize};

use crate::hold::HeldDevice;
use crate::swap;

const MAX_REQUEST_SIZE: usize = 4096;

#[derive(Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    pub id: serde_json::Value,
}

#[derive(Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: serde_json::Value,
}

#[derive(Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

fn make_jsonrpc_ok(id: serde_json::Value, result: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        result: Some(result),
        error: None,
        id,
    }
}

fn write_jsonrpc_ok(
    stream: &UnixStream,
    id: serde_json::Value,
    result: serde_json::Value,
) -> Result<(), String> {
    let resp = make_jsonrpc_ok(id, result);
    let json = serde_json::to_string(&resp).map_err(|e| format!("serialize: {e}"))?;
    let mut w: &UnixStream = stream;
    w.write_all(format!("{json}\n").as_bytes())
        .map_err(|e| format!("write: {e}"))
}

fn write_jsonrpc_error(
    stream: &UnixStream,
    id: serde_json::Value,
    code: i32,
    message: &str,
) -> Result<(), String> {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0",
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.to_string(),
        }),
        id,
    };
    let json = serde_json::to_string(&resp).map_err(|e| format!("serialize: {e}"))?;
    let mut w: &UnixStream = stream;
    w.write_all(format!("{json}\n").as_bytes())
        .map_err(|e| format!("write: {e}"))
}

pub fn handle_client(
    stream: &UnixStream,
    held: &mut HashMap<String, HeldDevice>,
    started_at: std::time::Instant,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let mut buf = [0u8; MAX_REQUEST_SIZE];
    let n = std::io::Read::read(&mut &*stream, &mut buf).map_err(|e| format!("read: {e}"))?;
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
            )?;
            return Ok(());
        }
    };

    if req.jsonrpc != "2.0" {
        write_jsonrpc_error(
            stream,
            req.id,
            -32600,
            &format!("invalid jsonrpc version: {}", req.jsonrpc),
        )?;
        return Ok(());
    }

    let id = req.id;
    let params = &req.params;

    match req.method.as_str() {
        "ember.vfio_fds" => {
            let bdf = params
                .get("bdf")
                .and_then(|v| v.as_str())
                .ok_or("missing 'bdf' parameter")?;
            let dev = match held.get(bdf) {
                Some(d) => d,
                None => {
                    write_jsonrpc_error(
                        stream,
                        id,
                        -32000,
                        &format!("device {bdf} not held by ember"),
                    )?;
                    return Ok(());
                }
            };

            let fds = [
                dev.device.container_as_fd(),
                dev.device.group_as_fd(),
                dev.device.device_as_fd(),
            ];

            let resp = make_jsonrpc_ok(id, serde_json::json!({"bdf": bdf, "num_fds": 3}));
            let resp_bytes = format!(
                "{}\n",
                serde_json::to_string(&resp).map_err(|e| format!("serialize: {e}"))?
            );

            send_with_fds(stream, resp_bytes.as_bytes(), &fds)
                .map_err(|e| format!("sendmsg: {e}"))?;
            tracing::debug!(bdf, "sent VFIO fds to client");
        }
        "ember.list" => {
            let devices: Vec<String> = held.keys().cloned().collect();
            write_jsonrpc_ok(stream, id, serde_json::json!({"devices": devices}))?;
        }
        "ember.release" => {
            let bdf = params
                .get("bdf")
                .and_then(|v| v.as_str())
                .ok_or("missing 'bdf' parameter")?;
            match held.remove(bdf) {
                Some(device) => {
                    drop(device);
                    tracing::info!(bdf, "ember released VFIO fds for swap");
                    write_jsonrpc_ok(stream, id, serde_json::json!({"bdf": bdf}))?;
                }
                None => {
                    write_jsonrpc_error(
                        stream,
                        id,
                        -32000,
                        &format!("device {bdf} not held by ember"),
                    )?;
                }
            }
        }
        "ember.reacquire" => {
            let bdf = params
                .get("bdf")
                .and_then(|v| v.as_str())
                .ok_or("missing 'bdf' parameter")?;
            if held.contains_key(bdf) {
                tracing::warn!(bdf, "device already held — skipping reacquire");
                write_jsonrpc_ok(stream, id, serde_json::json!({"bdf": bdf}))?;
            } else {
                match coral_driver::vfio::VfioDevice::open(bdf) {
                    Ok(device) => {
                        tracing::info!(
                            bdf,
                            container_fd = device.container_fd(),
                            group_fd = device.group_fd(),
                            device_fd = device.device_fd(),
                            "VFIO device reacquired by ember after swap"
                        );
                        held.insert(
                            bdf.to_string(),
                            HeldDevice {
                                bdf: bdf.to_string(),
                                device,
                            },
                        );
                        write_jsonrpc_ok(stream, id, serde_json::json!({"bdf": bdf}))?;
                    }
                    Err(e) => {
                        tracing::error!(bdf, error = %e, "failed to reacquire VFIO device");
                        write_jsonrpc_error(stream, id, -32000, &format!("reacquire failed: {e}"))?;
                    }
                }
            }
        }
        "ember.swap" => {
            let bdf = params
                .get("bdf")
                .and_then(|v| v.as_str())
                .ok_or("missing 'bdf' parameter")?;
            let target = params
                .get("target")
                .and_then(|v| v.as_str())
                .ok_or("missing 'target' parameter")?;
            match swap::handle_swap_device(bdf, target, held) {
                Ok(personality) => {
                    write_jsonrpc_ok(
                        stream,
                        id,
                        serde_json::json!({"bdf": bdf, "personality": personality}),
                    )?;
                }
                Err(e) => {
                    write_jsonrpc_error(stream, id, -32000, &e)?;
                }
            }
        }
        "ember.status" => {
            write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({
                    "devices": held.keys().cloned().collect::<Vec<_>>(),
                    "uptime_secs": started_at.elapsed().as_secs(),
                }),
            )?;
        }
        other => {
            write_jsonrpc_error(stream, id, -32601, &format!("method not found: {other}"))?;
        }
    }

    Ok(())
}

/// Send data with ancillary `SCM_RIGHTS` file descriptors (`rustix::net::sendmsg`).
pub fn send_with_fds(
    stream: impl AsFd,
    data: &[u8],
    fds: &[BorrowedFd<'_>],
) -> std::io::Result<()> {
    let iov = [IoSlice::new(data)];
    let mut space = vec![MaybeUninit::uninit(); SendAncillaryMessage::ScmRights(fds).size()];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !control.push(SendAncillaryMessage::ScmRights(fds)) {
        return Err(std::io::Error::other(
            "ancillary buffer too small for SCM_RIGHTS",
        ));
    }

    sendmsg(stream, &iov, &mut control, SendFlags::empty())?;
    Ok(())
}
