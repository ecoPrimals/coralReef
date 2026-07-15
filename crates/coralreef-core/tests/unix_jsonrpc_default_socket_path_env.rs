// SPDX-License-Identifier: AGPL-3.0-or-later
//! `default_unix_socket_path` vs socket resolution tiers (integration tests).
//!
//! Uses the shared [`test_env::EnvGuard`] helper to safely mutate env vars
//! under a process-wide lock (Rust 1.85+ marks `env::set_var` as `unsafe`).

#![cfg(unix)]

#[path = "test_env.rs"]
mod test_env;

use coralreef_core::config::ecosystem_namespace;
use coralreef_core::ipc::{default_unix_socket_path, unix_socket_path_for_base};
use test_env::{ENV_LOCK, EnvGuard};

#[test]
fn default_socket_path_uses_run_or_temp_when_no_env() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut xdg = EnvGuard::capture("XDG_RUNTIME_DIR");
    let mut socket_dir = EnvGuard::capture("BIOMEOS_SOCKET_DIR");
    xdg.remove();
    socket_dir.remove();

    let ns = ecosystem_namespace();
    let got = default_unix_socket_path();
    let run_dir = format!("/run/{ns}");
    let run_exists = std::path::Path::new(&run_dir).exists();
    if run_exists {
        assert!(
            got.starts_with(&run_dir),
            "with {run_dir} present, should resolve there: {got:?}"
        );
    } else {
        let temp = std::env::temp_dir().join(format!("{ns}-runtime"));
        assert!(
            got.starts_with(&temp),
            "without {run_dir}, should fall to temp dir: {got:?} (expected prefix: {temp:?})"
        );
    }
}

#[test]
fn default_socket_path_respects_xdg_runtime_dir() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut xdg = EnvGuard::capture("XDG_RUNTIME_DIR");
    let mut socket_dir = EnvGuard::capture("BIOMEOS_SOCKET_DIR");
    let temp = tempfile::tempdir().unwrap();
    let xdg_path = temp.path().to_path_buf();

    xdg.set(xdg_path.to_str().expect("utf8 path"));
    socket_dir.remove();

    let got = default_unix_socket_path();
    let want = unix_socket_path_for_base(Some(xdg_path.clone()));
    assert_eq!(got, want);
    assert!(
        got.starts_with(&xdg_path),
        "should use the XDG_RUNTIME_DIR value: {got:?}"
    );
}

#[test]
fn default_socket_path_empty_xdg_falls_to_run_or_temp() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut xdg = EnvGuard::capture("XDG_RUNTIME_DIR");
    let mut socket_dir = EnvGuard::capture("BIOMEOS_SOCKET_DIR");
    xdg.set("");
    socket_dir.remove();

    let ns = ecosystem_namespace();
    let got = default_unix_socket_path();
    let run_dir = format!("/run/{ns}");
    let run_exists = std::path::Path::new(&run_dir).exists();
    if run_exists {
        assert!(
            got.starts_with(&run_dir),
            "empty XDG + {run_dir} present → should use {run_dir}: {got:?}"
        );
    } else {
        let temp = std::env::temp_dir().join(format!("{ns}-runtime"));
        assert!(
            got.starts_with(&temp),
            "empty XDG + no {run_dir} → should fall to temp dir: {got:?}"
        );
    }
}

#[test]
fn biomeos_socket_dir_takes_priority() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut socket_dir = EnvGuard::capture("BIOMEOS_SOCKET_DIR");
    let mut xdg = EnvGuard::capture("XDG_RUNTIME_DIR");
    let temp = tempfile::tempdir().unwrap();
    let custom_dir = temp.path().join("custom-sockets");
    std::fs::create_dir_all(&custom_dir).unwrap();

    socket_dir.set(custom_dir.to_str().expect("utf8 path"));
    xdg.set("/this-should-be-ignored");

    let got = default_unix_socket_path();
    assert!(
        got.starts_with(&custom_dir),
        "BIOMEOS_SOCKET_DIR should take priority: {got:?}"
    );
}
