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
//! | `device.resurrect` | Attempt HBM2 resurrection via nouveau warm swap |
//! | `device.reset`     | PCIe Function Level Reset via VFIO              |
//! | `device.write_register` | Write a single BAR0 register            |
//! | `device.read_bar0_range` | Read contiguous BAR0 register range    |
//! | `device.pramin_read` | Read VRAM via PRAMIN window                |
//! | `device.pramin_write` | Write VRAM via PRAMIN window              |
//! | `device.register_dump` | Dump key BAR0 registers for a device    |
//! | `device.register_snapshot` | Save timestamped register snapshot to JSON |
//! | `device.lend`      | Lend VFIO fd to an external consumer        |
//! | `device.reclaim`   | Reclaim a previously lent VFIO fd           |
//! | `health.check`     | Daemon health check                         |
//! | `health.liveness`  | Lightweight alive probe                     |
//! | `device.oracle_capture` | Capture MMU page tables via daemon (no VFIO access needed) |
//! | `device.dispatch`  | Submit compute work (shader + buffers) through the daemon |
//! | `device.compute_info` | Query NVML telemetry for a GPU            |
//! | `device.quota`     | Query compute quota for shared/display GPU   |
//! | `device.set_quota` | Set compute quota (power limit, mode)        |
//! | `daemon.status`    | Daemon uptime and device count              |
//! | `daemon.shutdown`  | Graceful shutdown                           |

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UnixListener, UnixStream};
use tokio::sync::Mutex;

/// Maximum line length for a single JSON-RPC request (4 MiB).
/// Sized for compute dispatch payloads (base64-encoded shader + buffers)
/// while still bounding memory from unbounded input.
const MAX_REQUEST_LINE_BYTES: usize = 4 * 1024 * 1024;

/// Initial per-connection read buffer (64 KiB).
/// Tokio's `BufReader` grows on demand up to `MAX_REQUEST_LINE_BYTES` via
/// `lines()`, so idle connections only use this smaller allocation.
const INITIAL_BUF_CAPACITY: usize = 64 * 1024;

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
    /// True when the device has `role = "display"` and is immune to swaps.
    #[serde(default)]
    pub protected: bool,
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

/// Set socket group ownership for unprivileged user access.
///
/// Resolves `group_name` from `/etc/group` and chowns the socket.
/// The glowplug socket should be `root:coralreef 0660` so users in the
/// `coralreef` group can send RPC commands without privilege escalation.
/// Ember's socket stays `root:root 0660` (service-to-service only).
#[cfg(unix)]
fn set_socket_group(path: &str, group_name: &str) {
    let gid = match resolve_group_gid(group_name) {
        Some(gid) => gid,
        None => {
            tracing::warn!(
                group = group_name,
                path,
                "group not found — socket remains root:root. \
                 Create with: sudo groupadd -r {group_name} && sudo usermod -aG {group_name} $USER"
            );
            return;
        }
    };

    match std::os::unix::fs::chown(path, None, Some(gid)) {
        Ok(()) => {
            tracing::info!(
                path,
                group = group_name,
                gid,
                "socket group set — unprivileged RPC enabled"
            );
        }
        Err(e) => {
            tracing::warn!(path, group = group_name, gid, error = %e, "failed to chown socket");
        }
    }
}

#[cfg(unix)]
fn resolve_group_gid(group_name: &str) -> Option<u32> {
    let content = std::fs::read_to_string("/etc/group").ok()?;
    for line in content.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 3 && fields[0] == group_name {
            return fields[2].parse().ok();
        }
    }
    None
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
                    std::os::unix::fs::PermissionsExt::from_mode(0o660),
                );

                let group =
                    std::env::var("CORALREEF_SOCKET_GROUP").unwrap_or_else(|_| "coralreef".into());
                set_socket_group(addr, &group);

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
                    coral_glowplug::config::FALLBACK_TCP_BIND
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
        devices: Arc<Mutex<Vec<coral_glowplug::device::DeviceSlot>>>,
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
                        let Ok(permit) = semaphore.clone().try_acquire_owned() else {
                            tracing::warn!("max concurrent clients reached ({MAX_CONCURRENT_CLIENTS}), rejecting");
                            continue;
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
    result: Result<serde_json::Value, coral_glowplug::error::RpcError>,
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
fn validate_bdf(bdf: &str) -> Result<&str, coral_glowplug::error::RpcError> {
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
        Err(coral_glowplug::error::RpcError::invalid_params(format!(
            "invalid BDF address: {bdf:?}"
        )))
    }
}

