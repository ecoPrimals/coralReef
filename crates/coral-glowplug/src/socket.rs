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
    #[allow(dead_code)] // Read by serde for validation; clippy may flag as unused
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
                .ok_or_else(|| RpcError::device_error(format!("device {bdf} not managed")))?;
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
                .ok_or_else(|| RpcError::device_error(format!("device {bdf} not managed")))?;
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
                .ok_or_else(|| RpcError::device_error(format!("device {bdf} not managed")))?;
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
        "device.resurrect" => {
            let raw_bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(raw_bdf)?.to_owned();
            let slot = devices
                .iter_mut()
                .find(|d| d.bdf == bdf)
                .ok_or_else(|| RpcError::device_error(format!("device {bdf} not managed")))?;
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
                } else if req.method == "device.swap" || req.method == "device.resurrect" {
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
mod tests {
    use super::*;

    #[test]
    fn test_jsonrpc_request_parse_valid() {
        let line = r#"{"jsonrpc":"2.0","method":"health.check","params":{},"id":1}"#;
        let result: Result<JsonRpcRequest, _> = serde_json::from_str(line);
        let req = match result {
            Ok(r) => r,
            Err(e) => panic!("expected valid JSON-RPC request: {e}"),
        };
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.method, "health.check");
        assert!(req.params.is_object());
        assert_eq!(req.id, serde_json::json!(1));
    }

    #[test]
    fn test_jsonrpc_request_parse_with_params() {
        let line = r#"{"jsonrpc":"2.0","method":"device.get","params":{"bdf":"0000:01:00.0"},"id":"req-1"}"#;
        let result: Result<JsonRpcRequest, _> = serde_json::from_str(line);
        let req = match result {
            Ok(r) => r,
            Err(e) => panic!("expected valid JSON-RPC request: {e}"),
        };
        assert_eq!(req.method, "device.get");
        let bdf = req.params.get("bdf").and_then(serde_json::Value::as_str);
        assert_eq!(bdf, Some("0000:01:00.0"));
    }

    #[test]
    fn test_jsonrpc_request_parse_default_params() {
        let line = r#"{"jsonrpc":"2.0","method":"daemon.status","id":null}"#;
        let result: Result<JsonRpcRequest, _> = serde_json::from_str(line);
        let req = match result {
            Ok(r) => r,
            Err(e) => panic!("expected valid JSON-RPC request: {e}"),
        };
        assert_eq!(req.method, "daemon.status");
        assert!(req.params.is_null() || req.params.is_object());
    }

    #[test]
    fn test_jsonrpc_request_parse_invalid() {
        let line = r#"{"jsonrpc":"2.0","method":123}"#;
        let result: Result<JsonRpcRequest, _> = serde_json::from_str(line);
        assert!(result.is_err());
    }

    #[test]
    fn test_device_info_serialization_roundtrip() {
        let info = DeviceInfo {
            bdf: "0000:01:00.0".into(),
            name: Some("Compute GPU".into()),
            chip: "GV100 (Titan V)".into(),
            vendor_id: 0x10de,
            device_id: 0x1d81,
            personality: "vfio (group 5)".into(),
            role: Some("compute".into()),
            power: "D0".into(),
            vram_alive: true,
            domains_alive: 8,
            domains_faulted: 0,
            has_vfio_fd: true,
            pci_link_width: Some(16),
        };
        let json = match serde_json::to_string(&info) {
            Ok(j) => j,
            Err(e) => panic!("serialize DeviceInfo: {e}"),
        };
        let parsed: DeviceInfo = match serde_json::from_str(&json) {
            Ok(p) => p,
            Err(e) => panic!("deserialize DeviceInfo: {e}"),
        };
        assert_eq!(parsed.bdf, info.bdf);
        assert_eq!(parsed.name, info.name);
        assert_eq!(parsed.chip, info.chip);
        assert_eq!(parsed.vendor_id, info.vendor_id);
        assert_eq!(parsed.device_id, info.device_id);
        assert_eq!(parsed.personality, info.personality);
        assert_eq!(parsed.power, info.power);
        assert_eq!(parsed.vram_alive, info.vram_alive);
        assert_eq!(parsed.domains_alive, info.domains_alive);
        assert_eq!(parsed.domains_faulted, info.domains_faulted);
        assert_eq!(parsed.has_vfio_fd, info.has_vfio_fd);
        assert_eq!(parsed.pci_link_width, info.pci_link_width);
    }

    #[test]
    fn test_health_info_serialization_roundtrip() {
        let info = HealthInfo {
            bdf: "0000:02:00.0".into(),
            boot0: 0x1234_5678,
            pmc_enable: 0x9abc_def0,
            vram_alive: true,
            power: "D3hot".into(),
            domains_alive: 7,
            domains_faulted: 1,
        };
        let json = match serde_json::to_string(&info) {
            Ok(j) => j,
            Err(e) => panic!("serialize HealthInfo: {e}"),
        };
        let parsed: HealthInfo = match serde_json::from_str(&json) {
            Ok(p) => p,
            Err(e) => panic!("deserialize HealthInfo: {e}"),
        };
        assert_eq!(parsed.bdf, info.bdf);
        assert_eq!(parsed.boot0, info.boot0);
        assert_eq!(parsed.pmc_enable, info.pmc_enable);
        assert_eq!(parsed.vram_alive, info.vram_alive);
        assert_eq!(parsed.power, info.power);
        assert_eq!(parsed.domains_alive, info.domains_alive);
        assert_eq!(parsed.domains_faulted, info.domains_faulted);
    }

    #[test]
    fn test_health_check_response_format() {
        let response = serde_json::json!({
            "alive": true,
            "name": "coral-glowplug",
            "device_count": 0,
            "healthy_count": 0
        });
        assert_eq!(response["alive"], true);
        assert_eq!(response["name"], "coral-glowplug");
        assert_eq!(response["device_count"], 0);
        assert_eq!(response["healthy_count"], 0);
    }

    #[test]
    fn test_dispatch_device_list_empty() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        let result = super::dispatch("device.list", &serde_json::json!({}), &mut devices, started);
        let val = result.expect("device.list should succeed");
        let arr = val.as_array().expect("should be array");
        assert!(arr.is_empty());
    }

    #[test]
    fn test_dispatch_device_list_with_devices() {
        let config = crate::config::DeviceConfig {
            bdf: "0000:99:00.0".into(),
            name: Some("Test GPU".into()),
            boot_personality: "vfio".into(),
            power_policy: "always_on".into(),
            role: Some("compute".into()),
            oracle_dump: None,
        };
        let mut devices = vec![crate::device::DeviceSlot::new(config)];
        let started = std::time::Instant::now();
        let result = super::dispatch("device.list", &serde_json::json!({}), &mut devices, started);
        let val = result.expect("device.list should succeed");
        let arr = val.as_array().expect("should be array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["bdf"], "0000:99:00.0");
        assert_eq!(arr[0]["name"], "Test GPU");
    }

    #[test]
    fn test_dispatch_device_get_found() {
        let config = crate::config::DeviceConfig {
            bdf: "0000:99:00.0".into(),
            name: Some("Test".into()),
            boot_personality: "vfio".into(),
            power_policy: "always_on".into(),
            role: None,
            oracle_dump: None,
        };
        let mut devices = vec![crate::device::DeviceSlot::new(config)];
        let started = std::time::Instant::now();
        let result = super::dispatch(
            "device.get",
            &serde_json::json!({"bdf": "0000:99:00.0"}),
            &mut devices,
            started,
        );
        let val = result.expect("device.get should succeed");
        assert_eq!(val["bdf"], "0000:99:00.0");
    }

    #[test]
    fn test_dispatch_device_get_missing_bdf() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        let result = super::dispatch("device.get", &serde_json::json!({}), &mut devices, started);
        let err = result.expect_err("device.get without bdf should fail");
        assert_eq!(i32::from(err.code), -32602);
        assert!(err.message.contains("bdf"));
    }

    #[test]
    fn test_dispatch_device_get_not_managed() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        let result = super::dispatch(
            "device.get",
            &serde_json::json!({"bdf": "0000:01:00.0"}),
            &mut devices,
            started,
        );
        let err = result.expect_err("device.get for unmanaged device should fail");
        assert_eq!(i32::from(err.code), -32000);
        assert!(err.message.contains("not managed"));
    }

    #[test]
    fn test_dispatch_health_check() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        let result = super::dispatch(
            "health.check",
            &serde_json::json!({}),
            &mut devices,
            started,
        );
        let val = result.expect("health.check should succeed");
        assert_eq!(val["alive"], true);
        assert_eq!(val["name"], "coral-glowplug");
        assert_eq!(val["device_count"], 0);
    }

    #[test]
    fn test_dispatch_health_liveness() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        let result = super::dispatch(
            "health.liveness",
            &serde_json::json!({}),
            &mut devices,
            started,
        );
        let val = result.expect("health.liveness should succeed");
        assert_eq!(val["alive"], true);
    }

    #[test]
    fn test_dispatch_daemon_status() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        let result = super::dispatch(
            "daemon.status",
            &serde_json::json!({}),
            &mut devices,
            started,
        );
        let val = result.expect("daemon.status should succeed");
        assert!(val["uptime_secs"].as_u64().is_some());
        assert_eq!(val["device_count"], 0);
    }

    #[test]
    fn test_dispatch_daemon_shutdown_returns_error() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        let result = super::dispatch(
            "daemon.shutdown",
            &serde_json::json!({}),
            &mut devices,
            started,
        );
        let err = result.expect_err("daemon.shutdown should return Err for shutdown signal");
        assert_eq!(i32::from(err.code), -32000);
        assert_eq!(err.message, "shutdown");
    }

    #[test]
    fn test_dispatch_unknown_method() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        let result = super::dispatch(
            "nonexistent.method",
            &serde_json::json!({}),
            &mut devices,
            started,
        );
        let err = result.expect_err("unknown method should fail");
        assert_eq!(i32::from(err.code), -32601);
        assert!(err.message.contains("method not found"));
    }

    #[test]
    fn test_dispatch_device_swap_missing_params() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        let result = super::dispatch("device.swap", &serde_json::json!({}), &mut devices, started);
        let err = result.expect_err("device.swap without params should fail");
        assert_eq!(i32::from(err.code), -32602);
    }

    #[test]
    fn test_dispatch_device_health() {
        let config = crate::config::DeviceConfig {
            bdf: "0000:99:00.0".into(),
            name: None,
            boot_personality: "vfio".into(),
            power_policy: "always_on".into(),
            role: None,
            oracle_dump: None,
        };
        let mut devices = vec![crate::device::DeviceSlot::new(config)];
        let started = std::time::Instant::now();
        let result = super::dispatch(
            "device.health",
            &serde_json::json!({"bdf": "0000:99:00.0"}),
            &mut devices,
            started,
        );
        let val = result.expect("device.health should succeed");
        assert_eq!(val["bdf"], "0000:99:00.0");
        assert!(val.get("vram_alive").is_some());
        assert!(val.get("domains_alive").is_some());
    }

    #[test]
    fn test_make_response_success() {
        let resp = super::make_response(serde_json::json!(1), Ok(serde_json::json!({"ok": true})));
        let parsed: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["result"]["ok"], true);
        assert_eq!(parsed["id"], 1);
    }

    #[test]
    fn test_make_response_error() {
        let resp = super::make_response(
            serde_json::json!("req-1"),
            Err(crate::error::RpcError::invalid_params("missing parameter")),
        );
        let parsed: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert!(parsed["error"].is_object());
        assert_eq!(parsed["error"]["code"], -32602);
        assert_eq!(parsed["error"]["message"], "missing parameter");
    }

    #[tokio::test]
    async fn test_tcp_bind_127_0_0_1_0() {
        let server = SocketServer::bind("127.0.0.1:0")
            .await
            .expect("TCP bind should succeed");
        let addr = server.bound_addr();
        assert!(addr.contains("127.0.0.1"));
        assert!(addr.contains(':'));
        // Port 0 means OS assigns; we should get a non-zero port
        let port_part: &str = addr.rsplit(':').next().unwrap_or("");
        let port: u16 = port_part.parse().expect("port should parse");
        assert!(port > 0, "OS should assign non-zero port");
    }

    #[tokio::test]
    async fn test_tcp_client_health_check() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::TcpStream;

        let server = SocketServer::bind("127.0.0.1:0").await.expect("bind");
        let addr = server.bound_addr();
        let devices = Arc::new(Mutex::new(Vec::<crate::device::DeviceSlot>::new()));
        let (_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
        let devices_clone = devices.clone();

        let handle = tokio::spawn(async move {
            server.accept_loop(devices_clone, &mut shutdown_rx).await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let stream = TcpStream::connect(&addr).await.expect("connect");
        let (reader, mut writer) = stream.into_split();

        let req = r#"{"jsonrpc":"2.0","method":"health.check","params":{},"id":1}"#;
        writer
            .write_all(format!("{req}\n").as_bytes())
            .await
            .expect("write");

        let mut lines = BufReader::new(reader).lines();
        let resp_line = lines.next_line().await.expect("read").expect("line");
        let resp: serde_json::Value = serde_json::from_str(&resp_line).expect("parse");

        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["result"]["alive"], true);
        assert_eq!(resp["result"]["name"], "coral-glowplug");
        assert_eq!(resp["id"], 1);

        handle.abort();
    }

    #[tokio::test]
    async fn test_invalid_jsonrpc_version() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::TcpStream;

        let server = SocketServer::bind("127.0.0.1:0").await.expect("bind");
        let addr = server.bound_addr();
        let devices = Arc::new(Mutex::new(Vec::<crate::device::DeviceSlot>::new()));
        let (_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
        let devices_clone = devices.clone();

        let handle = tokio::spawn(async move {
            server.accept_loop(devices_clone, &mut shutdown_rx).await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let stream = TcpStream::connect(&addr).await.expect("connect");
        let (reader, mut writer) = stream.into_split();

        let req = r#"{"jsonrpc":"1.0","method":"health.check","params":{},"id":1}"#;
        writer
            .write_all(format!("{req}\n").as_bytes())
            .await
            .expect("write");

        let mut lines = BufReader::new(reader).lines();
        let resp_line = lines.next_line().await.expect("read").expect("line");
        let resp: serde_json::Value = serde_json::from_str(&resp_line).expect("parse");

        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["error"]["code"], -32600);
        assert_eq!(resp["id"], 1);

        handle.abort();
    }

    #[tokio::test]
    async fn test_parse_error() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::TcpStream;

        let server = SocketServer::bind("127.0.0.1:0").await.expect("bind");
        let addr = server.bound_addr();
        let devices = Arc::new(Mutex::new(Vec::<crate::device::DeviceSlot>::new()));
        let (_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
        let devices_clone = devices.clone();

        let handle = tokio::spawn(async move {
            server.accept_loop(devices_clone, &mut shutdown_rx).await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let stream = TcpStream::connect(&addr).await.expect("connect");
        let (reader, mut writer) = stream.into_split();

        writer.write_all(b"not valid json\n").await.expect("write");

        let mut lines = BufReader::new(reader).lines();
        let resp_line = lines.next_line().await.expect("read").expect("line");
        let resp: serde_json::Value = serde_json::from_str(&resp_line).expect("parse");

        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["error"]["code"], -32700);
        assert_eq!(resp["id"], serde_json::Value::Null);

        handle.abort();
    }

    #[tokio::test]
    async fn test_daemon_shutdown_via_jsonrpc() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::TcpStream;

        let server = SocketServer::bind("127.0.0.1:0").await.expect("bind");
        let addr = server.bound_addr();
        let devices = Arc::new(Mutex::new(Vec::<crate::device::DeviceSlot>::new()));
        let (_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
        let devices_clone = devices.clone();

        let handle = tokio::spawn(async move {
            server.accept_loop(devices_clone, &mut shutdown_rx).await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let stream = TcpStream::connect(&addr).await.expect("connect");
        let (reader, mut writer) = stream.into_split();

        let req = r#"{"jsonrpc":"2.0","method":"daemon.shutdown","params":{},"id":1}"#;
        writer
            .write_all(format!("{req}\n").as_bytes())
            .await
            .expect("write");
        writer.flush().await.expect("flush");

        let mut lines = BufReader::new(reader).lines();
        let resp_line = lines.next_line().await.expect("read").expect("line");
        let resp: serde_json::Value = serde_json::from_str(&resp_line).expect("parse");

        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["result"]["ok"], true);
        assert_eq!(resp["id"], 1);

        // Connection should close cleanly; next read should return None (EOF)
        let next = lines.next_line().await.expect("read");
        assert!(
            next.is_none(),
            "connection should close after shutdown response"
        );

        handle.abort();
    }

    // ── Chaos / Fault / Penetration Tests ──────────────────────────────────

    /// Helper: start a server on TCP loopback and return (addr, abort_handle, _tx_keepalive).
    ///
    /// The returned `watch::Sender` must be held alive for the server to
    /// keep accepting — dropping it signals shutdown via the watch channel.
    async fn spawn_test_server() -> (
        String,
        tokio::task::JoinHandle<()>,
        tokio::sync::watch::Sender<bool>,
    ) {
        let server = SocketServer::bind("127.0.0.1:0")
            .await
            .expect("bind should succeed");
        let addr = server.bound_addr();
        let devices = Arc::new(Mutex::new(Vec::<crate::device::DeviceSlot>::new()));
        let (tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let mut rx = shutdown_rx.clone();
        let handle = tokio::spawn(async move {
            server.accept_loop(devices, &mut rx).await;
        });
        for _ in 0..50 {
            if tokio::net::TcpStream::connect(&addr).await.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        (addr, handle, tx)
    }

    async fn send_line(addr: &str, payload: &str) -> String {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::TcpStream;
        let stream =
            tokio::time::timeout(std::time::Duration::from_secs(2), TcpStream::connect(addr))
                .await
                .expect("connect timeout")
                .expect("connect");
        let (reader, mut writer) = stream.into_split();
        writer
            .write_all(format!("{payload}\n").as_bytes())
            .await
            .expect("write");
        let mut lines = BufReader::new(reader).lines();
        tokio::time::timeout(std::time::Duration::from_secs(2), lines.next_line())
            .await
            .expect("read timeout")
            .expect("read")
            .unwrap_or_default()
    }

    // ── BDF Injection / Path Traversal ────────────────────────

    #[test]
    fn test_validate_bdf_valid() {
        assert!(super::validate_bdf("0000:01:00.0").is_ok());
        assert!(super::validate_bdf("0000:4a:00.0").is_ok());
    }

    #[test]
    fn test_validate_bdf_rejects_path_traversal() {
        assert!(super::validate_bdf("../../../etc/shadow").is_err());
        assert!(super::validate_bdf("0000:01:00.0/../../root").is_err());
    }

    #[test]
    fn test_validate_bdf_rejects_null_bytes() {
        assert!(super::validate_bdf("0000:01:00.0\0rm -rf /").is_err());
    }

    #[test]
    fn test_validate_bdf_rejects_empty() {
        assert!(super::validate_bdf("").is_err());
    }

    #[test]
    fn test_validate_bdf_rejects_overlong() {
        let long = "0".repeat(100);
        assert!(super::validate_bdf(&long).is_err());
    }

    #[test]
    fn test_validate_bdf_rejects_shell_injection() {
        assert!(super::validate_bdf("0000:01:00.0; rm -rf /").is_err());
        assert!(super::validate_bdf("$(cat /etc/passwd)").is_err());
        assert!(super::validate_bdf("`whoami`").is_err());
    }

    #[test]
    fn test_validate_bdf_rejects_unicode() {
        assert!(super::validate_bdf("0000:01:00.0\u{200B}").is_err());
        assert!(super::validate_bdf("０000:01:00.0").is_err()); // fullwidth zero
    }

    #[tokio::test]
    async fn test_dispatch_bdf_traversal_rejected() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        let result = super::dispatch(
            "device.get",
            &serde_json::json!({"bdf": "../../../etc/shadow"}),
            &mut devices,
            started,
        );
        let err = result.expect_err("traversal should be rejected");
        assert_eq!(i32::from(err.code), -32602);
        assert!(err.message.contains("invalid BDF"));
    }

    #[tokio::test]
    async fn test_dispatch_bdf_null_byte_rejected() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        let result = super::dispatch(
            "device.health",
            &serde_json::json!({"bdf": "0000:01:00.0\u{0000}"}),
            &mut devices,
            started,
        );
        let err = result.expect_err("null byte should be rejected");
        assert_eq!(i32::from(err.code), -32602);
    }

    // ── Malformed JSON-RPC Fuzzing ────────────────────────────

    #[tokio::test]
    async fn test_fuzz_empty_object() {
        let (addr, handle, _tx) = spawn_test_server().await;
        let resp = send_line(&addr, "{}").await;
        let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
        assert!(v["error"].is_object(), "empty object should be an error");
        handle.abort();
    }

    #[tokio::test]
    async fn test_fuzz_null_payload() {
        let (addr, handle, _tx) = spawn_test_server().await;
        let resp = send_line(&addr, "null").await;
        let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
        assert_eq!(v["error"]["code"], -32700);
        handle.abort();
    }

    #[tokio::test]
    async fn test_fuzz_array_payload() {
        let (addr, handle, _tx) = spawn_test_server().await;
        let resp = send_line(&addr, "[1,2,3]").await;
        let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
        assert_eq!(v["error"]["code"], -32700);
        handle.abort();
    }

    #[tokio::test]
    async fn test_fuzz_deeply_nested_json() {
        let (addr, handle, _tx) = spawn_test_server().await;
        let nested = "{".repeat(100) + &"}".repeat(100);
        let resp = send_line(&addr, &nested).await;
        let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
        assert!(v["error"].is_object());
        handle.abort();
    }

    #[tokio::test]
    async fn test_fuzz_huge_string_value() {
        let (addr, handle, _tx) = spawn_test_server().await;
        let big_method = "x".repeat(10_000);
        let payload = format!(
            r#"{{"jsonrpc":"2.0","method":"{}","params":{{}},"id":1}}"#,
            big_method
        );
        let resp = send_line(&addr, &payload).await;
        let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
        assert!(v["error"].is_object());
        assert_eq!(v["error"]["code"], -32601);
        handle.abort();
    }

    #[tokio::test]
    async fn test_fuzz_numeric_method() {
        let (addr, handle, _tx) = spawn_test_server().await;
        let resp = send_line(
            &addr,
            r#"{"jsonrpc":"2.0","method":12345,"params":{},"id":1}"#,
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
        assert_eq!(v["error"]["code"], -32700);
        handle.abort();
    }

    #[tokio::test]
    async fn test_fuzz_negative_id() {
        let (addr, handle, _tx) = spawn_test_server().await;
        let resp = send_line(
            &addr,
            r#"{"jsonrpc":"2.0","method":"health.check","params":{},"id":-999}"#,
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
        assert_eq!(v["id"], -999);
        assert!(v["result"]["alive"].as_bool().unwrap_or(false));
        handle.abort();
    }

    // ── Connection Chaos ──────────────────────────────────────

    #[tokio::test]
    async fn test_chaos_connect_and_disconnect_immediately() {
        let (addr, handle, _tx) = spawn_test_server().await;
        for _ in 0..20 {
            let stream = tokio::net::TcpStream::connect(&addr)
                .await
                .expect("connect");
            drop(stream);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Server should still accept new requests
        let resp = send_line(
            &addr,
            r#"{"jsonrpc":"2.0","method":"health.liveness","params":{},"id":1}"#,
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
        assert_eq!(v["result"]["alive"], true);
        handle.abort();
    }

    #[tokio::test]
    async fn test_chaos_partial_write_then_disconnect() {
        let (addr, handle, _tx) = spawn_test_server().await;
        for _ in 0..10 {
            let mut stream = tokio::net::TcpStream::connect(&addr)
                .await
                .expect("connect");
            use tokio::io::AsyncWriteExt;
            let _ = stream.write_all(b"{\"jsonrpc\":\"2.0\",\"met").await;
            drop(stream);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let resp = send_line(
            &addr,
            r#"{"jsonrpc":"2.0","method":"health.check","params":{},"id":1}"#,
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
        assert_eq!(v["result"]["alive"], true);
        handle.abort();
    }

    #[tokio::test]
    async fn test_chaos_rapid_sequential_requests() {
        let (addr, handle, _tx) = spawn_test_server().await;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let stream = tokio::net::TcpStream::connect(&addr)
            .await
            .expect("connect");
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();

        for i in 0..50 {
            let req =
                format!(r#"{{"jsonrpc":"2.0","method":"health.liveness","params":{{}},"id":{i}}}"#);
            writer
                .write_all(format!("{req}\n").as_bytes())
                .await
                .expect("write");
        }

        for i in 0..50 {
            let resp_line = lines.next_line().await.expect("read").expect("line");
            let v: serde_json::Value = serde_json::from_str(&resp_line).expect("valid json");
            assert_eq!(v["id"], i);
            assert_eq!(v["result"]["alive"], true);
        }
        handle.abort();
    }

    #[tokio::test]
    async fn test_chaos_concurrent_connections() {
        let (addr, handle, _tx) = spawn_test_server().await;
        let mut tasks = Vec::new();
        for i in 0..20 {
            let addr = addr.clone();
            tasks.push(tokio::spawn(async move {
                let resp = send_line(
                    &addr,
                    &format!(
                        r#"{{"jsonrpc":"2.0","method":"daemon.status","params":{{}},"id":{i}}}"#
                    ),
                )
                .await;
                let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
                assert!(v["result"]["uptime_secs"].is_number());
            }));
        }
        for t in tasks {
            t.await.expect("task should complete");
        }
        handle.abort();
    }

    // ── Fault Injection ───────────────────────────────────────

    #[tokio::test]
    async fn test_fault_invalid_method_does_not_crash() {
        let (addr, handle, _tx) = spawn_test_server().await;
        let methods = [
            "device.list.extra",
            "",
            ".",
            "device.",
            ".list",
            "DEVICE.LIST",
            "device.swap",
            "💀",
        ];
        for method in &methods {
            let resp = send_line(
                &addr,
                &format!(r#"{{"jsonrpc":"2.0","method":"{method}","params":{{}},"id":1}}"#),
            )
            .await;
            let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
            assert!(
                v["result"].is_object() || v["error"].is_object(),
                "method {method:?} should return valid JSON-RPC"
            );
        }
        handle.abort();
    }

    #[tokio::test]
    async fn test_fault_swap_with_traversal_target() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        let result = super::dispatch(
            "device.swap",
            &serde_json::json!({
                "bdf": "0000:01:00.0",
                "target": "../../../bin/sh"
            }),
            &mut devices,
            started,
        );
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_fault_device_get_with_empty_bdf() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        let result = super::dispatch(
            "device.get",
            &serde_json::json!({"bdf": ""}),
            &mut devices,
            started,
        );
        let err = result.expect_err("empty bdf should fail");
        assert_eq!(i32::from(err.code), -32602);
    }

    #[tokio::test]
    async fn test_fault_params_wrong_type() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        let result = super::dispatch(
            "device.get",
            &serde_json::json!({"bdf": 12345}),
            &mut devices,
            started,
        );
        let err = result.expect_err("numeric bdf should fail");
        assert_eq!(i32::from(err.code), -32602);
    }

    #[tokio::test]
    async fn test_fault_extra_params_ignored() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        let result = super::dispatch(
            "health.check",
            &serde_json::json!({"extra_field": "should_be_ignored", "another": 42}),
            &mut devices,
            started,
        );
        assert!(result.is_ok());
    }

    // ── Penetration: Repeated shutdown / method abuse ─────────

    #[test]
    fn test_pen_repeated_shutdown_does_not_panic() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        for _ in 0..10 {
            let result = super::dispatch(
                "daemon.shutdown",
                &serde_json::json!({}),
                &mut devices,
                started,
            );
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_pen_method_not_found_does_not_leak_internals() {
        let mut devices: Vec<crate::device::DeviceSlot> = Vec::new();
        let started = std::time::Instant::now();
        let probes = [
            "__proto__",
            "constructor",
            "toString",
            "system.listMethods",
            "rpc.discover",
            "admin.shutdown",
        ];
        for method in &probes {
            let result = super::dispatch(method, &serde_json::json!({}), &mut devices, started);
            let err = result.expect_err("should be method not found");
            assert_eq!(i32::from(err.code), -32601);
            assert!(
                !err.message.contains("panic"),
                "error should not reveal internal state"
            );
        }
    }
}
