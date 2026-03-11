// SPDX-License-Identifier: AGPL-3.0-only
//! Capability-based device discovery via the ecoPrimals ecosystem.
//!
//! Follows the ecoPrimals **Node Atomic** pattern: coralReef discovers GPU
//! hardware through capability-based IPC rather than scanning `/dev/dri/`
//! directly. When no ecosystem provider is available (standalone mode),
//! falls back to direct DRM render node enumeration.
//!
//! ## Discovery flow
//!
//! ```text
//! coralReef → discovery_dir/*.json → find "gpu.dispatch" capability
//!         → provider endpoint → gpu.info / gpu.enumerate
//!         → GpuDeviceDescriptor { vendor, arch, render_node_path }
//!
//!         (fallback if no ecosystem provider)
//!         → DRM render node scan → DrmDeviceInfo { driver, path }
//! ```
//!
//! No primal names are hardcoded. coralReef only knows it needs
//! `"gpu.dispatch"` — whoever provides it is discovered at runtime.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Vendor-agnostic GPU device descriptor.
///
/// Can be populated from either ecosystem discovery or direct DRM scan.
/// Contains enough metadata for coralReef to select the correct
/// compilation target and open the correct render node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuDeviceDescriptor {
    /// GPU vendor (`"nvidia"`, `"amd"`, `"intel"`).
    pub vendor: String,
    /// Architecture identifier (`"sm86"`, `"rdna2"`, etc.).
    pub arch: Option<String>,
    /// DRM render node path (e.g. `/dev/dri/renderD128`).
    pub render_node: Option<String>,
    /// DRM driver name (e.g. `"amdgpu"`, `"nvidia-drm"`).
    pub driver: Option<String>,
    /// Device memory in bytes (from ecosystem discovery, if available).
    pub memory_bytes: Option<u64>,
    /// Discovery source: `"ecosystem"` or `"drm-scan"`.
    pub source: String,
}

/// A discovered provider with GPU capabilities.
///
/// Supports both legacy format (`capabilities`) and Phase 10 (`provides`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscoveryEntry {
    /// Legacy: capability list.
    #[serde(default)]
    capabilities: Vec<String>,
    /// Phase 10: what this primal provides (preferred over capabilities).
    #[serde(default)]
    provides: Vec<String>,
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(default)]
    devices: Vec<DiscoveryDevice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscoveryDevice {
    #[serde(default)]
    vendor: String,
    #[serde(default)]
    arch: Option<String>,
    #[serde(default)]
    render_node: Option<String>,
    #[serde(default)]
    driver: Option<String>,
    #[serde(default)]
    memory_bytes: Option<u64>,
}

/// Discover GPU devices through the ecoPrimals ecosystem.
///
/// 1. Checks the shared discovery directory for capability files
///    containing `"gpu.dispatch"` or `"gpu-*"` capabilities.
/// 2. Falls back to direct DRM render node enumeration if no
///    ecosystem provider is found.
///
/// This function never panics — discovery failures are logged and
/// result in an empty or DRM-only device list.
#[must_use]
pub fn discover_gpu_devices() -> Vec<GpuDeviceDescriptor> {
    let mut devices = Vec::new();

    if let Ok(dir) = crate::config::discovery_dir() {
        if let Some(ecosystem_devices) = discover_from_ecosystem(&dir) {
            devices.extend(ecosystem_devices);
        }
    }

    if devices.is_empty() {
        devices.extend(discover_from_drm());
    }

    devices
}

/// Discover GPU devices from the ecoPrimals capability directory.
///
/// Scans `$DISCOVERY_DIR/*.json` for entries advertising GPU capabilities.
fn discover_from_ecosystem(discovery_dir: &Path) -> Option<Vec<GpuDeviceDescriptor>> {
    let entries = std::fs::read_dir(discovery_dir).ok()?;
    let mut devices = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                if let Ok(discovery) = serde_json::from_str::<DiscoveryEntry>(&contents) {
                    let caps = if discovery.provides.is_empty() {
                        &discovery.capabilities
                    } else {
                        &discovery.provides
                    };
                    let has_gpu_cap = caps.iter().any(|c| {
                        c == "gpu.dispatch" || c.starts_with("gpu-") || c == "science.gpu.dispatch"
                    });

                    if has_gpu_cap {
                        for dev in &discovery.devices {
                            devices.push(GpuDeviceDescriptor {
                                vendor: dev.vendor.clone(),
                                arch: dev.arch.clone(),
                                render_node: dev.render_node.clone(),
                                driver: dev.driver.clone(),
                                memory_bytes: dev.memory_bytes,
                                source: "ecosystem".to_string(),
                            });
                        }

                        if discovery.devices.is_empty() {
                            tracing::debug!(
                                path = %path.display(),
                                "ecosystem provider found with GPU capability but no device list"
                            );
                        }
                    }
                }
            }
        }
    }

    if devices.is_empty() {
        None
    } else {
        Some(devices)
    }
}

