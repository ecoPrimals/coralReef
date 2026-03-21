// SPDX-License-Identifier: AGPL-3.0-only
#![expect(
    missing_docs,
    reason = "config schema fields mirror TOML keys; crate-level lib.rs documents the public surface."
)]
//! TOML configuration for the `GlowPlug` daemon.

use coral_driver::linux_paths;
use serde::Deserialize;
use std::sync::OnceLock;

/// XDG config subdirectory for coralReef ecosystem.
const CONFIG_SUBDIR: &str = "coralreef";
/// Default config filename.
const CONFIG_FILENAME: &str = "glowplug.toml";
/// System-wide glowplug config path when `$CORALREEF_GLOWPLUG_CONFIG` is unset.
const DEFAULT_SYSTEM_GLOWPLUG_CONFIG: &str = "/etc/coralreef/glowplug.toml";
/// Fallback home directory when `HOME` and `$CORALREEF_HOME_FALLBACK` are unset (container-style).
const DEFAULT_HOME_FALLBACK: &str = "/root";

/// System-wide glowplug config path (fallback after XDG).
///
/// Override with `$CORALREEF_GLOWPLUG_CONFIG`; defaults to `/etc/coralreef/glowplug.toml`.
#[must_use]
pub fn system_config_path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        std::env::var("CORALREEF_GLOWPLUG_CONFIG")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_SYSTEM_GLOWPLUG_CONFIG.to_string())
    })
    .as_str()
}

fn home_fallback_dir() -> String {
    static H: OnceLock<String> = OnceLock::new();
    H.get_or_init(|| {
        std::env::var("CORALREEF_HOME_FALLBACK")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_HOME_FALLBACK.to_string())
    })
    .clone()
}

/// Config resolution order: CLI `--config` > `$CORALREEF_CONFIG` > `XDG_CONFIG_HOME` config > system fallback.
///
/// Returns candidate paths in search order. Callers try loading each until one succeeds.
#[must_use]
pub fn config_search_paths() -> Vec<String> {
    if let Ok(path) = std::env::var("CORALREEF_CONFIG")
        && !path.is_empty()
    {
        return vec![path];
    }
    let xdg_config = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        format!(
            "{}/.config",
            std::env::var("HOME").unwrap_or_else(|_| home_fallback_dir()),
        )
    });
    vec![
        format!("{xdg_config}/{CONFIG_SUBDIR}/{CONFIG_FILENAME}"),
        system_config_path().to_owned(),
    ]
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub device: Vec<DeviceConfig>,
}

/// GPU vendor IDs worth managing.
const GPU_VENDORS: &[(u16, &str)] = &[(0x10de, "nvidia"), (0x1002, "amd")];

/// PCI class code for VGA controllers.
const PCI_CLASS_VGA: u64 = 0x0300;
/// PCI class code for 3D controllers.
const PCI_CLASS_3D: u64 = 0x0302;

const DEFAULT_PERSONALITY: &str = "vfio";
const DEFAULT_POWER_POLICY: &str = "always_on";
const DEFAULT_ROLE: &str = "compute";

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
    /// Loaded from TOML config, used by `device.rs` `snapshot_registers`.
    #[serde(default)]
    pub oracle_dump: Option<String>,
}

/// TCP loopback with OS-assigned port (ecoBin fallback on non-Unix platforms).
///
/// Kept in sync with `coralreef-core::ipc::FALLBACK_TCP_BIND` — coral-glowplug does not
/// depend on coralreef-core, so both define this constant for ecoBin compliance.
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

/// Runtime filesystem segment for IPC socket layout per wateringHole `PRIMAL_IPC_PROTOCOL` v3.0.
///
/// This is a protocol-defined path component, not a capability registry or primal lookup key.
const ECOSYSTEM_NAMESPACE: &str = "biomeos";

/// Instance isolation id for socket filenames (from `$BIOMEOS_FAMILY_ID`, default `"default"`).
fn family_id() -> String {
    std::env::var("BIOMEOS_FAMILY_ID").unwrap_or_else(|_| "default".into())
}

