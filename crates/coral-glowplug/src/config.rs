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

#[derive(Debug, Deserialize)]
pub struct DaemonConfig {
    #[serde(default = "default_socket")]
    pub socket: String,
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    #[serde(default)]
    pub role: Option<String>,
    #[allow(dead_code)]
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

    #[allow(dead_code)]
    pub fn default_with_bdf(bdf: &str) -> Self {
        Self {
            daemon: DaemonConfig::default(),
            device: vec![DeviceConfig {
                bdf: bdf.to_string(),
                name: None,
                boot_personality: "vfio".into(),
                power_policy: "always_on".into(),
                role: Some("compute".into()),
                oracle_dump: None,
            }],
        }
    }
}
