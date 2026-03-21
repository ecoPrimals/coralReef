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

/// Incoming JSON-RPC 2.0 request line (single object per connection read).
#[derive(Deserialize)]
pub struct JsonRpcRequest {
    /// Must be `"2.0"`.
    pub jsonrpc: String,
    /// Method name (e.g. `ember.list`).
    pub method: String,
    #[serde(default)]
    /// Method parameters object.
    pub params: serde_json::Value,
    /// Request correlation id.
    pub id: serde_json::Value,
}

/// JSON-RPC 2.0 response envelope.
#[derive(Serialize)]
pub struct JsonRpcResponse {
    /// Protocol version (`"2.0"`).
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Success payload.
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Error payload.
    pub error: Option<JsonRpcError>,
    /// Matches the request `id`.
    pub id: serde_json::Value,
}

/// JSON-RPC error object.
#[derive(Serialize)]
pub struct JsonRpcError {
    /// JSON-RPC error code.
    pub code: i32,
    /// Human-readable message.
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

/// Handle one JSON-RPC request on `stream` (read one line, dispatch, write response).
///
/// For `ember.vfio_fds`, sends the JSON line first, then passes three fds via `SCM_RIGHTS`.
///
/// # Errors
///
/// Returns `Err` when a required parameter is missing for a method that uses `?` (e.g. `ember.swap`
/// without `target`); socket write/serialize errors are returned as `Err` strings.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::os::fd::AsFd;
    use std::os::unix::net::UnixStream;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    static IPC_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn drain_json_line(stream: &mut UnixStream) -> serde_json::Value {
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            stream.read_exact(&mut byte).unwrap();
            if byte[0] == b'\n' {
                break;
            }
            buf.push(byte[0]);
        }
        let s = std::str::from_utf8(&buf).unwrap();
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn handle_client_empty_read_returns_ok() {
        let (a, b) = UnixStream::pair().unwrap();
        drop(b);
        let mut held = HashMap::new();
        handle_client(&a, &mut held, Instant::now()).unwrap();
        drop(a);
    }

    #[test]
    fn handle_client_invalid_json_emits_parse_error() {
        let _guard = IPC_TEST_LOCK.lock().unwrap();
        let (server, mut client) = UnixStream::pair().unwrap();
        client.write_all(b"not json\n").unwrap();
        let mut held = HashMap::new();
        handle_client(&server, &mut held, Instant::now()).unwrap();
        let v = drain_json_line(&mut client);
        assert_eq!(v["error"]["code"].as_i64().unwrap(), -32700);
    }

    #[test]
    fn handle_client_wrong_jsonrpc_version() {
        let _guard = IPC_TEST_LOCK.lock().unwrap();
        let (server, mut client) = UnixStream::pair().unwrap();
        let req = r#"{"jsonrpc":"1.0","method":"ember.list","id":1}"#;
        client.write_all(req.as_bytes()).unwrap();
        client.write_all(b"\n").unwrap();
        let mut held = HashMap::new();
        handle_client(&server, &mut held, Instant::now()).unwrap();
        let v = drain_json_line(&mut client);
        assert_eq!(v["error"]["code"].as_i64().unwrap(), -32600);
    }

    #[test]
    fn handle_client_ember_list_empty() {
        let _guard = IPC_TEST_LOCK.lock().unwrap();
        let (server, mut client) = UnixStream::pair().unwrap();
        let req = r#"{"jsonrpc":"2.0","method":"ember.list","id":7}"#;
        client.write_all(req.as_bytes()).unwrap();
        client.write_all(b"\n").unwrap();
        let mut held = HashMap::new();
        handle_client(&server, &mut held, Instant::now()).unwrap();
        let v = drain_json_line(&mut client);
        assert_eq!(v["result"]["devices"], serde_json::json!([]));
    }

    #[test]
    fn handle_client_ember_status_reports_uptime() {
        let _guard = IPC_TEST_LOCK.lock().unwrap();
        let (server, mut client) = UnixStream::pair().unwrap();
        let req = r#"{"jsonrpc":"2.0","method":"ember.status","id":2}"#;
        client.write_all(req.as_bytes()).unwrap();
        client.write_all(b"\n").unwrap();
        let started = Instant::now() - Duration::from_secs(10);
        let mut held = HashMap::new();
        handle_client(&server, &mut held, started).unwrap();
        let v = drain_json_line(&mut client);
        let uptime = v["result"]["uptime_secs"].as_u64().unwrap();
        assert!(uptime >= 10);
    }

    #[test]
    fn handle_client_unknown_method() {
        let _guard = IPC_TEST_LOCK.lock().unwrap();
        let (server, mut client) = UnixStream::pair().unwrap();
        let req = r#"{"jsonrpc":"2.0","method":"nope.not_found","id":3}"#;
        client.write_all(req.as_bytes()).unwrap();
        client.write_all(b"\n").unwrap();
        let mut held = HashMap::new();
        handle_client(&server, &mut held, Instant::now()).unwrap();
        let v = drain_json_line(&mut client);
        assert_eq!(v["error"]["code"].as_i64().unwrap(), -32601);
    }