/// Platform-aware default socket address (ecoBin / wateringHole compliance).
///
/// On Unix: `$XDG_RUNTIME_DIR/{ECOSYSTEM_NAMESPACE}/<crate>-<family_id>.sock`
/// (or `$TMPDIR/{ECOSYSTEM_NAMESPACE}/` if `XDG_RUNTIME_DIR` unset).
/// On non-Unix: TCP fallback to `127.0.0.1:0` (OS-assigned port).
#[must_use]
fn default_socket() -> String {
    #[cfg(unix)]
    {
        let base = std::env::var("XDG_RUNTIME_DIR")
            .map_or_else(|_| std::env::temp_dir(), std::path::PathBuf::from);
        let sock_name = format!("{}-{}.sock", env!("CARGO_PKG_NAME"), family_id());
        base.join(ECOSYSTEM_NAMESPACE)
            .join(sock_name)
            .display()
            .to_string()
    }
    #[cfg(not(unix))]
    {
        default_tcp_fallback()
    }
}
fn default_log_level() -> String {
    "info".into()
}
const fn default_health_interval() -> u64 {
    5000
}
fn default_personality() -> String {
    DEFAULT_PERSONALITY.into()
}
fn default_power_policy() -> String {
    DEFAULT_POWER_POLICY.into()
}

impl Config {
    /// Load and parse a TOML config file.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError::ReadFailed` if the file can't be read, or
    /// `ConfigError::ParseFailed` if the TOML is malformed.
    pub fn load(path: &str) -> Result<Self, crate::error::ConfigError> {
        let content =
            std::fs::read_to_string(path).map_err(|e| crate::error::ConfigError::ReadFailed {
                path: path.to_owned(),
                source: e,
            })?;
        toml::from_str(&content).map_err(|e| crate::error::ConfigError::ParseFailed {
            path: path.to_owned(),
            source: e,
        })
    }

    /// Build a config by scanning the PCI bus for discrete GPUs.
    ///
    /// Skips any device currently bound to proprietary `nvidia` driver
    /// (assumed to be the display card). Assigns `vfio` to everything else.
    pub fn auto_discover() -> Self {
        let mut devices = Vec::new();

        let Ok(entries) = std::fs::read_dir(linux_paths::sysfs_pci_devices()) else {
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

            let class_top = (class >> 8) & 0xFFFF;
            if class_top != PCI_CLASS_VGA && class_top != PCI_CLASS_3D {
                continue;
            }

            // Only known GPU vendors
            let vendor_name = GPU_VENDORS
                .iter()
                .find(|(vid, _)| u16::try_from(vendor).is_ok_and(|v| v == *vid))
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

            let personality = match &driver {
                Some(s) if s == "nouveau" || s == "amdgpu" => s.clone(),
                _ => DEFAULT_PERSONALITY.into(),
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
                power_policy: DEFAULT_POWER_POLICY.into(),
                role: Some(DEFAULT_ROLE.into()),
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
        let msg = err.to_string();
        assert!(msg.contains("read config") || msg.contains("failed to read"));
        assert!(msg.contains("/nonexistent/path/glowplug.toml"));
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
        let msg = err.to_string();
        assert!(msg.contains("parse config") || msg.contains("failed to parse"));
    }

    #[test]
    fn test_load_invalid_structure() {
        let path = write_temp_config(
            r"
[[device]]
bdf = 12345
",
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
        assert!(default.socket.contains("coral-glowplug") && default.socket.ends_with(".sock"));
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

    #[test]
    fn auto_discover_returns_daemon_defaults() {
        let cfg = Config::auto_discover();
        assert_eq!(cfg.daemon.log_level, "info");
        assert_eq!(cfg.daemon.health_interval_ms, 5000);
        assert!(cfg.device.len() <= 256);
    }

    #[test]
    fn test_load_daemon_defaults_merge_with_devices() {
        let path = write_temp_config(
            r#"
[daemon]
socket = "/run/custom/glowplug.sock"

[[device]]
bdf = "0000:01:00.0"
name = "GPU A"
boot_personality = "vfio"
power_policy = "always_on"
role = "render"

[[device]]
bdf = "0000:02:00.0"
boot_personality = "nouveau"
power_policy = "power_save"
oracle_dump = "/tmp/oracle-a.txt"

[[device]]
bdf = "0000:03:00.0"
name = "Akida"
boot_personality = "akida-pcie"
role = "npu"
"#,
            "daemon_merge",
        );
        let path_str = path.to_str().expect("path has str");
        let config = Config::load(path_str).expect("load");
        let _ = std::fs::remove_file(&path);
        assert_eq!(config.daemon.socket, "/run/custom/glowplug.sock");
        assert_eq!(config.daemon.log_level, "info");
        assert_eq!(config.device.len(), 3);
        assert_eq!(config.device[0].name.as_deref(), Some("GPU A"));
        assert_eq!(
            config.device[1].oracle_dump.as_deref(),
            Some("/tmp/oracle-a.txt")
        );
        assert_eq!(config.device[2].boot_personality, "akida-pcie");
        assert_eq!(config.device[2].role.as_deref(), Some("npu"));
    }
}
