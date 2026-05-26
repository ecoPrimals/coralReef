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

/// Simulate the full toadStool → coralReef discovery pipeline:
/// 1. toadStool publishes a discovery JSON advertising GPU capabilities
/// 2. coralReef discovers the GPU target from the shared directory
/// 3. coralReef compiles a shader for the discovered architecture
///
/// This validates the "node atomic" pattern: discover by capability, not name.
#[test]
fn live_discovery_toadstool_gpu_target_to_compile() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut xdg = EnvRestore::take("XDG_RUNTIME_DIR");
    let tmp = tempfile::tempdir().expect("tempdir");
    let discovery_dir = tmp.path().join(config::ECOSYSTEM_NAMESPACE);
    std::fs::create_dir_all(&discovery_dir).expect("create discovery dir");

    let toadstool_entry = serde_json::json!({
        "primal": "toadstool",
        "version": "0.2.0",
        "pid": 99999,
        "provides": [
            {"id": "compute.dispatch.submit", "version": "0.1.0"},
            {"id": "compute.dispatch.capabilities", "version": "0.1.0"},
            {"id": "gpu.dispatch", "version": "0.1.0"}
        ],
        "transports": {
            "jsonrpc": {"bind": "unix:///run/user/1000/biomeos/toadstool.sock"}
        },
        "devices": [
            {
                "vendor": "nvidia",
                "arch": "sm86",
                "render_node": "/dev/dri/renderD128",
                "driver": "vfio-pci",
                "memory_bytes": 25_769_803_776_u64
            },
            {
                "vendor": "amd",
                "arch": "rdna2",
                "render_node": "/dev/dri/renderD129",
                "driver": "amdgpu",
                "memory_bytes": 17_179_869_184_u64
            }
        ]
    });
    let path = discovery_dir.join("toadstool.json");
    std::fs::write(&path, toadstool_entry.to_string()).expect("write discovery");

    xdg.set(tmp.path().to_str().expect("utf8"));

    let devices = coralreef_core::discovery::discover_gpu_devices();
    assert_eq!(devices.len(), 2, "should discover 2 GPU devices from toadStool");
    assert_eq!(devices[0].vendor, "nvidia");
    assert_eq!(devices[0].arch.as_deref(), Some("sm86"));
    assert_eq!(devices[1].vendor, "amd");
    assert_eq!(devices[1].arch.as_deref(), Some("rdna2"));

    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = f32(gid.x) * 2.0;
}
";
    let nvidia_result = coral_reef::compile_wgsl_raw_sm(wgsl, 86);
    assert!(
        nvidia_result.is_ok(),
        "should compile for discovered sm86: {nvidia_result:?}"
    );
    assert!(
        !nvidia_result.unwrap().is_empty(),
        "compiled binary should be non-empty"
    );
}

/// Verify that discovery ignores non-GPU primals in the directory.
#[test]
fn live_discovery_mixed_primals_only_gpu_resolved() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut xdg = EnvRestore::take("XDG_RUNTIME_DIR");
    let tmp = tempfile::tempdir().expect("tempdir");
    let discovery_dir = tmp.path().join(config::ECOSYSTEM_NAMESPACE);
    std::fs::create_dir_all(&discovery_dir).expect("create discovery dir");

    let toadstool = serde_json::json!({
        "primal": "toadstool",
        "version": "0.2.0",
        "pid": 10001,
        "provides": ["compute.dispatch.submit", "gpu.dispatch"],
        "devices": [{"vendor": "nvidia", "arch": "sm70"}]
    });
    std::fs::write(
        discovery_dir.join("toadstool.json"),
        toadstool.to_string(),
    )
    .expect("write");

    let beardog = serde_json::json!({
        "primal": "beardog",
        "version": "0.9.0",
        "pid": 10002,
        "provides": ["auth.check", "btsp.negotiate", "crypto.sign"],
        "devices": []
    });
    std::fs::write(
        discovery_dir.join("beardog.json"),
        beardog.to_string(),
    )
    .expect("write");

    let nestgate = serde_json::json!({
        "primal": "nestgate",
        "version": "0.1.0",
        "pid": 10003,
        "provides": ["storage.read", "storage.write", "storage.delete"],
        "devices": []
    });
    std::fs::write(
        discovery_dir.join("nestgate.json"),
        nestgate.to_string(),
    )
    .expect("write");

    xdg.set(tmp.path().to_str().expect("utf8"));

    let devices = coralreef_core::discovery::discover_gpu_devices();
    assert_eq!(devices.len(), 1, "only toadStool's GPU device should appear");
    assert_eq!(devices[0].vendor, "nvidia");
    assert_eq!(devices[0].arch.as_deref(), Some("sm70"));
}
