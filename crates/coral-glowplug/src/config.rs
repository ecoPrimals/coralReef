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
const GPU_VENDORS: &[(u16, &str)] = &[(0x10de, "nvidia"), (0x1002, "amd")];

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

/// TCP loopback with OS-assigned port (ecoBin fallback on non-Unix platforms).
#[cfg_attr(unix, allow(dead_code))]
pub const FALLBACK_TCP_BIND: &str = "127.0.0.1:0";

/// Default TCP bind address for ecoBin compliance.
///
/// Returns `127.0.0.1:0` (localhost, OS-assigned port). Callers may override
/// via `$CORALREEF_TCP_BIND` for deployment configuration.
#[must_use]
#[cfg_attr(unix, allow(dead_code))]
pub fn default_tcp_fallback() -> String {
    std::env::var("CORALREEF_TCP_BIND").unwrap_or_else(|_| FALLBACK_TCP_BIND.to_owned())
}

/// Platform-aware default socket address (ecoBin compliance).
///
/// On Unix: primary transport is Unix domain socket under `$XDG_RUNTIME_DIR`
/// (or `/run/coralreef/` as fallback).
/// On non-Unix: TCP fallback to `127.0.0.1:0` (OS-assigned port).
#[must_use]
fn default_socket() -> String {
    #[cfg(unix)]
    {
        if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
            return format!("{xdg}/coralreef/glowplug.sock");
        }
        "/run/coralreef/glowplug.sock".into()
    }
    #[cfg(not(unix))]
    {
        default_tcp_fallback()
    }
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
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("read config {path}: {e}"))?;
        toml::from_str(&content).map_err(|e| format!("parse config {path}: {e}"))
    }

    /// Build a config by scanning the PCI bus for discrete GPUs.
    ///
    /// Skips any device currently bound to proprietary `nvidia` driver
    /// (assumed to be the display card). Assigns `vfio` to everything else.
    pub fn auto_discover() -> Self {
        let mut devices = Vec::new();

        let Ok(entries) = std::fs::read_dir("/sys/bus/pci/devices") else {
            return Self {
                daemon: DaemonConfig::default(),
                device: devices,
            };
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
            let vendor_name = GPU_VENDORS
                .iter()
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

            let personality =
                if driver.as_deref() == Some("nouveau") || driver.as_deref() == Some("amdgpu") {
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

        Self {
            daemon: DaemonConfig::default(),
            device: devices,
        }
    }
}

fn read_sysfs_hex(path: &std::path::Path) -> u64 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| u64::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp_config(content: &str, suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "glowplug_test_{}_{}.toml",
            std::process::id(),
            suffix
        ));
        let _ = std::fs::write(&path, content);
        path
    }

    #[test]
    fn test_load_valid_minimal() {
        let path = write_temp_config(
            r#"
[daemon]
socket = "/tmp/test.sock"
log_level = "debug"
health_interval_ms = 1000

[[device]]
bdf = "0000:01:00.0"
"#,
            "minimal",
        );
        let path_str = path.to_str().expect("path has str");
        let result = Config::load(path_str);
        let _ = std::fs::remove_file(&path);
        let config = match result {
            Ok(c) => c,
            Err(e) => panic!("valid config should load: {e}"),
        };
        assert_eq!(config.daemon.socket, "/tmp/test.sock");
        assert_eq!(config.daemon.log_level, "debug");
        assert_eq!(config.daemon.health_interval_ms, 1000);
        assert_eq!(config.device.len(), 1);
        assert_eq!(config.device[0].bdf, "0000:01:00.0");
        assert_eq!(config.device[0].boot_personality, "vfio");
        assert_eq!(config.device[0].power_policy, "always_on");
        assert!(config.device[0].name.is_none());
        assert!(config.device[0].role.is_none());
    }

    #[test]
    fn test_load_valid_full_device() {
        let path = write_temp_config(
            r#"
[[device]]
bdf = "0000:02:00.0"
name = "Compute GPU"
boot_personality = "nouveau"
power_policy = "power_save"
role = "compute"
oracle_dump = "/var/lib/glowplug/state.txt"
"#,
            "full_device",
        );
        let path_str = path.to_str().expect("path has str");
        let result = Config::load(path_str);
        let _ = std::fs::remove_file(&path);
        let config = match result {
            Ok(c) => c,
            Err(e) => panic!("valid config should load: {e}"),
        };
        assert_eq!(config.device.len(), 1);
        assert_eq!(config.device[0].bdf, "0000:02:00.0");
        assert_eq!(config.device[0].name.as_deref(), Some("Compute GPU"));
        assert_eq!(config.device[0].boot_personality, "nouveau");
        assert_eq!(config.device[0].power_policy, "power_save");
        assert_eq!(config.device[0].role.as_deref(), Some("compute"));
        assert_eq!(
            config.device[0].oracle_dump.as_deref(),
            Some("/var/lib/glowplug/state.txt")
        );
    }

    #[test]
    fn test_load_empty_uses_defaults() {
        let path = write_temp_config("", "empty");
        let path_str = path.to_str().expect("path has str");
        let result = Config::load(path_str);
        let _ = std::fs::remove_file(&path);
        let config = match result {
            Ok(c) => c,
            Err(e) => panic!("empty config should parse: {e}"),
        };
        assert_eq!(config.daemon.log_level, "info");
        assert_eq!(config.daemon.health_interval_ms, 5000);
        assert!(config.device.is_empty());
    }

    #[test]
    fn test_load_device_defaults() {
        let path = write_temp_config(
            r#"
[[device]]
bdf = "0000:03:00.0"
"#,
            "device_defaults",
        );
        let path_str = path.to_str().expect("path has str");
        let result = Config::load(path_str);
        let _ = std::fs::remove_file(&path);
        let config = match result {
            Ok(c) => c,
            Err(e) => panic!("config should load: {e}"),
        };
        let dev = &config.device[0];
        assert_eq!(dev.boot_personality, "vfio");
        assert_eq!(dev.power_policy, "always_on");
        assert!(dev.name.is_none());
        assert!(dev.role.is_none());
        assert!(dev.oracle_dump.is_none());
    }

    #[test]
    fn test_load_missing_file() {
        let result = Config::load("/nonexistent/path/glowplug.toml");
        let err = match result {
            Ok(_) => panic!("expected load to fail"),
            Err(e) => e,
        };
        assert!(err.contains("read config"));
        assert!(err.contains("/nonexistent/path/glowplug.toml"));
    }

    #[test]
    fn test_load_invalid_toml() {
        let path = write_temp_config("{{{ invalid toml }}}", "invalid");
        let path_str = path.to_str().expect("path has str");
        let result = Config::load(path_str);
        let _ = std::fs::remove_file(&path);
        let err = match result {
            Ok(_) => panic!("expected parse to fail"),
            Err(e) => e,
        };
        assert!(err.contains("parse config"));
    }

    #[test]
    fn test_load_invalid_structure() {
        let path = write_temp_config(
            r#"
[[device]]
bdf = 12345
"#,
            "invalid_structure",
        );
        let result = Config::load(path.to_str().expect("path has str"));
        let _ = std::fs::remove_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_daemon_config_default() {
        let default = DaemonConfig::default();
        assert_eq!(default.log_level, "info");
        assert_eq!(default.health_interval_ms, 5000);
        #[cfg(unix)]
        assert!(default.socket.contains("glowplug.sock"));
        #[cfg(not(unix))]
        assert!(default.socket.contains("127.0.0.1"));
    }

    #[test]
    fn test_default_tcp_fallback() {
        assert_eq!(FALLBACK_TCP_BIND, "127.0.0.1:0");
        let fallback = default_tcp_fallback();
        assert!(fallback.contains("127.0.0.1"));
        assert!(fallback.contains(':'));
    }

    #[test]
    fn test_read_sysfs_hex_valid() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("glowplug_test_hex_{}.txt", std::process::id()));
        let _ = std::fs::write(&path, "0x10de");
        let val = super::read_sysfs_hex(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(val, 0x10de);
    }

    #[test]
    fn test_read_sysfs_hex_no_prefix() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("glowplug_test_hex2_{}.txt", std::process::id()));
        let _ = std::fs::write(&path, "1234");
        let val = super::read_sysfs_hex(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(val, 0x1234);
    }

    #[test]
    fn test_read_sysfs_hex_missing_returns_zero() {
        let val = super::read_sysfs_hex(std::path::Path::new("/nonexistent/path/hex"));
        assert_eq!(val, 0);
    }

    #[test]
    fn test_load_multiple_devices() {
        let path = write_temp_config(
            r#"
[[device]]
bdf = "0000:01:00.0"

[[device]]
bdf = "0000:02:00.0"
boot_personality = "amdgpu"
"#,
            "multiple_devices",
        );
        let path_str = path.to_str().expect("path has str");
        let result = Config::load(path_str);
        let _ = std::fs::remove_file(&path);
        let config = match result {
            Ok(c) => c,
            Err(e) => panic!("valid config should load: {e}"),
        };
        assert_eq!(config.device.len(), 2);
        assert_eq!(config.device[0].bdf, "0000:01:00.0");
        assert_eq!(config.device[0].boot_personality, "vfio");
        assert_eq!(config.device[1].bdf, "0000:02:00.0");
        assert_eq!(config.device[1].boot_personality, "amdgpu");
    }
}
