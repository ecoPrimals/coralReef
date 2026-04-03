// SPDX-License-Identifier: AGPL-3.0-only
//! `default_unix_socket_path` vs `$XDG_RUNTIME_DIR` (integration tests; may use `unsafe` env).

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Mutex;

use coralreef_core::ipc::{default_unix_socket_path, unix_socket_path_for_base};

static XDG_ENV_LOCK: Mutex<()> = Mutex::new(());

fn restore_xdg_runtime_dir(previous: Option<String>) {
    // SAFETY: Serialized by `XDG_ENV_LOCK`; integration test process only.
    unsafe {
        match previous {
            Some(value) => std::env::set_var("XDG_RUNTIME_DIR", value),
            None => std::env::remove_var("XDG_RUNTIME_DIR"),
        }
    }
}

#[test]
fn default_unix_socket_path_when_xdg_unset_matches_temp_base() {
    let _guard = XDG_ENV_LOCK.lock().unwrap();
    let previous = std::env::var("XDG_RUNTIME_DIR").ok();
    // SAFETY: serialized by `XDG_ENV_LOCK`; no concurrent env access.
    unsafe {
        std::env::remove_var("XDG_RUNTIME_DIR");
    }

    let got = default_unix_socket_path();
    let want = unix_socket_path_for_base(None);
    assert_eq!(got, want);

    restore_xdg_runtime_dir(previous);
}

#[test]
fn default_unix_socket_path_when_xdg_set_to_directory() {
    let _guard = XDG_ENV_LOCK.lock().unwrap();
    let previous = std::env::var("XDG_RUNTIME_DIR").ok();
    let temp = tempfile::tempdir().unwrap();
    let xdg = temp.path().to_path_buf();
    // SAFETY: serialized by `XDG_ENV_LOCK`; no concurrent env access.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", xdg.as_os_str());
    }

    let got = default_unix_socket_path();
    let want = unix_socket_path_for_base(Some(xdg));
    assert_eq!(got, want);

    restore_xdg_runtime_dir(previous);
}

#[test]
fn default_unix_socket_path_when_xdg_empty_string() {
    let _guard = XDG_ENV_LOCK.lock().unwrap();
    let previous = std::env::var("XDG_RUNTIME_DIR").ok();
    // SAFETY: serialized by `XDG_ENV_LOCK`; no concurrent env access.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", "");
    }

    let got = default_unix_socket_path();
    let want = unix_socket_path_for_base(Some(PathBuf::new()));
    assert_eq!(got, want);

    restore_xdg_runtime_dir(previous);
}
