// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC 2.0 Unix socket server for coral-glowplug.
//!
//! Evolved from custom Request/Response enums to proper JSON-RPC 2.0
//! per hotSpring P1 requirement. toadStool (and other consumers) connect
//! via newline-delimited JSON-RPC over a Unix domain socket.
//!
//! ## Semantic methods
//!
//! | Method             | Description                                |
//! |--------------------|--------------------------------------------|
//! | `device.list`      | List all managed devices and capabilities   |
//! | `device.get`       | Get details for a specific device (by BDF)  |
//! | `device.swap`      | Hot-swap driver personality                 |
//! | `device.health`    | Query device health registers               |
//! | `device.resurrect` | Attempt HBM2 resurrection via nouveau       |
//! | `health.check`     | Daemon health check                         |
//! | `health.liveness`  | Lightweight alive probe                     |
//! | `daemon.status`    | Daemon uptime and device count              |
//! | `daemon.shutdown`  | Graceful shutdown                           |

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio::sync::Mutex;

#[derive(Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
    id: serde_json::Value,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
    id: serde_json::Value,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceInfo {
    pub bdf: String,
    pub name: Option<String>,
    pub chip: String,
    pub vendor_id: u16,
    pub device_id: u16,
    pub personality: String,
    pub role: Option<String>,
    pub power: String,
    pub vram_alive: bool,
    pub domains_alive: usize,
    pub domains_faulted: usize,
    pub has_vfio_fd: bool,
    pub pci_link_width: Option<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthInfo {
    pub bdf: String,
    pub boot0: u32,
    pub pmc_enable: u32,
    pub vram_alive: bool,
    pub power: String,
    pub domains_alive: usize,
    pub domains_faulted: usize,
}

pub struct SocketServer {
    listener: UnixListener,
    pub started_at: std::time::Instant,
}

impl SocketServer {
    pub async fn bind(path: &str) -> Result<Self, String> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::remove_file(path);

        let listener = UnixListener::bind(path).map_err(|e| format!("bind {path}: {e}"))?;

        let _ = std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o666));

        tracing::info!(path, "JSON-RPC 2.0 socket server listening");
        Ok(Self {
            listener,
            started_at: std::time::Instant::now(),
        })
    }

    pub async fn accept_loop(&self, devices: Arc<Mutex<Vec<crate::device::DeviceSlot>>>) {
        loop {
            match self.listener.accept().await {
                Ok((stream, _addr)) => {
                    let devices = devices.clone();
                    let started_at = self.started_at;
                    tokio::spawn(async move {
                        if let Err(e) = handle_client(stream, devices, started_at).await {
                            tracing::warn!(error = %e, "client handler error");
                        }
                    });
                }
                Err(e) => {
                    tracing::error!(error = %e, "accept error");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }
}

fn make_response(
    id: serde_json::Value,
    result: Result<serde_json::Value, (i32, String)>,
) -> String {
    let resp = match result {
        Ok(val) => JsonRpcResponse {
            jsonrpc: "2.0",
            result: Some(val),
            error: None,
            id,
        },
        Err((code, msg)) => JsonRpcResponse {
            jsonrpc: "2.0",
            result: None,
            error: Some(JsonRpcError { code, message: msg }),
            id,
        },
    };
    serde_json::to_string(&resp).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"internal error"},"id":null}"#
            .to_owned()
    })
}

