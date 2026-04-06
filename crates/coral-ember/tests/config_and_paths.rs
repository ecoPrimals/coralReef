// SPDX-License-Identifier: AGPL-3.0-or-later
//! Integration tests for config discovery and path helpers.

use std::sync::Mutex;

use coral_ember::drm_isolation::{default_udev_path, default_xorg_path};
use coral_ember::journal::Journal;
use tempfile::TempDir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Mutate process environment in tests; callers must hold [`ENV_LOCK`].
///
/// # Safety
///
/// In Rust 2024, `set_var`/`remove_var` are `unsafe` (single-threaded mutation invariants).
/// Integration tests serialize env access with [`ENV_LOCK`].
unsafe fn env_set_var(key: &str, value: &str) {
    // SAFETY: `set_var` is unsafe in Rust 2024; callers hold [`ENV_LOCK`].
    unsafe {
        std::env::set_var(key, value);
    }
}

unsafe fn env_remove_var(key: &str) {
    // SAFETY: `remove_var` is unsafe in Rust 2024; callers hold [`ENV_LOCK`].
    unsafe {
        std::env::remove_var(key);
    }
}

fn with_env_cleared<F: FnOnce()>(vars: &[&str], f: F) {
    let _guard = ENV_LOCK.lock().expect("env test lock poisoned");
    let saved: Vec<(&str, Option<String>)> =
        vars.iter().map(|&k| (k, std::env::var(k).ok())).collect();
    for &k in vars {
        // SAFETY: serialized by `ENV_LOCK`.
        unsafe { env_remove_var(k) };
    }
    f();
    for (k, v) in saved {
        match v {
            Some(val) => {
                // SAFETY: serialized by `ENV_LOCK`.
                unsafe { env_set_var(k, &val) };
            }
            None => {
                // SAFETY: serialized by `ENV_LOCK`.
                unsafe { env_remove_var(k) };
            }
        }
    }
}

#[test]
fn ember_socket_path_default_without_env() {
    with_env_cleared(&["CORALREEF_EMBER_SOCKET"], || {
        assert_eq!(
            coral_ember::ember_socket_path(),
            "/run/coralreef/ember.sock"
        );
    });
}

#[test]
fn ember_socket_path_respects_env_override() {
    with_env_cleared(&["CORALREEF_EMBER_SOCKET"], || {
        // SAFETY: `with_env_cleared` holds `ENV_LOCK`.
        unsafe { env_set_var("CORALREEF_EMBER_SOCKET", "/tmp/ember-test.sock") };
        assert_eq!(coral_ember::ember_socket_path(), "/tmp/ember-test.sock");
    });
}

#[test]
fn system_glowplug_config_path_default_without_env() {
    with_env_cleared(&["CORALREEF_GLOWPLUG_CONFIG"], || {
        assert_eq!(
            coral_ember::system_glowplug_config_path(),
            "/etc/coralreef/glowplug.toml"
        );
    });
}

#[test]
fn system_glowplug_config_path_respects_env_override() {
    with_env_cleared(&["CORALREEF_GLOWPLUG_CONFIG"], || {
        // SAFETY: `with_env_cleared` holds `ENV_LOCK`.
        unsafe { env_set_var("CORALREEF_GLOWPLUG_CONFIG", "/opt/coral/glowplug.toml") };
        assert_eq!(
            coral_ember::system_glowplug_config_path(),
            "/opt/coral/glowplug.toml"
        );
    });
}

#[test]
fn system_glowplug_config_path_ignores_empty_env() {
    with_env_cleared(&["CORALREEF_GLOWPLUG_CONFIG"], || {
        // SAFETY: `with_env_cleared` holds `ENV_LOCK`.
        unsafe { env_set_var("CORALREEF_GLOWPLUG_CONFIG", "") };
        assert_eq!(
            coral_ember::system_glowplug_config_path(),
            "/etc/coralreef/glowplug.toml"
        );
    });
}