/// Run oracle capture off the async event loop so it doesn't block the
/// watchdog or other RPC handlers.
async fn oracle_capture_async(
    params: &serde_json::Value,
    devices: &Mutex<Vec<coral_glowplug::device::DeviceSlot>>,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();
    let max_channels = params
        .get("max_channels")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    let (bar0_handle, _busy_guard) = {
        let devs = devices.lock().await;
        let slot = devs
            .iter()
            .find(|d| d.bdf.as_ref() == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::from(bdf.as_str()),
            })
            .map_err(RpcError::from)?;
        let guard = slot.try_acquire_busy().ok_or_else(|| {
            RpcError::device_error(format!("device {bdf} is busy with another long-running operation"))
        })?;
        (slot.vfio_bar0_handle(), guard)
    };

    let bdf_clone = bdf.clone();
    let result = tokio::task::spawn_blocking(move || {
        if let Some(handle) = bar0_handle {
            handle.capture_page_tables(&bdf_clone, max_channels)
        } else {
            coral_driver::vfio::channel::mmu_oracle::capture_page_tables(
                &bdf_clone,
                max_channels,
            )
        }
    })
    .await
    .map_err(|e| RpcError::internal(format!("oracle task panicked: {e}")))?
    .map_err(|e| RpcError::device_error(e))?;

    serde_json::to_value(&result).map_err(|e| RpcError::internal(e.to_string()))
}

