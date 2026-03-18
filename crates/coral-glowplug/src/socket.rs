// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC 2.0 socket server for coral-glowplug (`ecoBin` compliant).
//!
//! Platform-agnostic IPC per `ecoBin` standard:
//! - **Unix**: primary transport is Unix domain socket
//! - **Non-Unix**: TCP fallback to `127.0.0.1:0` (OS-assigned port)
//!
//! Ecosystem primals connect via newline-delimited JSON-RPC
//! over either transport. The JSON-RPC dispatch logic is identical for both.
//!
//! ## Semantic methods
//!
//! | Method             | Description                                |
//! |--------------------|--------------------------------------------|
//! | `device.list`      | List all managed devices and capabilities   |
//! | `device.get`       | Get details for a specific device (by `BDF`)  |
//! | `device.swap`      | Hot-swap driver personality                 |
//! | `device.health`    | Query device health registers               |
//! | `device.resurrect` | Attempt HBM2 resurrection via nouveau       |
//! | `device.lend`      | Lend VFIO fd to an external consumer        |
//! | `device.reclaim`   | Reclaim a previously lent VFIO fd           |
//! | `health.check`     | Daemon health check                         |
//! | `health.liveness`  | Lightweight alive probe                     |
//! | `daemon.status`    | Daemon uptime and device count              |
//! | `daemon.shutdown`  | Graceful shutdown                           |

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UnixListener, UnixStream};
use tokio::sync::Mutex;

/// Maximum line length for a single JSON-RPC request (64 KiB).
/// Prevents memory exhaustion from malicious unbounded input.
const MAX_REQUEST_LINE_BYTES: usize = 64 * 1024;

/// Maximum concurrent client connections.
const MAX_CONCURRENT_CLIENTS: usize = 64;

/// Per-client request timeout (30 seconds idle = disconnect).
const CLIENT_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

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

/// Platform-agnostic JSON-RPC socket server (`ecoBin` compliant).
///
/// Binds to either a Unix domain socket path or a TCP address.
/// Use `SocketServer::bind` with a path (e.g. `/run/coralreef/glowplug.sock`)
/// or TCP address (e.g. `127.0.0.1:0`).
pub struct SocketServer {
    transport: Transport,
    pub started_at: std::time::Instant,
}

#[cfg(unix)]
enum Transport {
    Unix(UnixListener),
    Tcp(TcpListener),
}

#[cfg(not(unix))]
enum Transport {
    Tcp(TcpListener),
}

impl SocketServer {
    /// Bind to the given address.
    ///
    /// - If `addr` parses as a `SocketAddr` (e.g. `127.0.0.1:0`), binds TCP.
    /// - Otherwise, treats `addr` as a Unix socket path (Unix platforms only).
    ///
    /// On non-Unix platforms, only TCP addresses are supported.
    pub async fn bind(addr: &str) -> Result<Self, String> {
        let started_at = std::time::Instant::now();

        if let Ok(socket_addr) = addr.parse::<SocketAddr>() {
            // TCP transport
            let listener = TcpListener::bind(socket_addr)
                .await
                .map_err(|e| format!("bind TCP {addr}: {e}"))?;
            let bound = listener
                .local_addr()
                .map_err(|e| format!("get TCP local addr: {e}"))?;
            tracing::info!(%bound, "JSON-RPC 2.0 TCP server listening");
            Ok(Self {
                transport: Transport::Tcp(listener),
                started_at,
            })
        } else {
            // Unix transport (Unix platforms only)
            #[cfg(unix)]
            {
                if let Some(parent) = std::path::Path::new(addr).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::remove_file(addr);

                let listener =
                    UnixListener::bind(addr).map_err(|e| format!("bind Unix {addr}: {e}"))?;

                let _ = std::fs::set_permissions(
                    addr,
                    std::os::unix::fs::PermissionsExt::from_mode(0o666),
                );

                tracing::info!(path = %addr, "JSON-RPC 2.0 Unix socket server listening");
                Ok(Self {
                    transport: Transport::Unix(listener),
                    started_at,
                })
            }
            #[cfg(not(unix))]
            {
                Err(format!(
                    "Unix socket path not supported on this platform; use TCP address (e.g. {})",
                    crate::config::FALLBACK_TCP_BIND
                ))
            }
        }
    }

    /// Returns the bound address for display (e.g. in startup banner).
    ///
    /// For TCP with port 0, returns the actual bound address including the
    /// OS-assigned port.
    #[must_use]
    pub fn bound_addr(&self) -> String {
        match &self.transport {
            #[cfg(unix)]
            Transport::Unix(listener) => listener
                .local_addr()
                .ok()
                .and_then(|a| a.as_pathname().map(|p| format!("unix://{}", p.display())))
                .unwrap_or_else(|| "unix:(unknown)".to_owned()),
            Transport::Tcp(listener) => listener
                .local_addr()
                .map_or_else(|_| "tcp:(unknown)".to_owned(), |a| a.to_string()),
        }
    }