#[test]
fn find_config_prefers_xdg_over_system_path() {
    let _guard = ENV_LOCK.lock().expect("env test lock poisoned");
    let dir = TempDir::new().expect("tempdir");
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join(".config/coralreef")).expect("mkdir");
    let xdg_cfg = home.join(".config/coralreef/glowplug.toml");
    std::fs::write(&xdg_cfg, "[[device]]\nbdf=\"0000:01:00.0\"\n").expect("write xdg");

    let system = dir.path().join("system.toml");
    std::fs::write(&system, "[[device]]\nbdf=\"0000:02:00.0\"\n").expect("write system");

    let saved_home = std::env::var("HOME").ok();
    let saved_xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let saved_glow = std::env::var("CORALREEF_GLOWPLUG_CONFIG").ok();

    // SAFETY: `ENV_LOCK` held.
    unsafe {
        env_remove_var("XDG_CONFIG_HOME");
        env_set_var("HOME", home.to_str().expect("utf8 home"));
        env_set_var(
            "CORALREEF_GLOWPLUG_CONFIG",
            system.to_str().expect("utf8 system"),
        );
    }

    let found = coral_ember::find_config();
    assert_eq!(found.as_deref(), Some(xdg_cfg.to_str().expect("utf8 xdg")));

    if let Some(v) = saved_home {
        // SAFETY: `ENV_LOCK` held.
        unsafe { env_set_var("HOME", &v) };
    } else {
        unsafe { env_remove_var("HOME") };
    }
    if let Some(v) = saved_xdg {
        unsafe { env_set_var("XDG_CONFIG_HOME", &v) };
    } else {
        unsafe { env_remove_var("XDG_CONFIG_HOME") };
    }
    if let Some(v) = saved_glow {
        unsafe { env_set_var("CORALREEF_GLOWPLUG_CONFIG", &v) };
    } else {
        unsafe { env_remove_var("CORALREEF_GLOWPLUG_CONFIG") };
    }
}

#[test]
fn find_config_falls_back_to_system_when_xdg_missing() {
    let _guard = ENV_LOCK.lock().expect("env test lock poisoned");
    let dir = TempDir::new().expect("tempdir");
    let home = dir.path().join("empty_home");
    std::fs::create_dir_all(&home).expect("mkdir");

    let system = dir.path().join("fallback.toml");
    std::fs::write(&system, "[[device]]\nbdf=\"0000:02:00.0\"\n").expect("write system");

    let saved_home = std::env::var("HOME").ok();
    let saved_xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let saved_glow = std::env::var("CORALREEF_GLOWPLUG_CONFIG").ok();

    unsafe {
        env_remove_var("XDG_CONFIG_HOME");
        env_set_var("HOME", home.to_str().expect("utf8 home"));
        env_set_var(
            "CORALREEF_GLOWPLUG_CONFIG",
            system.to_str().expect("utf8 system"),
        );
    }

    let found = coral_ember::find_config();
    assert_eq!(found.as_deref(), Some(system.to_str().expect("utf8 path")));

    if let Some(v) = saved_home {
        unsafe { env_set_var("HOME", &v) };
    } else {
        unsafe { env_remove_var("HOME") };
    }
    if let Some(v) = saved_xdg {
        unsafe { env_set_var("XDG_CONFIG_HOME", &v) };
    } else {
        unsafe { env_remove_var("XDG_CONFIG_HOME") };
    }
    if let Some(v) = saved_glow {
        unsafe { env_set_var("CORALREEF_GLOWPLUG_CONFIG", &v) };
    } else {
        unsafe { env_remove_var("CORALREEF_GLOWPLUG_CONFIG") };
    }
}