fn dispatch(
    method: &str,
    params: &serde_json::Value,
    devices: &mut [crate::device::DeviceSlot],
    started_at: std::time::Instant,
) -> Result<serde_json::Value, (i32, String)> {
    match method {
        "device.list" => {
            let infos: Vec<DeviceInfo> = devices.iter().map(device_to_info).collect();
            serde_json::to_value(infos).map_err(|e| (-32603, e.to_string()))
        }
        "device.get" => {
            let bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or((-32602, "missing 'bdf' parameter".into()))?;
            let slot = devices
                .iter()
                .find(|d| d.bdf == bdf)
                .ok_or((-32000, format!("device {bdf} not managed")))?;
            serde_json::to_value(device_to_info(slot)).map_err(|e| (-32603, e.to_string()))
        }
        "device.swap" => {
            let bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or((-32602, "missing 'bdf' parameter".into()))?
                .to_owned();
            let target = params
                .get("target")
                .and_then(serde_json::Value::as_str)
                .ok_or((-32602, "missing 'target' parameter".into()))?
                .to_owned();
            let slot = devices
                .iter_mut()
                .find(|d| d.bdf == bdf)
                .ok_or((-32000, format!("device {bdf} not managed")))?;
            slot.swap(&target).map_err(|e| (-32000, e))?;
            Ok(serde_json::json!({
                "bdf": bdf,
                "personality": slot.personality.to_string(),
                "vram_alive": slot.health.vram_alive,
            }))
        }
        "device.health" => {
            let bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or((-32602, "missing 'bdf' parameter".into()))?;
            let slot = devices
                .iter_mut()
                .find(|d| d.bdf == bdf)
                .ok_or((-32000, format!("device {bdf} not managed")))?;
            slot.check_health();
            serde_json::to_value(HealthInfo {
                bdf: bdf.to_owned(),
                boot0: slot.health.boot0,
                pmc_enable: slot.health.pmc_enable,
                vram_alive: slot.health.vram_alive,
                power: slot.health.power.to_string(),
                domains_alive: slot.health.domains_alive,
                domains_faulted: slot.health.domains_faulted,
            })
            .map_err(|e| (-32603, e.to_string()))
        }
        "device.resurrect" => {
            let bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or((-32602, "missing 'bdf' parameter".into()))?
                .to_owned();
            let slot = devices
                .iter_mut()
                .find(|d| d.bdf == bdf)
                .ok_or((-32000, format!("device {bdf} not managed")))?;
            let alive = slot.resurrect_hbm2().map_err(|e| (-32000, e))?;
            Ok(serde_json::json!({
                "bdf": bdf,
                "vram_alive": alive,
                "domains_alive": slot.health.domains_alive,
            }))
        }
        "health.check" | "health.liveness" => Ok(serde_json::json!({
            "alive": true,
            "name": "coral-glowplug",
            "device_count": devices.len(),
            "healthy_count": devices.iter().filter(|d| d.health.vram_alive).count(),
        })),
        "daemon.status" => Ok(serde_json::json!({
            "uptime_secs": started_at.elapsed().as_secs(),
            "device_count": devices.len(),
            "healthy_count": devices.iter().filter(|d| d.health.vram_alive).count(),
        })),
        "daemon.shutdown" => {
            tracing::info!("shutdown requested via JSON-RPC");
            Err((-32000, "shutdown".into()))
        }
        other => Err((-32601, format!("method not found: {other}"))),
    }
}

fn device_to_info(d: &crate::device::DeviceSlot) -> DeviceInfo {
    DeviceInfo {
        bdf: d.bdf.clone(),
        name: d.config.name.clone(),
        chip: d.chip_name.clone(),
        vendor_id: d.vendor_id,
        device_id: d.device_id,
        personality: d.personality.to_string(),
        role: d.config.role.clone(),
        power: d.health.power.to_string(),
        vram_alive: d.health.vram_alive,
        domains_alive: d.health.domains_alive,
        domains_faulted: d.health.domains_faulted,
        has_vfio_fd: d.has_vfio(),
        pci_link_width: d.health.pci_link_width,
    }
}

async fn handle_client(
    stream: tokio::net::UnixStream,
    devices: Arc<Mutex<Vec<crate::device::DeviceSlot>>>,
    started_at: std::time::Instant,
) -> Result<(), String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_owned();
        if line.is_empty() {
            continue;
        }

        let resp = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(req) => {
                if req.jsonrpc != "2.0" {
                    make_response(
                        req.id,
                        Err((-32600, format!("invalid jsonrpc version: {}", req.jsonrpc))),
                    )
                } else if req.method == "device.swap" || req.method == "device.resurrect" {
                    let devs_clone = devices.clone();
                    let method = req.method.clone();
                    let params = req.params.clone();
                    let id = req.id.clone();
                    match tokio::task::spawn_blocking(move || {
                        let rt = tokio::runtime::Handle::current();
                        let mut devs = rt.block_on(devs_clone.lock());
                        dispatch(&method, &params, &mut devs, started_at)
                    })
                    .await
                    {
                        Ok(result) => make_response(id, result),
                        Err(e) => {
                            make_response(req.id, Err((-32603, format!("spawn_blocking: {e}"))))
                        }
                    }
                } else {
                    let mut devs = devices.lock().await;
                    let result = dispatch(&req.method, &req.params, &mut devs, started_at);
                    if req.method == "daemon.shutdown" {
                        let resp_str = make_response(req.id, Ok(serde_json::json!({"ok": true})));
                        let msg = format!("{resp_str}\n");
                        let _ = writer.write_all(msg.as_bytes()).await;
                        return Ok(());
                    }
                    make_response(req.id, result)
                }
            }
            Err(e) => make_response(
                serde_json::Value::Null,
                Err((-32700, format!("parse error: {e}"))),
            ),
        };

        let msg = format!("{resp}\n");
        if writer.write_all(msg.as_bytes()).await.is_err() {
            break;
        }
    }

    Ok(())
}