    pub async fn accept_loop(
        &self,
        devices: Arc<Mutex<Vec<crate::device::DeviceSlot>>>,
        shutdown: &mut tokio::sync::watch::Receiver<bool>,
    ) {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_CLIENTS));

        loop {
            let accept_fut = async {
                match &self.transport {
                    #[cfg(unix)]
                    Transport::Unix(listener) => match listener.accept().await {
                        Ok((stream, _addr)) => Some(ClientStream::Unix(stream)),
                        Err(e) => {
                            tracing::error!(error = %e, "Unix accept error");
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            None
                        }
                    },
                    Transport::Tcp(listener) => match listener.accept().await {
                        Ok((stream, _addr)) => Some(ClientStream::Tcp(stream)),
                        Err(e) => {
                            tracing::error!(error = %e, "TCP accept error");
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            None
                        }
                    },
                }
            };

            tokio::select! {
                accepted = accept_fut => {
                    if let Some(stream) = accepted {
                        let permit = match semaphore.clone().try_acquire_owned() {
                            Ok(p) => p,
                            Err(_) => {
                                tracing::warn!("max concurrent clients reached ({MAX_CONCURRENT_CLIENTS}), rejecting");
                                continue;
                            }
                        };
                        let devices = devices.clone();
                        let started_at = self.started_at;
                        tokio::spawn(async move {
                            if let Err(e) = handle_client(stream, devices, started_at).await {
                                tracing::warn!(error = %e, "client handler error");
                            }
                            drop(permit);
                        });
                    }
                }
                _ = shutdown.changed() => {
                    tracing::info!("accept loop: shutdown signal received");
                    return;
                }
            }
        }
    }
}

/// Client stream abstraction — Unix or TCP.
#[cfg(unix)]
enum ClientStream {
    Unix(UnixStream),
    Tcp(TcpStream),
}

#[cfg(not(unix))]
enum ClientStream {
    Tcp(TcpStream),
}

fn make_response(
    id: serde_json::Value,
    result: Result<serde_json::Value, crate::error::RpcError>,
) -> String {
    let resp = match result {
        Ok(val) => JsonRpcResponse {
            jsonrpc: "2.0",
            result: Some(val),
            error: None,
            id,
        },
        Err(e) => JsonRpcResponse {
            jsonrpc: "2.0",
            result: None,
            error: Some(JsonRpcError {
                code: e.code.into(),
                message: e.message,
            }),
            id,
        },
    };
    match serde_json::to_string(&resp) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "failed to serialize JSON-RPC response");
            r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"internal error"},"id":null}"#
                .to_owned()
        }
    }
}

/// Validate that a BDF string matches the expected PCI address format.
///
/// Rejects path traversal attempts, null bytes, and malformed addresses
/// that could be interpolated into sysfs paths by device operations.
fn validate_bdf(bdf: &str) -> Result<&str, crate::error::RpcError> {
    let is_valid = !bdf.is_empty()
        && bdf.len() <= 16
        && !bdf.contains('/')
        && !bdf.contains('\0')
        && !bdf.contains("..")
        && bdf
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.');
    if is_valid {
        Ok(bdf)
    } else {
        Err(crate::error::RpcError::invalid_params(format!(
            "invalid BDF address: {bdf:?}"
        )))
    }
}

