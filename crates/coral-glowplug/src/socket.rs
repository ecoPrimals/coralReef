// SPDX-License-Identifier: AGPL-3.0-only
//! Unix socket server with SCM_RIGHTS fd passing.
//!
//! toadStool (and other consumers) connect to this socket to:
//! - List available devices and their capabilities
//! - Receive VFIO container fds via SCM_RIGHTS
//! - Request driver personality swaps
//! - Query device health

use serde::{Deserialize, Serialize};
use tokio::net::UnixListener;
use std::sync::Arc;
use tokio::sync::Mutex;


#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    ListDevices,
    GetDevice { bdf: String },
    Swap { bdf: String, target: String },
    Health { bdf: String },
    Resurrect { bdf: String },
    Status,
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Devices(Vec<DeviceInfo>),
    DeviceReady { bdf: String, personality: String, vram_alive: bool },
    SwapComplete { bdf: String, personality: String, vram_alive: bool },
    Resurrected { bdf: String, vram_alive: bool, domains_alive: usize },
    Health(HealthInfo),
    Status(DaemonStatus),
    Error(String),
    Ok,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub uptime_secs: u64,
    pub device_count: usize,
    pub healthy_count: usize,
}

pub struct SocketServer {
    listener: UnixListener,
    pub started_at: std::time::Instant,
}

impl SocketServer {
    pub async fn bind(path: &str) -> Result<Self, String> {
        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Remove stale socket
        let _ = std::fs::remove_file(path);

        let listener = UnixListener::bind(path)
            .map_err(|e| format!("bind {path}: {e}"))?;

        // Set permissions so non-root toadStool can connect
        let _ = std::fs::set_permissions(
            path,
            std::os::unix::fs::PermissionsExt::from_mode(0o666),
        );

        tracing::info!(path, "socket server listening");
        Ok(Self {
            listener,
            started_at: std::time::Instant::now(),
        })
    }

    pub async fn accept_loop(
        &self,
        devices: Arc<Mutex<Vec<crate::device::DeviceSlot>>>,
    ) {
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

async fn handle_client(
    stream: tokio::net::UnixStream,
    devices: Arc<Mutex<Vec<crate::device::DeviceSlot>>>,
    started_at: std::time::Instant,
) -> Result<(), String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let request: Request = serde_json::from_str(&line)
            .map_err(|e| format!("parse request: {e}"))?;

        let response = match request {
            Request::ListDevices => {
                let devs = devices.lock().await;
                let infos: Vec<DeviceInfo> = devs.iter().map(|d| DeviceInfo {
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
                }).collect();
                Response::Devices(infos)
            }

            Request::Swap { bdf, target } => {
                // Swap involves blocking sysfs writes + driver bind (can take 30s+)
                // so we run it on a blocking thread to avoid stalling the runtime.
                let devs_clone = devices.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    let mut devs = rt.block_on(devs_clone.lock());
                    if let Some(slot) = devs.iter_mut().find(|d| d.bdf == bdf) {
                        match slot.swap(&target) {
                            Ok(()) => Response::SwapComplete {
                                bdf,
                                personality: slot.personality.to_string(),
                                vram_alive: slot.health.vram_alive,
                            },
                            Err(e) => Response::Error(e),
                        }
                    } else {
                        Response::Error(format!("device {bdf} not managed"))
                    }
                }).await.unwrap_or_else(|e| Response::Error(format!("spawn_blocking: {e}")));
                result
            }

            Request::Resurrect { bdf } => {
                let devs_clone = devices.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    let mut devs = rt.block_on(devs_clone.lock());
                    if let Some(slot) = devs.iter_mut().find(|d| d.bdf == bdf) {
                        match slot.resurrect_hbm2() {
                            Ok(alive) => Response::Resurrected {
                                bdf,
                                vram_alive: alive,
                                domains_alive: slot.health.domains_alive,
                            },
                            Err(e) => Response::Error(e),
                        }
                    } else {
                        Response::Error(format!("device {bdf} not managed"))
                    }
                }).await.unwrap_or_else(|e| Response::Error(format!("spawn_blocking: {e}")));
                result
            }

            Request::Health { bdf } => {
                let mut devs = devices.lock().await;
                if let Some(slot) = devs.iter_mut().find(|d| d.bdf == bdf) {
                    slot.check_health();
                    Response::Health(HealthInfo {
                        bdf,
                        boot0: slot.health.boot0,
                        pmc_enable: slot.health.pmc_enable,
                        vram_alive: slot.health.vram_alive,
                        power: slot.health.power.to_string(),
                        domains_alive: slot.health.domains_alive,
                        domains_faulted: slot.health.domains_faulted,
                    })
                } else {
                    Response::Error(format!("device {bdf} not managed"))
                }
            }

            Request::Status => {
                let devs = devices.lock().await;
                let healthy = devs.iter().filter(|d| d.health.vram_alive).count();
                Response::Status(DaemonStatus {
                    uptime_secs: started_at.elapsed().as_secs(),
                    device_count: devs.len(),
                    healthy_count: healthy,
                })
            }

            Request::GetDevice { bdf } => {
                let devs = devices.lock().await;
                if let Some(slot) = devs.iter().find(|d| d.bdf == bdf) {
                    Response::DeviceReady {
                        bdf,
                        personality: slot.personality.to_string(),
                        vram_alive: slot.health.vram_alive,
                    }
                } else {
                    Response::Error(format!("device {bdf} not managed"))
                }
            }

            Request::Shutdown => {
                tracing::info!("shutdown requested via socket");
                return Ok(());
            }
        };

        let json = serde_json::to_string(&response)
            .map_err(|e| format!("serialize response: {e}"))?;
        writer.write_all(json.as_bytes()).await
            .map_err(|e| format!("write: {e}"))?;
        writer.write_all(b"\n").await
            .map_err(|e| format!("write newline: {e}"))?;
    }

    Ok(())
}
