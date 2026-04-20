// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(unsafe_code)]
//! Integration tests for ecosystem discovery and registration.
//!
//! Environment mutation uses `unsafe` (Rust 1.85+); the `coralreef-core` library
//! crate forbids `unsafe_code`, so these tests live in the integration test crate.

use std::io::Write;

use coralreef_core::config;
use coralreef_core::ecosystem::{discover_ecosystem_jsonrpc_bind, spawn_registration};

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvRestore {
    key: &'static str,
    previous: Option<String>,
}

impl EnvRestore {
    fn take(key: &'static str) -> Self {
        let previous = std::env::var(key).ok();
        Self { key, previous }
    }

    fn set(&mut self, value: &str) {
        // SAFETY: `ENV_LOCK` is held for the whole test; no concurrent env access.
        unsafe {
            std::env::set_var(self.key, value);
        }
    }

    fn remove(&mut self) {
        // SAFETY: `ENV_LOCK` is held for the whole test; no concurrent env access.
        unsafe {
            std::env::remove_var(self.key);
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        // SAFETY: `ENV_LOCK` is still held by the test guard when `drop` runs.
        unsafe {
            match &self.previous {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

#[test]
fn discover_ecosystem_jsonrpc_bind_prefers_biomeos_registry_trimmed() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut bio = EnvRestore::take("BIOMEOS_ECOSYSTEM_REGISTRY");
    bio.set("  unix:///tmp/registry-trimmed.sock  ");
    let got = discover_ecosystem_jsonrpc_bind();
    assert_eq!(got.as_deref(), Some("unix:///tmp/registry-trimmed.sock"));
}

#[test]
fn discover_ecosystem_jsonrpc_bind_biomeos_whitespace_only_falls_back_to_scan() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut bio = EnvRestore::take("BIOMEOS_ECOSYSTEM_REGISTRY");
    let mut xdg = EnvRestore::take("XDG_RUNTIME_DIR");
    let tmp = tempfile::tempdir().expect("tempdir");
    let biomeos = tmp.path().join(config::ECOSYSTEM_NAMESPACE);
    std::fs::create_dir_all(&biomeos).expect("create_dir_all");
    let path = biomeos.join("reg.json");
    let j = serde_json::json!({
        "provides": ["capability.register"],
        "endpoint": "unix:///tmp/from-scan.sock"
    });
    let mut f = std::fs::File::create(&path).expect("create");
    write!(f, "{j}").expect("write");

    bio.set("   ");
    xdg.set(tmp.path().to_str().expect("utf8 path"));
    let got = discover_ecosystem_jsonrpc_bind();
    assert_eq!(got.as_deref(), Some("unix:///tmp/from-scan.sock"));
}

#[test]
fn discover_ecosystem_jsonrpc_bind_empty_discovery_dir() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut bio = EnvRestore::take("BIOMEOS_ECOSYSTEM_REGISTRY");
    let mut xdg = EnvRestore::take("XDG_RUNTIME_DIR");
    let tmp = tempfile::tempdir().expect("tempdir");
    let biomeos = tmp.path().join(config::ECOSYSTEM_NAMESPACE);
    std::fs::create_dir_all(&biomeos).expect("create_dir_all");

    bio.remove();
    xdg.set(tmp.path().to_str().expect("utf8 path"));
    let got = discover_ecosystem_jsonrpc_bind();
    assert!(got.is_none(), "expected no registry in empty discovery dir");
}

#[test]
fn discover_ecosystem_jsonrpc_bind_skips_malformed_json_files() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut bio = EnvRestore::take("BIOMEOS_ECOSYSTEM_REGISTRY");
    let mut xdg = EnvRestore::take("XDG_RUNTIME_DIR");
    let tmp = tempfile::tempdir().expect("tempdir");
    let biomeos = tmp.path().join(config::ECOSYSTEM_NAMESPACE);
    std::fs::create_dir_all(&biomeos).expect("create_dir_all");
    std::fs::write(biomeos.join("bad.json"), "{ not valid json").expect("write");

    bio.remove();
    xdg.set(tmp.path().to_str().expect("utf8 path"));
    let got = discover_ecosystem_jsonrpc_bind();
    assert!(got.is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn spawn_registration_no_registry_returns_without_panic() {
    let tmp = tempfile::tempdir().expect("tempdir");
    {
        let _guard = ENV_LOCK.lock().unwrap();
        let mut bio = EnvRestore::take("BIOMEOS_ECOSYSTEM_REGISTRY");
        let mut xdg = EnvRestore::take("XDG_RUNTIME_DIR");
        let biomeos = tmp.path().join(config::ECOSYSTEM_NAMESPACE);
        std::fs::create_dir_all(&biomeos).expect("create_dir_all");

        bio.remove();
        xdg.set(tmp.path().to_str().expect("utf8 path"));
        spawn_registration(coralreef_core::capability::self_description());
    }
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
}

#[cfg(unix)]
#[tokio::test]
async fn spawn_registration_non_unix_bind_skips_background_tasks() {
    {
        let _guard = ENV_LOCK.lock().unwrap();
        let mut bio = EnvRestore::take("BIOMEOS_ECOSYSTEM_REGISTRY");
        bio.set("127.0.0.1:65530");
        spawn_registration(coralreef_core::capability::self_description());
    }
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
}