fn dispatch(
    method: &str,
    params: &serde_json::Value,
    devices: &mut [crate::device::DeviceSlot],
    started_at: std::time::Instant,
) -> Result<serde_json::Value, crate::error::RpcError> {
    use crate::error::RpcError;

    match method {
        "device.list" => {
            let infos: Vec<DeviceInfo> = devices.iter().map(device_to_info).collect();
            serde_json::to_value(infos).map_err(|e| RpcError::internal(e.to_string()))
        }
        "device.get" => {
            let bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(bdf)?;
            let slot = devices
                .iter()
                .find(|d| d.bdf == bdf)
                .ok_or_else(|| crate::error::DeviceError::NotManaged {
                    bdf: bdf.to_string(),
                })
                .map_err(RpcError::from)?;
            serde_json::to_value(device_to_info(slot))
                .map_err(|e| RpcError::internal(e.to_string()))
        }
        "device.swap" => {
            let raw_bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(raw_bdf)?.to_owned();
            let target = params
                .get("target")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'target' parameter"))?
                .to_owned();
            let slot = devices
                .iter_mut()
                .find(|d| d.bdf == bdf)
                .ok_or_else(|| crate::error::DeviceError::NotManaged { bdf: bdf.clone() })
                .map_err(RpcError::from)?;
            slot.swap(&target)
                .map_err(|e| RpcError::device_error(e.to_string()))?;
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
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(bdf)?;
            let slot = devices
                .iter_mut()
                .find(|d| d.bdf == bdf)
                .ok_or_else(|| crate::error::DeviceError::NotManaged {
                    bdf: bdf.to_string(),
                })
                .map_err(RpcError::from)?;
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
            .map_err(|e| RpcError::internal(e.to_string()))
        }
        "device.lend" => {
            let raw_bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(raw_bdf)?.to_owned();
            let slot = devices
                .iter_mut()
                .find(|d| d.bdf == bdf)
                .ok_or_else(|| crate::error::DeviceError::NotManaged { bdf: bdf.clone() })
                .map_err(RpcError::from)?;
            let group_id = slot
                .lend()
                .map_err(|e| RpcError::device_error(e.to_string()))?;
            Ok(serde_json::json!({
                "bdf": bdf,
                "group_id": group_id,
                "personality": slot.personality.to_string(),
            }))
        }
        "device.reclaim" => {
            let raw_bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(raw_bdf)?.to_owned();
            let slot = devices
                .iter_mut()
                .find(|d| d.bdf == bdf)
                .ok_or_else(|| crate::error::DeviceError::NotManaged { bdf: bdf.clone() })
                .map_err(RpcError::from)?;
            slot.reclaim()
                .map_err(|e| RpcError::device_error(e.to_string()))?;
            Ok(serde_json::json!({
                "bdf": bdf,
                "personality": slot.personality.to_string(),
                "vram_alive": slot.health.vram_alive,
                "has_vfio_fd": slot.has_vfio(),
            }))
        }
        "device.resurrect" => {
            let raw_bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(raw_bdf)?.to_owned();
            let slot = devices
                .iter_mut()
                .find(|d| d.bdf == bdf)
                .ok_or_else(|| crate::error::DeviceError::NotManaged { bdf: bdf.clone() })
                .map_err(RpcError::from)?;
            let alive = slot
                .resurrect_hbm2()
                .map_err(|e| RpcError::device_error(e.to_string()))?;
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
            Err(RpcError::device_error("shutdown"))
        }
        other => Err(RpcError::method_not_found(other)),
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
    stream: ClientStream,
    devices: Arc<Mutex<Vec<crate::device::DeviceSlot>>>,
    started_at: std::time::Instant,
) -> Result<(), String> {
    match stream {
        #[cfg(unix)]
        ClientStream::Unix(s) => handle_client_stream(s, devices, started_at).await,
        ClientStream::Tcp(s) => handle_client_stream(s, devices, started_at).await,
    }
}

/// Generic JSON-RPC handler — identical logic for Unix and TCP (ecoBin).
///
/// Hardened against:
/// - Unbounded line length (capped at `MAX_REQUEST_LINE_BYTES`)
/// - Idle connections (disconnected after `CLIENT_IDLE_TIMEOUT`)
/// - Rapid request flooding (bounded by line-buffered I/O)
async fn handle_client_stream<S>(
    stream: S,
    devices: Arc<Mutex<Vec<crate::device::DeviceSlot>>>,
    started_at: std::time::Instant,
) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::with_capacity(MAX_REQUEST_LINE_BYTES, reader).lines();

    loop {
        let line = match tokio::time::timeout(CLIENT_IDLE_TIMEOUT, lines.next_line()).await {
            Ok(Ok(Some(l))) => l,
            Ok(Ok(None)) => break,
            Ok(Err(e)) => {
                if e.kind() == std::io::ErrorKind::InvalidData
                    || e.to_string().contains("stream capacity")
                {
                    tracing::warn!(error = %e, "oversized or malformed request — disconnecting");
                } else {
                    tracing::error!(error = %e, "client read error");
                }
                break;
            }
            Err(_) => {
                tracing::debug!("client idle timeout — disconnecting");
                break;
            }
        };

        if line.len() > MAX_REQUEST_LINE_BYTES {
            tracing::warn!(
                len = line.len(),
                max = MAX_REQUEST_LINE_BYTES,
                "request exceeds max line length — disconnecting"
            );
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let resp = match serde_json::from_str::<JsonRpcRequest>(line) {
            Ok(req) => {
                if req.jsonrpc != "2.0" {
                    make_response(
                        req.id,
                        Err(crate::error::RpcError {
                            code: crate::error::RpcErrorCode::INVALID_REQUEST,
                            message: format!("invalid jsonrpc version: {}", req.jsonrpc),
                        }),
                    )
                } else if matches!(
                    req.method.as_str(),
                    "device.swap" | "device.resurrect" | "device.lend" | "device.reclaim"
                ) {
                    let result = {
                        let mut devs = devices.lock().await;
                        dispatch(&req.method, &req.params, &mut devs, started_at)
                    };
                    make_response(req.id, result)
                } else {
                    let result = {
                        let mut devs = devices.lock().await;
                        dispatch(&req.method, &req.params, &mut devs, started_at)
                    };
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
                Err(crate::error::RpcError {
                    code: crate::error::RpcErrorCode::PARSE_ERROR,
                    message: format!("parse error: {e}"),
                }),
            ),
        };

        let msg = format!("{resp}\n");
        if writer.write_all(msg.as_bytes()).await.is_err() {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "socket_tests.rs"]
mod socket_tests;
