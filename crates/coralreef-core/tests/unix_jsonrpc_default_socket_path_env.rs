// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(unsafe_code)]
//! `default_unix_socket_path` vs socket resolution tiers (integration tests; may use `unsafe` env).

#![cfg(unix)]

use std::sync::Mutex;

use coralreef_core::ipc::{default_unix_socket_path, unix_socket_path_for_base};

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvRestore {
    xdg: Option<String>,
    socket_dir: Option<String>,
}

impl EnvRestore {
    fn capture() -> Self {
        Self {
            xdg: std::env::var("XDG_RUNTIME_DIR").ok(),
            socket_dir: std::env::var("BIOMEOS_SOCKET_DIR").ok(),
        }
    }

    fn restore(self) {
        // SAFETY: Serialized by ENV_LOCK; integration test process only.
        unsafe {
            match self.xdg {
                Some(v) => std::env::set_var("XDG_RUNTIME_DIR", v),
                None => std::env::remove_var("XDG_RUNTIME_DIR"),
            }
            match self.socket_dir {
                Some(v) => std::env::set_var("BIOMEOS_SOCKET_DIR", v),
                None => std::env::remove_var("BIOMEOS_SOCKET_DIR"),
            }
        }
    }
}

#[test]
fn default_socket_path_uses_run_when_no_env() {
    let _guard = ENV_LOCK.lock().unwrap();
    let prev = EnvRestore::capture();
    // SAFETY: Serialized by ENV_LOCK.
    unsafe {
        std::env::remove_var("XDG_RUNTIME_DIR");
        std::env::remove_var("BIOMEOS_SOCKET_DIR");
    }

    let got = default_unix_socket_path();
    assert!(
        got.starts_with("/run/biomeos"),
        "without XDG or BIOMEOS_SOCKET_DIR, should resolve to /run/biomeos: {got:?}"
    );
    assert!(
        !got.starts_with("/tmp"),
        "must never fall back to /tmp: {got:?}"
    );

    prev.restore();
}

#[test]
fn default_socket_path_respects_xdg_runtime_dir() {
    let _guard = ENV_LOCK.lock().unwrap();
    let prev = EnvRestore::capture();
    let temp = tempfile::tempdir().unwrap();
    let xdg = temp.path().to_path_buf();
    // SAFETY: Serialized by ENV_LOCK.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", xdg.as_os_str());
        std::env::remove_var("BIOMEOS_SOCKET_DIR");
    }

    let got = default_unix_socket_path();
    let want = unix_socket_path_for_base(Some(xdg.clone()));
    assert_eq!(got, want);
    assert!(
        got.starts_with(&xdg),
        "should use the XDG_RUNTIME_DIR value: {got:?}"
    );

    prev.restore();
}

#[test]
fn default_socket_path_empty_xdg_falls_to_run() {
    let _guard = ENV_LOCK.lock().unwrap();
    let prev = EnvRestore::capture();
    // SAFETY: Serialized by ENV_LOCK.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", "");
        std::env::remove_var("BIOMEOS_SOCKET_DIR");
    }

    let got = default_unix_socket_path();
    assert!(
        got.starts_with("/run/biomeos"),
        "empty XDG should fall through to /run/biomeos: {got:?}"
    );
    assert!(
        !got.starts_with("/tmp"),
        "must never fall back to /tmp: {got:?}"
    );

    prev.restore();
}

#[test]
fn biomeos_socket_dir_takes_priority() {
    let _guard = ENV_LOCK.lock().unwrap();
    let prev = EnvRestore::capture();
    let temp = tempfile::tempdir().unwrap();
    let custom_dir = temp.path().join("custom-sockets");
    std::fs::create_dir_all(&custom_dir).unwrap();
    // SAFETY: Serialized by ENV_LOCK.
    unsafe {
        std::env::set_var("BIOMEOS_SOCKET_DIR", custom_dir.as_os_str());
        std::env::set_var("XDG_RUNTIME_DIR", "/this-should-be-ignored");
    }

    let got = default_unix_socket_path();
    assert!(
        got.starts_with(&custom_dir),
        "BIOMEOS_SOCKET_DIR should take priority: {got:?}"
    );

    prev.restore();
}
