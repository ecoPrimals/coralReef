// SPDX-License-Identifier: AGPL-3.0-or-later

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

#[test]
fn device_config_role_helpers() {
    let display = DeviceConfig {
        bdf: "0000:01:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: Some("display".into()),
        oracle_dump: None,
        shared: None,
    };
    assert!(display.is_display());
    assert!(!display.is_shared());
    assert!(display.is_protected());

    let shared = DeviceConfig {
        role: Some("shared".into()),
        ..display.clone()
    };
    assert!(!shared.is_display());
    assert!(shared.is_shared());
    assert!(shared.is_protected());

    let compute = DeviceConfig {
        role: Some("compute".into()),
        ..display
    };
    assert!(!compute.is_display());
    assert!(!compute.is_shared());
    assert!(!compute.is_protected());
}

#[test]
fn shared_quota_default_compute_mode() {
    let q = SharedQuota::default();
    assert_eq!(q.compute_mode, "default");
    assert_eq!(q.compute_priority, 0);
    assert!(q.power_limit_w.is_none());
    assert!(q.vram_budget_mib.is_none());
}

#[test]
fn load_shared_quota_and_compute_priority() {
    let path = write_temp_config(
        r#"
[[device]]
bdf = "0000:04:00.0"
role = "shared"
shared = { power_limit_w = 220, vram_budget_mib = 4096, compute_mode = "exclusive_process", compute_priority = 1 }
"#,
        "shared_quota",
    );
    let cfg = Config::load(path.to_str().expect("utf8 path")).expect("parse");
    let _ = std::fs::remove_file(&path);
    let shared = cfg.device[0].shared.as_ref().expect("shared table");
    assert_eq!(shared.power_limit_w, Some(220));
    assert_eq!(shared.vram_budget_mib, Some(4096));
    assert_eq!(shared.compute_mode, "exclusive_process");
    assert_eq!(shared.compute_priority, 1);
}

#[test]
fn daemon_config_deserialize_partial_preserves_other_defaults() {
    let path = write_temp_config(
        r#"
[daemon]
log_level = "warn"
"#,
        "daemon_partial",
    );
    let cfg = Config::load(path.to_str().expect("utf8 path")).expect("parse");
    let _ = std::fs::remove_file(&path);
    assert_eq!(cfg.daemon.log_level, "warn");
    assert_eq!(cfg.daemon.health_interval_ms, 5000);
    #[cfg(unix)]
    assert!(cfg.daemon.socket.contains("coral-glowplug"));
}

#[test]
fn system_config_path_uses_glowplug_filename() {
    assert!(system_config_path().ends_with("glowplug.toml"));
}