/// Run compute dispatch off the async event loop via spawn_blocking.
///
/// Params:
///  - `bdf`:        target device BDF
///  - `shader`:     base64-encoded PTX (or native binary)
///  - `inputs`:     array of base64-encoded input buffers
///  - `output_sizes`: array of output buffer sizes (bytes)
///  - `dims`:       [x, y, z] workgroup grid dimensions
///  - `workgroup`:  [x, y, z] threads per workgroup (default [64,1,1])
///  - `shared_mem`: shared memory bytes (default 0)
async fn compute_dispatch_async(
    params: &serde_json::Value,
    devices: &Mutex<Vec<coral_glowplug::device::DeviceSlot>>,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;
    use coral_driver::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf'"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();

    let shader_b64 = params
        .get("shader")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'shader' (base64 PTX)"))?
        .to_owned();

    let inputs: Vec<String> = params
        .get("inputs")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let output_sizes: Vec<u64> = params
        .get("output_sizes")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default();

    let dims_arr = params
        .get("dims")
        .and_then(|v| v.as_array())
        .ok_or_else(|| RpcError::invalid_params("missing 'dims' [x,y,z]"))?;
    let dims = [
        dims_arr.first().and_then(|v| v.as_u64()).unwrap_or(1) as u32,
        dims_arr.get(1).and_then(|v| v.as_u64()).unwrap_or(1) as u32,
        dims_arr.get(2).and_then(|v| v.as_u64()).unwrap_or(1) as u32,
    ];

    let workgroup = params
        .get("workgroup")
        .and_then(|v| v.as_array())
        .map(|arr| {
            [
                arr.first().and_then(|v| v.as_u64()).unwrap_or(64) as u32,
                arr.get(1).and_then(|v| v.as_u64()).unwrap_or(1) as u32,
                arr.get(2).and_then(|v| v.as_u64()).unwrap_or(1) as u32,
            ]
        })
        .unwrap_or([64, 1, 1]);

    let shared_mem = params
        .get("shared_mem")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let kernel_name = params
        .get("kernel_name")
        .and_then(|v| v.as_str())
        .unwrap_or("main_kernel")
        .to_owned();

    // Validate device is managed and acquire busy guard
    let _busy_guard = {
        let devs = devices.lock().await;
        let slot = devs
            .iter()
            .find(|d| d.bdf.as_ref() == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::from(bdf.as_str()),
            })
            .map_err(RpcError::from)?;
        slot.try_acquire_busy().ok_or_else(|| {
            RpcError::device_error(format!("device {bdf} is busy with another long-running operation"))
        })?
    };

    let bdf_for_task = bdf.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Vec<Vec<u8>>, String> {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD;

        let shader_bytes = b64
            .decode(&shader_b64)
            .map_err(|e| format!("base64 decode shader: {e}"))?;

        let input_data: Vec<Vec<u8>> = inputs
            .iter()
            .map(|s| b64.decode(s).map_err(|e| format!("base64 decode input: {e}")))
            .collect::<Result<Vec<_>, _>>()?;

        let mut dev =
            coral_driver::cuda::CudaComputeDevice::from_bdf_hint(&bdf_for_task)
                .map_err(|e| format!("CUDA open for {bdf_for_task}: {e}"))?;

        let mut handles: Vec<BufferHandle> = Vec::new();

        // Allocate and upload input buffers
        for data in &input_data {
            let h = dev
                .alloc(data.len() as u64, MemoryDomain::VramOrGtt)
                .map_err(|e| format!("alloc input: {e}"))?;
            dev.upload(h, 0, data)
                .map_err(|e| format!("upload: {e}"))?;
            handles.push(h);
        }

        // Allocate output buffers
        let output_start = handles.len();
        for &size in &output_sizes {
            let h = dev
                .alloc(size, MemoryDomain::VramOrGtt)
                .map_err(|e| format!("alloc output: {e}"))?;
            handles.push(h);
        }

        let info = ShaderInfo {
            gpr_count: 0,
            shared_mem_bytes: shared_mem,
            barrier_count: 0,
            workgroup,
            wave_size: 32,
        };

        dev.dispatch_named(
            &shader_bytes,
            &handles,
            DispatchDims::new(dims[0], dims[1], dims[2]),
            &info,
            &kernel_name,
        )
        .map_err(|e| format!("dispatch: {e}"))?;

        dev.sync().map_err(|e| format!("sync: {e}"))?;

        // Readback output buffers
        let mut outputs = Vec::new();
        for (i, &size) in output_sizes.iter().enumerate() {
            let h = handles[output_start + i];
            let data = dev
                .readback(h, 0, size as usize)
                .map_err(|e| format!("readback: {e}"))?;
            outputs.push(data);
        }

        Ok(outputs)
    })
    .await
    .map_err(|e| RpcError::internal(format!("dispatch task panicked: {e}")))?
    .map_err(|e| RpcError::device_error(e))?;

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let output_b64: Vec<String> = result.iter().map(|d| b64.encode(d)).collect();

    Ok(serde_json::json!({
        "bdf": bdf,
        "outputs": output_b64,
        "output_count": output_b64.len(),
    }))
}

