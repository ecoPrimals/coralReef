// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(unsafe_code)]
//! Environment-variable overrides for config search paths and TCP/socket defaults.
//!
//! Lives in `tests/` so the library unit tests stay `#![forbid(unsafe_code)]`;
//! mutating process environment requires `unsafe` on Rust 2024.

use coral_glowplug::config::{DaemonConfig, config_search_paths, default_tcp_fallback};
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn config_search_paths_prefers_coralreef_config() {
    let _guard = ENV_LOCK.lock().expect("env test lock");
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("glowplug_env.toml");
    std::fs::write(&path, "").expect("write");
    let path_str = path.to_str().expect("utf8 path").to_owned();
    // SAFETY: serialized by `ENV_LOCK`; no concurrent env access in this process.
    unsafe {
        std::env::set_var("CORALREEF_CONFIG", &path_str);
    }
    let paths = config_search_paths();
    // SAFETY: serialized by `ENV_LOCK`; no concurrent env access in this process.
    unsafe {
        std::env::remove_var("CORALREEF_CONFIG");
    }
    assert_eq!(paths, vec![path_str]);
}

#[test]
fn config_search_paths_uses_xdg_when_coralreef_config_unset() {
    let _guard = ENV_LOCK.lock().expect("env test lock");
    // SAFETY: serialized by `ENV_LOCK`; no concurrent env access in this process.
    unsafe {
        std::env::remove_var("CORALREEF_CONFIG");
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg_cfg_test_glowplug");
        std::env::set_var("HOME", "/tmp/home_test_glowplug");
    }
    let paths = config_search_paths();
    // SAFETY: serialized by `ENV_LOCK`; no concurrent env access in this process.
    unsafe {
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("HOME");
    }
    assert_eq!(paths.len(), 2);
    assert_eq!(
        paths[0],
        "/tmp/xdg_cfg_test_glowplug/coralreef/glowplug.toml"
    );
    assert!(paths[1].ends_with("glowplug.toml"));
}

#[test]
fn default_tcp_fallback_respects_coralreef_tcp_bind() {
    let _guard = ENV_LOCK.lock().expect("env test lock");
    // SAFETY: serialized by `ENV_LOCK`; no concurrent env access in this process.
    unsafe {
        std::env::set_var("CORALREEF_TCP_BIND", "127.0.0.1:4242");
    }
    let bind = default_tcp_fallback();
    // SAFETY: serialized by `ENV_LOCK`; no concurrent env access in this process.
    unsafe {
        std::env::remove_var("CORALREEF_TCP_BIND");
    }
    assert_eq!(bind, "127.0.0.1:4242");
}

#[cfg(unix)]
#[test]
fn default_daemon_socket_includes_biomeos_family_id() {
    let _guard = ENV_LOCK.lock().expect("env test lock");
    // SAFETY: serialized by `ENV_LOCK`; no concurrent env access in this process.
    unsafe {
        std::env::set_var("BIOMEOS_FAMILY_ID", "unit-test-family");
        std::env::set_var("XDG_RUNTIME_DIR", "/tmp/rt_glowplug_test");
    }
    let sock = DaemonConfig::default().socket;
    // SAFETY: serialized by `ENV_LOCK`; no concurrent env access in this process.
    unsafe {
        std::env::remove_var("BIOMEOS_FAMILY_ID");
        std::env::remove_var("XDG_RUNTIME_DIR");
    }
    assert!(
        sock.contains("unit-test-family"),
        "socket path should include BIOMEOS_FAMILY_ID: {sock}"
    );
    assert!(sock.ends_with(".sock"));
}