#[test]
fn find_config_none_when_no_candidates_exist() {
    let _guard = ENV_LOCK.lock().expect("env test lock poisoned");
    let dir = TempDir::new().expect("tempdir");
    let home = dir.path().join("nope");
    std::fs::create_dir_all(&home).expect("mkdir");

    let saved_home = std::env::var("HOME").ok();
    let saved_xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let saved_glow = std::env::var("CORALREEF_GLOWPLUG_CONFIG").ok();

    unsafe {
        env_remove_var("XDG_CONFIG_HOME");
        env_set_var("HOME", home.to_str().expect("utf8 home"));
        env_set_var(
            "CORALREEF_GLOWPLUG_CONFIG",
            dir.path().join("nonexistent.toml").to_str().expect("utf8"),
        );
    }

    assert!(coral_ember::find_config().is_none());

    if let Some(v) = saved_home {
        unsafe { env_set_var("HOME", &v) };
    } else {
        unsafe { env_remove_var("HOME") };
    }
    if let Some(v) = saved_xdg {
        unsafe { env_set_var("XDG_CONFIG_HOME", &v) };
    } else {
        unsafe { env_remove_var("XDG_CONFIG_HOME") };
    }
    if let Some(v) = saved_glow {
        unsafe { env_set_var("CORALREEF_GLOWPLUG_CONFIG", &v) };
    } else {
        unsafe { env_remove_var("CORALREEF_GLOWPLUG_CONFIG") };
    }
}

#[test]
fn parse_glowplug_config_accepts_minimal_and_optional_fields() {
    let minimal = r#"
            [[device]]
            bdf = "0000:01:00.0"
        "#;
    let cfg = coral_ember::parse_glowplug_config(minimal).expect("parse");
    assert_eq!(cfg.device.len(), 1);
    assert_eq!(cfg.device[0].bdf, "0000:01:00.0");

    let full = r#"
            [[device]]
            bdf = "0000:02:00.0"
            name = "gpu0"
            boot_personality = "native"
            power_policy = "on"
            role = "compute"
            oracle_dump = "/tmp/oracle.bin"
        "#;
    let cfg = coral_ember::parse_glowplug_config(full).expect("parse");
    assert_eq!(cfg.device[0].name.as_deref(), Some("gpu0"));
    assert_eq!(cfg.device[0].boot_personality.as_deref(), Some("native"));
}

#[test]
fn parse_glowplug_config_empty_device_list() {
    let cfg = coral_ember::parse_glowplug_config("").expect("parse");
    assert!(cfg.device.is_empty());
}

#[test]
fn parse_glowplug_config_rejects_malformed_toml() {
    let toml = "[[device]]\n bdf = ";
    assert!(coral_ember::parse_glowplug_config(toml).is_err());
}

#[test]
fn parse_glowplug_config_multiple_devices() {
    let toml = r#"
        [[device]]
        bdf = "0000:01:00.0"
        name = "a"

        [[device]]
        bdf = "0000:02:00.0"
        role = "compute"

        [[device]]
        bdf = "0000:03:00.0"
    "#;
    let cfg = coral_ember::parse_glowplug_config(toml).expect("parse");
    assert_eq!(cfg.device.len(), 3);
    assert_eq!(cfg.device[0].bdf, "0000:01:00.0");
    assert_eq!(cfg.device[0].name.as_deref(), Some("a"));
    assert_eq!(cfg.device[1].role.as_deref(), Some("compute"));
    assert!(cfg.device[2].name.is_none());
}

#[test]
fn parse_glowplug_config_ignores_unknown_top_level_and_device_keys() {
    let toml = r#"
        schema_version = 1
        [[device]]
        bdf = "0000:01:00.0"
        future_flag = true
    "#;
    let cfg = coral_ember::parse_glowplug_config(toml).expect("parse");
    assert_eq!(cfg.device.len(), 1);
    assert_eq!(cfg.device[0].bdf, "0000:01:00.0");
}