fn dispatch(
    method: &str,
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
    started_at: std::time::Instant,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

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
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf),
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
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf.as_str()),
                })
                .map_err(RpcError::from)?;
            if slot.is_busy() {
                return Err(RpcError::device_error(format!(
                    "device {bdf} is busy — cannot swap while a long-running operation is in progress"
                )));
            }
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
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf),
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
        "device.register_dump" => {
            let bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(bdf)?;
            let slot = devices
                .iter()
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf),
                })
                .map_err(RpcError::from)?;
            if !slot.has_vfio() {
                return Err(RpcError::device_error(format!(
                    "device {bdf} has no VFIO fd — register reads require VFIO personality"
                )));
            }
            let custom_offsets: Vec<usize> = params
                .get("offsets")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_u64().map(|n| n as usize))
                        .collect()
                })
                .unwrap_or_default();
            let regs = slot.dump_registers(&custom_offsets);
            let entries: Vec<serde_json::Value> = regs
                .iter()
                .map(|(off, val)| serde_json::json!({"offset": format!("{off:#010x}"), "value": format!("{val:#010x}"), "raw_offset": off, "raw_value": val}))
                .collect();
            Ok(
                serde_json::json!({"bdf": bdf, "register_count": entries.len(), "registers": entries}),
            )
        }
        "device.register_snapshot" => {
            let bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(bdf)?;
            let slot = devices
                .iter()
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf),
                })
                .map_err(RpcError::from)?;
            let snap = slot.last_snapshot();
            let entries: Vec<serde_json::Value> = snap
                .iter()
                .map(|(off, val)| serde_json::json!({"offset": format!("{off:#010x}"), "value": format!("{val:#010x}"), "raw_offset": off, "raw_value": val}))
                .collect();
            Ok(
                serde_json::json!({"bdf": bdf, "register_count": entries.len(), "registers": entries}),
            )
        }
        "device.write_register" => {
            let bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(bdf)?;
            let offset = params
                .get("offset")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| RpcError::invalid_params("missing 'offset' parameter"))?
                as usize;
            let value = params
                .get("value")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| RpcError::invalid_params("missing 'value' parameter"))?
                as u32;
            let allow_dangerous = params
                .get("allow_dangerous")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let slot = devices
                .iter()
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf),
                })
                .map_err(RpcError::from)?;
            slot.write_register(offset, value, allow_dangerous)
                .map_err(|e| RpcError::device_error(e.to_string()))?;
            Ok(serde_json::json!({
                "bdf": bdf,
                "offset": format!("{offset:#010x}"),
                "value": format!("{value:#010x}"),
            }))
        }
        "device.read_bar0_range" => {
            let bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(bdf)?;
            let offset = params
                .get("offset")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| RpcError::invalid_params("missing 'offset' parameter"))?
                as usize;
            let count = params
                .get("count")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| RpcError::invalid_params("missing 'count' parameter"))?
                as usize;
            if count > 4096 {
                return Err(RpcError::invalid_params("count exceeds 4096 maximum"));
            }
            let slot = devices
                .iter()
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf),
                })
                .map_err(RpcError::from)?;
            let values = slot.read_bar0_range(offset, count);
            Ok(serde_json::json!({
                "bdf": bdf,
                "offset": format!("{offset:#010x}"),
                "count": values.len(),
                "values": values,
            }))
        }
        "device.pramin_read" => {
            let bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(bdf)?;
            let vram_offset = params
                .get("vram_offset")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| RpcError::invalid_params("missing 'vram_offset' parameter"))?;
            let count = params
                .get("count")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| RpcError::invalid_params("missing 'count' parameter"))?
                as usize;
            if count > 4096 {
                return Err(RpcError::invalid_params("count exceeds 4096 maximum"));
            }
            let slot = devices
                .iter()
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf),
                })
                .map_err(RpcError::from)?;
            let values = slot
                .pramin_read(vram_offset, count)
                .map_err(|e| RpcError::device_error(e.to_string()))?;
            Ok(serde_json::json!({
                "bdf": bdf,
                "vram_offset": format!("{vram_offset:#010x}"),
                "count": values.len(),
                "values": values,
            }))
        }
        "device.pramin_write" => {
            let bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(bdf)?;
            let vram_offset = params
                .get("vram_offset")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| RpcError::invalid_params("missing 'vram_offset' parameter"))?;
            let values: Vec<u32> = params
                .get("values")
                .and_then(|v| v.as_array())
                .ok_or_else(|| RpcError::invalid_params("missing 'values' array parameter"))?
                .iter()
                .filter_map(|v| v.as_u64().map(|n| n as u32))
                .collect();
            if values.len() > 4096 {
                return Err(RpcError::invalid_params("values array exceeds 4096 maximum"));
            }
            let slot = devices
                .iter()
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf),
                })
                .map_err(RpcError::from)?;
            slot.pramin_write(vram_offset, &values)
                .map_err(|e| RpcError::device_error(e.to_string()))?;
            Ok(serde_json::json!({
                "bdf": bdf,
                "vram_offset": format!("{vram_offset:#010x}"),
                "count": values.len(),
            }))
        }
        "device.lend" => {
            let raw_bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(raw_bdf)?.to_owned();
            let slot = devices
                .iter_mut()
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf.as_str()),
                })
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
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf.as_str()),
                })
                .map_err(RpcError::from)?;
            if slot.is_busy() {
                return Err(RpcError::device_error(format!(
                    "device {bdf} is busy — cannot reclaim while a long-running operation is in progress"
                )));
            }
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
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf.as_str()),
                })
                .map_err(RpcError::from)?;
            if slot.is_busy() {
                return Err(RpcError::device_error(format!(
                    "device {bdf} is busy — cannot resurrect while a long-running operation is in progress"
                )));
            }
            let alive = slot
                .resurrect_hbm2()
                .map_err(|e| RpcError::device_error(e.to_string()))?;
            Ok(serde_json::json!({
                "bdf": bdf,
                "vram_alive": alive,
                "domains_alive": slot.health.domains_alive,
            }))
        }
        "device.reset" => {
            let raw_bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(raw_bdf)?.to_owned();
            let slot = devices
                .iter()
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf.as_str()),
                })
                .map_err(RpcError::from)?;
            if slot.is_busy() {
                return Err(RpcError::device_error(format!(
                    "device {bdf} is busy — cannot reset while a long-running operation is in progress"
                )));
            }
            slot.reset_device()
                .map_err(|e| RpcError::device_error(e.to_string()))?;
            tracing::info!(bdf = %bdf, "PCIe FLR completed via VFIO_DEVICE_RESET");
            Ok(serde_json::json!({
                "bdf": bdf,
                "reset": true,
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
        "device.compute_info" | "device.quota" => {
            Err(RpcError::internal("routed to async handler"))
        }
        "device.set_quota" => {
            Err(RpcError::internal("routed to async handler"))
        }
        "daemon.shutdown" => {
            tracing::info!("shutdown requested via JSON-RPC");
            Err(RpcError::device_error("shutdown"))
        }
        other => Err(RpcError::method_not_found(other)),
    }
}

/// Query GPU compute info via nvidia-smi, releasing the device lock first.
async fn compute_info_async(
    params: &serde_json::Value,
    devices: &Mutex<Vec<coral_glowplug::device::DeviceSlot>>,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();

    let (chip, personality, role, protected) = {
        let devs = devices.lock().await;
        let slot = devs
            .iter()
            .find(|d| d.bdf.as_ref() == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::from(bdf.as_str()),
            })
            .map_err(RpcError::from)?;
        (
            slot.chip_name.clone(),
            slot.personality.to_string(),
            slot.config.role.clone(),
            slot.config.is_protected(),
        )
    };

    let bdf2 = bdf.clone();
    let info = tokio::task::spawn_blocking(move || query_nvidia_smi(&bdf2))
        .await
        .map_err(|e| RpcError::internal(format!("nvidia-smi task panicked: {e}")))?;

    let render_node = coral_glowplug::sysfs::find_render_node(&bdf);
    Ok(serde_json::json!({
        "bdf": bdf,
        "chip": chip,
        "personality": personality,
        "role": role,
        "protected": protected,
        "render_node": render_node,
        "compute": info,
    }))
}