    #[test]
    fn handle_client_ember_vfio_fds_missing_device() {
        let _guard = IPC_TEST_LOCK.lock().unwrap();
        let (server, mut client) = UnixStream::pair().unwrap();
        let req =
            r#"{"jsonrpc":"2.0","method":"ember.vfio_fds","params":{"bdf":"0000:01:00.0"},"id":4}"#;
        client.write_all(req.as_bytes()).unwrap();
        client.write_all(b"\n").unwrap();
        let mut held = HashMap::new();
        handle_client(&server, &mut held, Instant::now()).unwrap();
        let v = drain_json_line(&mut client);
        assert_eq!(v["error"]["code"].as_i64().unwrap(), -32000);
    }

    #[test]
    fn handle_client_ember_release_missing_bdf_errors() {
        let _guard = IPC_TEST_LOCK.lock().unwrap();
        let (server, mut client) = UnixStream::pair().unwrap();
        let req = r#"{"jsonrpc":"2.0","method":"ember.release","params":{},"id":5}"#;
        client.write_all(req.as_bytes()).unwrap();
        client.write_all(b"\n").unwrap();
        let mut held = HashMap::new();
        let err = handle_client(&server, &mut held, Instant::now()).unwrap_err();
        assert!(err.contains("bdf"));
    }

    #[test]
    fn handle_client_ember_release_not_held() {
        let _guard = IPC_TEST_LOCK.lock().unwrap();
        let (server, mut client) = UnixStream::pair().unwrap();
        let req =
            r#"{"jsonrpc":"2.0","method":"ember.release","params":{"bdf":"0000:01:00.0"},"id":6}"#;
        client.write_all(req.as_bytes()).unwrap();
        client.write_all(b"\n").unwrap();
        let mut held = HashMap::new();
        handle_client(&server, &mut held, Instant::now()).unwrap();
        let v = drain_json_line(&mut client);
        assert_eq!(v["error"]["code"].as_i64().unwrap(), -32000);
    }

    #[test]
    fn handle_client_ember_swap_missing_params() {
        let _guard = IPC_TEST_LOCK.lock().unwrap();
        let (server, mut client) = UnixStream::pair().unwrap();
        let req =
            r#"{"jsonrpc":"2.0","method":"ember.swap","params":{"bdf":"0000:01:00.0"},"id":8}"#;
        client.write_all(req.as_bytes()).unwrap();
        client.write_all(b"\n").unwrap();
        let mut held = HashMap::new();
        let err = handle_client(&server, &mut held, Instant::now()).unwrap_err();
        assert!(err.contains("target"));
    }

    #[test]
    fn handle_client_ember_reacquire_open_failure_returns_jsonrpc_error() {
        let _guard = IPC_TEST_LOCK.lock().unwrap();
        let (server, mut client) = UnixStream::pair().unwrap();
        let req = r#"{"jsonrpc":"2.0","method":"ember.reacquire","params":{"bdf":"9999:99:99.9"},"id":11}"#;
        client.write_all(req.as_bytes()).unwrap();
        client.write_all(b"\n").unwrap();
        let mut held = HashMap::new();
        handle_client(&server, &mut held, Instant::now()).unwrap();
        let v = drain_json_line(&mut client);
        assert_eq!(v["error"]["code"].as_i64().unwrap(), -32000);
        let msg = v["error"]["message"].as_str().unwrap();
        assert!(msg.contains("reacquire"));
    }

    #[test]
    fn handle_client_ember_swap_reports_error_from_swap() {
        let _guard = IPC_TEST_LOCK.lock().unwrap();
        let (server, mut client) = UnixStream::pair().unwrap();
        let req = r#"{"jsonrpc":"2.0","method":"ember.swap","params":{"bdf":"9999:99:99.9","target":"bogus-target"},"id":9}"#;
        client.write_all(req.as_bytes()).unwrap();
        client.write_all(b"\n").unwrap();
        let mut held = HashMap::new();
        handle_client(&server, &mut held, Instant::now()).unwrap();
        let v = drain_json_line(&mut client);
        assert_eq!(v["error"]["code"].as_i64().unwrap(), -32000);
        let msg = v["error"]["message"].as_str().unwrap();
        assert!(msg.contains("unknown target"));
    }

    #[test]
    fn make_jsonrpc_ok_shape() {
        let v = make_jsonrpc_ok(serde_json::json!(1), serde_json::json!({"a": 1}));
        assert_eq!(v.jsonrpc, "2.0");
        assert!(v.error.is_none());
        assert_eq!(v.result, Some(serde_json::json!({"a": 1})));
    }

    #[test]
    fn send_with_fds_unix_stream() {
        let (a, _b) = UnixStream::pair().unwrap();
        let file = std::fs::File::open("/dev/null").unwrap();
        let fds = [file.as_fd()];
        send_with_fds(&a, b"ok", &fds).unwrap();
    }

    #[test]
    #[ignore = "requires GPU bound to vfio-pci and a real BDF"]
    fn handle_client_ember_vfio_fds_with_hardware() {
        let bdf = std::env::var("CORAL_EMBER_TEST_BDF").expect("set CORAL_EMBER_TEST_BDF");
        let (server, mut client) = UnixStream::pair().unwrap();
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"ember.vfio_fds","params":{{"bdf":"{bdf}"}},"id":1}}"#
        );
        client.write_all(req.as_bytes()).unwrap();
        client.write_all(b"\n").unwrap();
        let mut held = HashMap::new();
        let device = coral_driver::vfio::VfioDevice::open(&bdf).unwrap();
        held.insert(bdf.clone(), crate::hold::HeldDevice { bdf, device });
        handle_client(&server, &mut held, Instant::now()).unwrap();
    }
}