#[test]
fn find_config_prefers_explicit_xdg_config_home() {
    let _guard = ENV_LOCK.lock().expect("env test lock");
    let dir = TempDir::new().expect("tempdir");
    let xdg_base = dir.path().join("xdg");
    let xdg_cfg = xdg_base.join("coralreef/glowplug.toml");
    std::fs::create_dir_all(xdg_cfg.parent().expect("parent")).expect("mkdir");
    std::fs::write(&xdg_cfg, "[[device]]\nbdf=\"0000:01:00.0\"\n").expect("write");

    let saved_xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let saved_home = std::env::var("HOME").ok();
    let saved_glow = std::env::var("CORALREEF_GLOWPLUG_CONFIG").ok();

    unsafe {
        env_set_var("XDG_CONFIG_HOME", xdg_base.to_str().expect("utf8 xdg base"));
        env_set_var("HOME", "/nonexistent-coral-ember-home-for-test");
        env_set_var(
            "CORALREEF_GLOWPLUG_CONFIG",
            dir.path()
                .join("missing-system.toml")
                .to_str()
                .expect("utf8"),
        );
    }

    let found = coral_ember::find_config();
    assert_eq!(found.as_deref(), Some(xdg_cfg.to_str().expect("utf8 path")));

    if let Some(v) = saved_xdg {
        unsafe { env_set_var("XDG_CONFIG_HOME", &v) };
    } else {
        unsafe { env_remove_var("XDG_CONFIG_HOME") };
    }
    if let Some(v) = saved_home {
        unsafe { env_set_var("HOME", &v) };
    } else {
        unsafe { env_remove_var("HOME") };
    }
    if let Some(v) = saved_glow {
        unsafe { env_set_var("CORALREEF_GLOWPLUG_CONFIG", &v) };
    } else {
        unsafe { env_remove_var("CORALREEF_GLOWPLUG_CONFIG") };
    }
}

#[test]
fn default_xorg_path_respects_coralreef_xorg_conf_dir_env() {
    let _guard = ENV_LOCK.lock().expect("env test lock");
    let dir = TempDir::new().expect("tempdir");
    let prev = std::env::var("CORALREEF_XORG_CONF_DIR").ok();
    unsafe {
        env_set_var(
            "CORALREEF_XORG_CONF_DIR",
            dir.path().to_str().expect("utf8 temp path"),
        );
    }
    let path = default_xorg_path();
    if let Some(v) = prev {
        unsafe { env_set_var("CORALREEF_XORG_CONF_DIR", &v) };
    } else {
        unsafe { env_remove_var("CORALREEF_XORG_CONF_DIR") };
    }
    let expected = dir
        .path()
        .join("11-coralreef-gpu-isolation.conf")
        .to_string_lossy()
        .into_owned();
    assert_eq!(path, expected);
}

#[test]
fn default_udev_path_respects_coralreef_udev_rules_dir_env() {
    let _guard = ENV_LOCK.lock().expect("env test lock");
    let dir = TempDir::new().expect("tempdir");
    let prev = std::env::var("CORALREEF_UDEV_RULES_DIR").ok();
    unsafe {
        env_set_var(
            "CORALREEF_UDEV_RULES_DIR",
            dir.path().to_str().expect("utf8 temp path"),
        );
    }
    let path = default_udev_path();
    if let Some(v) = prev {
        unsafe { env_set_var("CORALREEF_UDEV_RULES_DIR", &v) };
    } else {
        unsafe { env_remove_var("CORALREEF_UDEV_RULES_DIR") };
    }
    let expected = dir
        .path()
        .join("61-coralreef-drm-ignore.rules")
        .to_string_lossy()
        .into_owned();
    assert_eq!(path, expected);
}

#[test]
fn journal_open_default_respects_coralreef_journal_path_env() {
    let _guard = ENV_LOCK.lock().expect("env test lock");
    let dir = TempDir::new().expect("tempdir");
    let journal_path = dir.path().join("from-env.jsonl");
    let path_str = journal_path.to_str().expect("utf8 journal path");
    let prev = std::env::var("CORALREEF_JOURNAL_PATH").ok();
    unsafe {
        env_set_var("CORALREEF_JOURNAL_PATH", path_str);
    }
    let journal = Journal::open_default();
    if let Some(v) = prev {
        unsafe { env_set_var("CORALREEF_JOURNAL_PATH", &v) };
    } else {
        unsafe { env_remove_var("CORALREEF_JOURNAL_PATH") };
    }
    assert_eq!(journal.path(), journal_path.as_path());
}