/// Query GPU quota info via nvidia-smi, releasing the device lock first.
async fn quota_info_async(
    params: &serde_json::Value,
    devices: &Mutex<Vec<coral_glowplug::device::DeviceSlot>>,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();

    let (role, protected, quota) = {
        let devs = devices.lock().await;
        let slot = devs
            .iter()
            .find(|d| d.bdf.as_ref() == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::from(bdf.as_str()),
            })
            .map_err(RpcError::from)?;
        (
            slot.config.role.clone(),
            slot.config.is_protected(),
            slot.config.shared.as_ref().cloned().unwrap_or_default(),
        )
    };

    let bdf2 = bdf.clone();
    let current = tokio::task::spawn_blocking(move || query_nvidia_smi(&bdf2))
        .await
        .map_err(|e| RpcError::internal(format!("nvidia-smi task panicked: {e}")))?;

    Ok(serde_json::json!({
        "bdf": bdf,
        "role": role,
        "protected": protected,
        "quota": {
            "power_limit_w": quota.power_limit_w,
            "vram_budget_mib": quota.vram_budget_mib,
            "compute_mode": quota.compute_mode,
            "compute_priority": quota.compute_priority,
        },
        "current": current,
    }))
}

/// Set GPU quota and apply via nvidia-smi, releasing the device lock for the
/// blocking nvidia-smi call.
async fn set_quota_async(
    params: &serde_json::Value,
    devices: &Mutex<Vec<coral_glowplug::device::DeviceSlot>>,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();

    let quota = {
        let mut devs = devices.lock().await;
        let slot = devs
            .iter_mut()
            .find(|d| d.bdf.as_ref() == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::from(bdf.as_str()),
            })
            .map_err(RpcError::from)?;

        if !slot.config.is_shared() && !slot.config.is_display() {
            return Err(RpcError::device_error(
                "set_quota only applies to role=shared or role=display devices",
            ));
        }

        let mut quota = slot.config.shared.clone().unwrap_or_default();
        if let Some(pl) = params.get("power_limit_w").and_then(|v| v.as_u64()) {
            quota.power_limit_w = Some(pl as u32);
        }
        if let Some(vb) = params.get("vram_budget_mib").and_then(|v| v.as_u64()) {
            quota.vram_budget_mib = Some(vb as u32);
        }
        if let Some(cm) = params.get("compute_mode").and_then(|v| v.as_str()) {
            quota.compute_mode = cm.to_string();
        }
        if let Some(cp) = params.get("compute_priority").and_then(|v| v.as_u64()) {
            quota.compute_priority = cp as u32;
        }

        slot.config.shared = Some(quota.clone());
        quota
    };

    let bdf2 = bdf.clone();
    let quota2 = quota.clone();
    let results = tokio::task::spawn_blocking(move || apply_quota(&bdf2, &quota2))
        .await
        .map_err(|e| RpcError::internal(format!("nvidia-smi task panicked: {e}")))?;

    Ok(serde_json::json!({
        "bdf": bdf,
        "quota": {
            "power_limit_w": quota.power_limit_w,
            "vram_budget_mib": quota.vram_budget_mib,
            "compute_mode": quota.compute_mode,
            "compute_priority": quota.compute_priority,
        },
        "applied": results,
    }))
}