/// Discover GPU devices by scanning DRM render nodes directly.
///
/// Fallback path when no ecosystem provider is available (standalone mode).
/// For NVIDIA devices, probes sysfs to determine the actual SM architecture
/// instead of guessing from driver name.
#[cfg(target_os = "linux")]
fn discover_from_drm() -> Vec<GpuDeviceDescriptor> {
    use coral_driver::drm::enumerate_render_nodes;
    use coral_driver::nv::identity::probe_gpu_identity;

    enumerate_render_nodes()
        .into_iter()
        .map(|info| {
            let vendor = match info.driver.as_str() {
                "amdgpu" => "amd",
                "nvidia-drm" | "nouveau" => "nvidia",
                "i915" | "xe" => "intel",
                _ => "unknown",
            };

            let arch = match info.driver.as_str() {
                "amdgpu" => Some("rdna2".to_string()),
                "nvidia-drm" | "nouveau" => probe_gpu_identity(&info.path)
                    .and_then(|id| id.nvidia_sm())
                    .map(|sm| format!("sm{sm}")),
                _ => None,
            };

            GpuDeviceDescriptor {
                vendor: vendor.to_string(),
                arch,
                render_node: Some(info.path),
                driver: Some(info.driver),
                memory_bytes: None,
                source: "drm-scan".to_string(),
            }
        })
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn discover_from_drm() -> Vec<GpuDeviceDescriptor> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn gpu_device_descriptor_debug() {
        let desc = GpuDeviceDescriptor {
            vendor: "amd".to_string(),
            arch: Some("rdna2".to_string()),
            render_node: Some("/dev/dri/renderD128".to_string()),
            driver: Some("amdgpu".to_string()),
            memory_bytes: Some(16 * 1024 * 1024 * 1024),
            source: "drm-scan".to_string(),
        };
        let debug = format!("{desc:?}");
        assert!(debug.contains("amd"));
        assert!(debug.contains("rdna2"));
    }

    #[test]
    fn gpu_device_descriptor_serialization() {
        let desc = GpuDeviceDescriptor {
            vendor: "nvidia".to_string(),
            arch: Some("sm86".to_string()),
            render_node: Some("/dev/dri/renderD129".to_string()),
            driver: Some("nvidia-drm".to_string()),
            memory_bytes: Some(24 * 1024 * 1024 * 1024),
            source: "ecosystem".to_string(),
        };
        let json = serde_json::to_string(&desc).unwrap();
        assert!(json.contains("nvidia"));
        assert!(json.contains("ecosystem"));

        let roundtrip: GpuDeviceDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.vendor, "nvidia");
        assert_eq!(roundtrip.source, "ecosystem");
    }

    #[test]
    fn discover_from_ecosystem_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = discover_from_ecosystem(dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn discover_from_ecosystem_with_gpu_capability() {
        let dir = tempfile::tempdir().unwrap();
        let entry = serde_json::json!({
            "capabilities": ["gpu.dispatch", "science.gpu.dispatch"],
            "endpoint": "unix:///run/user/1000/ecoPrimals/gpu-provider.sock",
            "devices": [
                {
                    "vendor": "amd",
                    "arch": "rdna2",
                    "render_node": "/dev/dri/renderD128",
                    "driver": "amdgpu",
                    "memory_bytes": 17_179_869_184_u64
                },
                {
                    "vendor": "nvidia",
                    "arch": "sm86",
                    "render_node": "/dev/dri/renderD129",
                    "driver": "nvidia-drm",
                    "memory_bytes": 25_769_803_776_u64
                }
            ]
        });
        let path = dir.path().join("gpu-provider.json");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(f, "{entry}").unwrap();

        let result = discover_from_ecosystem(dir.path());
        assert!(result.is_some());
        let devices = result.unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].vendor, "amd");
        assert_eq!(devices[0].source, "ecosystem");
        assert_eq!(devices[1].vendor, "nvidia");
        assert_eq!(devices[1].arch.as_deref(), Some("sm86"));
    }

    #[test]
    fn discover_from_ecosystem_phase10_provides() {
        let dir = tempfile::tempdir().unwrap();
        let entry = serde_json::json!({
            "version": "1.0.0",
            "pid": 12345,
            "provides": ["gpu.dispatch"],
            "transports": {
                "jsonrpc": { "bind": "unix:///run/user/1000/ecoPrimals/gpu-provider.sock" },
                "tarpc": { "bind": "unix:///run/user/1000/ecoPrimals/gpu-provider-tarpc.sock" }
            },
            "devices": [
                {
                    "vendor": "amd",
                    "arch": "rdna2",
                    "render_node": "/dev/dri/renderD128",
                    "driver": "amdgpu"
                }
            ]
        });
        let path = dir.path().join("gpu-provider.json");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(f, "{entry}").unwrap();

        let result = discover_from_ecosystem(dir.path());
        assert!(result.is_some());
        let devices = result.unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].vendor, "amd");
        assert_eq!(devices[0].arch.as_deref(), Some("rdna2"));
    }

    #[test]
    fn discover_from_ecosystem_ignores_non_gpu_files() {
        let dir = tempfile::tempdir().unwrap();
        let entry = serde_json::json!({
            "capabilities": ["storage.read", "storage.write"],
            "endpoint": "unix:///run/user/1000/ecoPrimals/storage-provider.sock",
            "devices": []
        });
        let path = dir.path().join("storage-provider.json");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(f, "{entry}").unwrap();

        let result = discover_from_ecosystem(dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn discover_from_ecosystem_handles_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.json");
        std::fs::write(&path, "not valid json {{{").unwrap();

        let result = discover_from_ecosystem(dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn discover_gpu_devices_returns_something() {
        let devices = discover_gpu_devices();
        // May be empty without GPUs, but should not panic.
        for dev in &devices {
            assert!(!dev.vendor.is_empty());
            assert!(!dev.source.is_empty());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn discover_from_drm_returns_known_drivers() {
        let devices = discover_from_drm();
        for dev in &devices {
            assert!(
                ["amd", "nvidia", "intel", "unknown"].contains(&dev.vendor.as_str()),
                "unexpected vendor: {}",
                dev.vendor
            );
            assert_eq!(dev.source, "drm-scan");
        }
    }
}
