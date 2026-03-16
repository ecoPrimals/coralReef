// SPDX-License-Identifier: AGPL-3.0-only
//! TOML configuration for the GlowPlug daemon.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub device: Vec<DeviceConfig>,
}

/// GPU vendor IDs worth managing.
const GPU_VENDORS: &[(u16, &str)] = &[
    (0x10de, "nvidia"),
    (0x1002, "amd"),
];

#[derive(Debug, Deserialize)]
pub struct DaemonConfig {
    #[serde(default = "default_socket")]
    pub socket: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_health_interval")]
    pub health_interval_ms: u64,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            socket: default_socket(),
            log_level: default_log_level(),
            health_interval_ms: default_health_interval(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceConfig {
    pub bdf: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_personality")]
    pub boot_personality: String,
    #[serde(default = "default_power_policy")]
    pub power_policy: String,
    #[serde(default)]
    pub role: Option<String>,
    /// Path to write oracle register dumps (state vault persistence).
    /// Loaded from TOML config, used by device.rs snapshot_registers.
    #[serde(default)]
    pub oracle_dump: Option<String>,
}

fn default_socket() -> String {
    // Use XDG_RUNTIME_DIR when available (user-owned, no root needed)
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        return format!("{xdg}/coralreef/glowplug.sock");
    }
    "/run/coralreef/glowplug.sock".into()
}
fn default_log_level() -> String {
    "info".into()
}
fn default_health_interval() -> u64 {
    5000
}
fn default_personality() -> String {
    "vfio".into()
}
fn default_power_policy() -> String {
    "always_on".into()
}

impl Config {
    pub fn load(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("read config {path}: {e}"))?;
        toml::from_str(&content)
            .map_err(|e| format!("parse config {path}: {e}"))
    }

    /// Build a config by scanning the PCI bus for discrete GPUs.
    ///
    /// Skips any device currently bound to proprietary `nvidia` driver
    /// (assumed to be the display card). Assigns `vfio` to everything else.
    pub fn auto_discover() -> Self {
        let mut devices = Vec::new();

        let Ok(entries) = std::fs::read_dir("/sys/bus/pci/devices") else {
            return Self { daemon: DaemonConfig::default(), device: devices };
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let bdf = entry.file_name().to_string_lossy().to_string();

            let vendor = read_sysfs_hex(&path.join("vendor"));
            let device_id = read_sysfs_hex(&path.join("device"));
            let class = read_sysfs_hex(&path.join("class"));

            // Only VGA controllers (0x0300xx) and 3D controllers (0x0302xx)
            let class_top = (class >> 8) & 0xFFFF;
            if class_top != 0x0300 && class_top != 0x0302 {
                continue;
            }

            // Only known GPU vendors
            let vendor_name = GPU_VENDORS.iter()
                .find(|(vid, _)| *vid == vendor as u16)
                .map(|(_, name)| *name);
            if vendor_name.is_none() {
                continue;
            }

            // Check current driver
            let driver = std::fs::read_link(path.join("driver"))
                .ok()
                .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()));

            // Skip display card (nvidia proprietary driver)
            if driver.as_deref() == Some("nvidia") {
                tracing::info!(bdf = %bdf, "skipping display card (nvidia proprietary)");
                continue;
            }

            let personality = if driver.as_deref() == Some("nouveau") || driver.as_deref() == Some("amdgpu") {
                driver.as_deref().unwrap_or("vfio").to_string()
            } else {
                "vfio".into()
            };

            tracing::info!(
                bdf = %bdf,
                vendor = vendor_name.unwrap_or("?"),
                device = format_args!("{device_id:#06x}"),
                driver = driver.as_deref().unwrap_or("none"),
                personality = %personality,
                "discovered GPU"
            );

            devices.push(DeviceConfig {
                bdf,
                name: None,
                boot_personality: personality,
                power_policy: "always_on".into(),
                role: Some("compute".into()),
                oracle_dump: None,
            });
        }

        Self { daemon: DaemonConfig::default(), device: devices }
    }
}

fn read_sysfs_hex(path: &std::path::Path) -> u64 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| u64::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
        .unwrap_or(0)
}