/// Apply quota settings to a GPU via nvidia-smi.
fn apply_quota(bdf: &str, quota: &coral_glowplug::config::SharedQuota) -> serde_json::Value {
    let pci_bus_id = bdf.trim_start_matches("0000:");
    let mut results = serde_json::Map::new();

    if let Some(pl) = quota.power_limit_w {
        let out = std::process::Command::new("nvidia-smi")
            .args(["-i", pci_bus_id, &format!("--power-limit={pl}")])
            .output();
        let ok = out.as_ref().is_ok_and(|o| o.status.success());
        let msg = out
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|e| e.to_string());
        results.insert(
            "power_limit".into(),
            serde_json::json!({"ok": ok, "message": msg}),
        );
    }

    match quota.compute_mode.as_str() {
        "default" | "exclusive_process" | "prohibited" => {
            let mode_id = match quota.compute_mode.as_str() {
                "default" => "0",
                "exclusive_process" => "3",
                "prohibited" => "2",
                _ => "0",
            };
            let out = std::process::Command::new("nvidia-smi")
                .args(["-i", pci_bus_id, &format!("--compute-mode={mode_id}")])
                .output();
            let ok = out.as_ref().is_ok_and(|o| o.status.success());
            let msg = out
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|e| e.to_string());
            results.insert(
                "compute_mode".into(),
                serde_json::json!({"ok": ok, "message": msg}),
            );
        }
        _ => {
            results.insert(
                "compute_mode".into(),
                serde_json::json!({"ok": false, "message": "unknown mode"}),
            );
        }
    }

    serde_json::Value::Object(results)
}

