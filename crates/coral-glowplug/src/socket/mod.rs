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
//! | `device.experiment_start` | Start an experiment session — pause health probes, set watchdog |
//! | `device.experiment_end` | End an experiment session — resume health probes |
//! | `device.oracle_capture` | Capture MMU page tables via daemon (no VFIO access needed) |
//! | `device.dispatch`  | Submit compute work (shader + buffers) through the daemon |
//! | `device.compute_info` | Query NVML telemetry for a GPU            |
//! | `device.quota`     | Query compute quota for shared/display GPU   |
//! | `device.set_quota` | Set compute quota (power limit, mode)        |
//! | `mailbox.create`   | Create a named mailbox on a device            |
//! | `mailbox.post`     | Post a firmware command to a mailbox          |
//! | `mailbox.poll`     | Poll a posted command's completion status      |
//! | `mailbox.complete` | Mark a command complete (test/simulation)      |
//! | `mailbox.drain`    | Drain completed mailbox entries                |
//! | `mailbox.stats`    | Mailbox statistics for a device               |
//! | `ring.create`      | Create a named ring buffer on a device        |
//! | `ring.submit`      | Submit an entry to a ring buffer              |
//! | `ring.consume`     | Consume the next pending ring entry           |
//! | `ring.fence`       | Consume entries through a fence value          |
//! | `ring.peek`        | Peek at next pending entry without consuming   |
//! | `ring.stats`       | Ring statistics for a device                  |
//! | `vault.restore_fds` | Return vaulted VFIO fds via SCM_RIGHTS (ember resurrection) |
//! | `daemon.status`    | Daemon uptime and device count              |
//! | `daemon.shutdown`  | Graceful shutdown                           |

mod handlers;
mod protocol;

#[cfg(test)]
pub(crate) use handlers::dispatch;
#[cfg(test)]
pub(crate) use handlers::validate_bdf;
#[cfg(test)]
pub(crate) use protocol::JsonRpcRequest;

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UnixListener, UnixStream};
use tokio::sync::Mutex;

use coral_glowplug::error::SocketServerError;

use protocol::{CLIENT_IDLE_TIMEOUT, INITIAL_BUF_CAPACITY, MAX_REQUEST_LINE_BYTES, make_response};

/// Maximum concurrent client connections.
const MAX_CONCURRENT_CLIENTS: usize = 64;

/// Set socket group ownership for unprivileged user access.
///
/// Resolves `group_name` from the group database file (`/etc/group` by default,
/// override with `CORALREEF_GROUP_FILE`) and chowns the socket.
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
    coral_glowplug::group_unix::gid_for_group_name(group_name)
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
    pub async fn bind(addr: &str) -> Result<Self, SocketServerError> {
        let started_at = std::time::Instant::now();

        if let Ok(socket_addr) = addr.parse::<SocketAddr>() {
            // TCP transport
            let listener = TcpListener::bind(socket_addr).await.map_err(|source| {
                SocketServerError::BindTcp {
                    addr: addr.to_string(),
                    source,
                }
            })?;
            let bound = listener
                .local_addr()
                .map_err(|source| SocketServerError::TcpLocalAddr { source })?;
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
                    UnixListener::bind(addr).map_err(|source| SocketServerError::BindUnix {
                        path: addr.to_string(),
                        source,
                    })?;

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
                Err(SocketServerError::UnixNotSupported {
                    fallback: coral_glowplug::config::FALLBACK_TCP_BIND.to_string(),
                })
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
        vault: Option<Arc<coral_glowplug::fd_vault::FdVault>>,
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
                        let vault = vault.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_client(stream, devices, started_at, vault).await {
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

async fn handle_client(
    stream: ClientStream,
    devices: Arc<Mutex<Vec<coral_glowplug::device::DeviceSlot>>>,
    started_at: std::time::Instant,
    vault: Option<Arc<coral_glowplug::fd_vault::FdVault>>,
) -> Result<(), SocketServerError> {
    match stream {
        #[cfg(unix)]
        ClientStream::Unix(s) => {
            if let Some(ref vault) = vault {
                if is_vault_restore_request(&s).await {
                    return handle_vault_restore_async(s, vault).await;
                }
            }
            handle_client_stream(s, devices, started_at).await
        }
        ClientStream::Tcp(s) => handle_client_stream(s, devices, started_at).await,
    }
}

/// Peek at the first bytes of a Unix stream (MSG_PEEK) to detect `vault.restore_fds`.
///
/// Returns `true` if the first request is a vault restore. MSG_PEEK leaves the
/// data in the kernel buffer so the normal handler can still read it.
#[cfg(unix)]
async fn is_vault_restore_request(stream: &tokio::net::UnixStream) -> bool {
    if stream.readable().await.is_err() {
        return false;
    }
    let mut peek_buf = [0u8; 512];
    match rustix::net::recv(stream, &mut peek_buf, rustix::net::RecvFlags::PEEK) {
        Ok((_, n)) if n > 0 => std::str::from_utf8(&peek_buf[..n])
            .unwrap_or("")
            .contains("vault.restore_fds"),
        _ => false,
    }
}

/// Route a vault.restore_fds request to the synchronous SCM_RIGHTS handler.
///
/// Converts the tokio stream to a std stream and handles in a blocking task,
/// since SCM_RIGHTS sendmsg requires the raw fd (not the async writer).
#[cfg(unix)]
async fn handle_vault_restore_async(
    stream: tokio::net::UnixStream,
    vault: &Arc<coral_glowplug::fd_vault::FdVault>,
) -> Result<(), SocketServerError> {
    let vault = vault.clone();
    match stream.into_std() {
        Ok(std_stream) => {
            let _ = std_stream.set_nonblocking(false);
            let result = tokio::task::spawn_blocking(move || {
                coral_glowplug::fd_vault::handle_vault_restore(&std_stream, &vault)
            })
            .await;
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::warn!(error = %e, "vault restore handler error"),
                Err(e) => tracing::warn!(error = %e, "vault restore task panicked"),
            }
        }
        Err(e) => tracing::warn!(error = %e, "vault: failed to convert stream to std"),
    }
    Ok(())
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
) -> Result<(), SocketServerError>
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

        let resp = match serde_json::from_str::<protocol::JsonRpcRequest>(line) {
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
                    let result = handlers::oracle_capture_async(&req.params, &devices).await;
                    make_response(req.id, result)
                } else if req.method == "device.dispatch" {
                    let result = handlers::compute_dispatch_async(&req.params, &devices).await;
                    make_response(req.id, result)
                } else if req.method == "device.compute_info" {
                    let result = handlers::compute_info_async(&req.params, &devices).await;
                    make_response(req.id, result)
                } else if req.method == "device.quota" {
                    let result = handlers::quota_info_async(&req.params, &devices).await;
                    make_response(req.id, result)
                } else if req.method == "device.set_quota" {
                    let result = handlers::set_quota_async(&req.params, &devices).await;
                    make_response(req.id, result)
                } else if req.method.starts_with("mailbox.") || req.method.starts_with("ring.") {
                    let result = {
                        let mut devs = devices.lock().await;
                        handlers::mailbox_ring::dispatch(&req.method, &req.params, &mut devs)
                    };
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
                        handlers::dispatch(&req.method, &req.params, &mut devs, started_at)
                    };
                    make_response(req.id, result)
                } else {
                    let result = {
                        let mut devs = devices.lock().await;
                        handlers::dispatch(&req.method, &req.params, &mut devs, started_at)
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
#[path = "../socket_tests/mod.rs"]
mod socket_tests;