/// Query nvidia-smi for GPU compute info. Returns a JSON object with memory, clocks, power, temp.
/// Returns null fields if nvidia-smi is unavailable or the BDF doesn't match a managed GPU.
fn query_nvidia_smi(bdf: &str) -> serde_json::Value {
    let pci_bus_id = bdf.trim_start_matches("0000:");
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=gpu_name,memory.total,memory.free,memory.used,temperature.gpu,power.draw,power.limit,clocks.current.sm,clocks.current.memory,compute_cap,pcie.link.width.current",
            "--format=csv,noheader,nounits",
            &format!("--id={pci_bus_id}"),
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let fields: Vec<&str> = text.trim().splitn(11, ", ").collect();
            if fields.len() >= 11 {
                serde_json::json!({
                    "gpu_name": fields[0],
                    "memory_total_mib": fields[1].trim().parse::<f64>().unwrap_or(0.0),
                    "memory_free_mib": fields[2].trim().parse::<f64>().unwrap_or(0.0),
                    "memory_used_mib": fields[3].trim().parse::<f64>().unwrap_or(0.0),
                    "temperature_c": fields[4].trim().parse::<u32>().unwrap_or(0),
                    "power_draw_w": fields[5].trim().parse::<f64>().unwrap_or(0.0),
                    "power_limit_w": fields[6].trim().parse::<f64>().unwrap_or(0.0),
                    "clock_sm_mhz": fields[7].trim().parse::<u32>().unwrap_or(0),
                    "clock_mem_mhz": fields[8].trim().parse::<u32>().unwrap_or(0),
                    "compute_cap": fields[9].trim(),
                    "pcie_width": fields[10].trim().parse::<u32>().unwrap_or(0),
                })
            } else {
                serde_json::json!({"error": "unexpected nvidia-smi output format"})
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            serde_json::json!({"error": format!("nvidia-smi failed: {}", stderr.trim())})
        }
        Err(e) => serde_json::json!({"error": format!("nvidia-smi not available: {e}")}),
    }
}

fn device_to_info(d: &coral_glowplug::device::DeviceSlot) -> DeviceInfo {
    DeviceInfo {
        bdf: d.bdf.to_string(),
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
        protected: d.config.is_protected(),
    }
}

async fn handle_client(
    stream: ClientStream,
    devices: Arc<Mutex<Vec<coral_glowplug::device::DeviceSlot>>>,
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
    devices: Arc<Mutex<Vec<coral_glowplug::device::DeviceSlot>>>,
    started_at: std::time::Instant,
) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::with_capacity(INITIAL_BUF_CAPACITY, reader).lines();

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
                        Err(coral_glowplug::error::RpcError {
                            code: coral_glowplug::error::RpcErrorCode::INVALID_REQUEST,
                            message: format!("invalid jsonrpc version: {}", req.jsonrpc),
                        }),
                    )
                } else if req.method == "device.oracle_capture" {
                    let result = oracle_capture_async(&req.params, &devices).await;
                    make_response(req.id, result)
                } else if req.method == "device.dispatch" {
                    let result = compute_dispatch_async(&req.params, &devices).await;
                    make_response(req.id, result)
                } else if req.method == "device.compute_info" {
                    let result = compute_info_async(&req.params, &devices).await;
                    make_response(req.id, result)
                } else if req.method == "device.quota" {
                    let result = quota_info_async(&req.params, &devices).await;
                    make_response(req.id, result)
                } else if req.method == "device.set_quota" {
                    let result = set_quota_async(&req.params, &devices).await;
                    make_response(req.id, result)
                } else if matches!(
                    req.method.as_str(),
                    "device.swap"
                        | "device.resurrect"
                        | "device.reset"
                        | "device.lend"
                        | "device.reclaim"
                        | "device.write_register"
                        | "device.pramin_write"
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
                Err(coral_glowplug::error::RpcError {
                    code: coral_glowplug::error::RpcErrorCode::PARSE_ERROR,
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
#[path = "socket_tests/mod.rs"]
mod socket_tests;
