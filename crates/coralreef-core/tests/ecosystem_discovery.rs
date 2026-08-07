// SPDX-License-Identifier: AGPL-3.0-or-later
//! Integration tests for ecosystem discovery and registration.
//!
//! Environment mutation uses the shared [`test_env::EnvGuard`] helper (Rust 1.85+
//! marks `env::set_var`/`env::remove_var` as `unsafe`). The `coralreef-core`
//! library crate forbids `unsafe_code`, so env tests live in the integration
//! test crate behind a process-wide lock.

#[path = "test_env.rs"]
mod test_env;

use std::io::Write;

use coralreef_core::config;
use coralreef_core::ecosystem::{discover_ecosystem_jsonrpc_bind, spawn_registration};
use test_env::{ENV_LOCK, EnvGuard};

#[test]
fn discover_ecosystem_jsonrpc_bind_prefers_biomeos_registry_trimmed() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut bio = EnvGuard::capture("BIOMEOS_ECOSYSTEM_REGISTRY");
    bio.set("  unix:///tmp/registry-trimmed.sock  ");
    let got = discover_ecosystem_jsonrpc_bind();
    assert_eq!(got.as_deref(), Some("unix:///tmp/registry-trimmed.sock"));
}

#[test]
fn discover_ecosystem_jsonrpc_bind_biomeos_whitespace_only_falls_back_to_scan() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut bio = EnvGuard::capture("BIOMEOS_ECOSYSTEM_REGISTRY");
    let mut xdg = EnvGuard::capture("XDG_RUNTIME_DIR");
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
    let mut bio = EnvGuard::capture("BIOMEOS_ECOSYSTEM_REGISTRY");
    let mut xdg = EnvGuard::capture("XDG_RUNTIME_DIR");
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
    let mut bio = EnvGuard::capture("BIOMEOS_ECOSYSTEM_REGISTRY");
    let mut xdg = EnvGuard::capture("XDG_RUNTIME_DIR");
    let tmp = tempfile::tempdir().expect("tempdir");
    let biomeos = tmp.path().join(config::ECOSYSTEM_NAMESPACE);
    std::fs::create_dir_all(&biomeos).expect("create_dir_all");
    std::fs::write(biomeos.join("bad.json"), "{ not valid json").expect("write");

    bio.remove();
    xdg.set(tmp.path().to_str().expect("utf8 path"));
    let got = discover_ecosystem_jsonrpc_bind();
    assert!(got.is_none());
}

#[tokio::test]
async fn spawn_registration_no_registry_returns_without_panic() {
    let tmp = tempfile::tempdir().expect("tempdir");
    {
        let _guard = ENV_LOCK.lock().unwrap();
        let mut bio = EnvGuard::capture("BIOMEOS_ECOSYSTEM_REGISTRY");
        let mut xdg = EnvGuard::capture("XDG_RUNTIME_DIR");
        let biomeos = tmp.path().join(config::ECOSYSTEM_NAMESPACE);
        std::fs::create_dir_all(&biomeos).expect("create_dir_all");

        bio.remove();
        xdg.set(tmp.path().to_str().expect("utf8 path"));
        spawn_registration(coralreef_core::capability::self_description());
    }
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
}

#[tokio::test]
async fn spawn_registration_tcp_bind_attempts_connection() {
    {
        let _guard = ENV_LOCK.lock().unwrap();
        let mut bio = EnvGuard::capture("BIOMEOS_ECOSYSTEM_REGISTRY");
        bio.set("127.0.0.1:65530");
        spawn_registration(coralreef_core::capability::self_description());
    }
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
}

/// Simulate the full compute-dispatch → coralReef discovery pipeline:
/// 1. Compute-dispatch provider publishes a discovery JSON advertising GPU capabilities
/// 2. coralReef discovers the GPU target from the shared directory
/// 3. coralReef compiles a shader for the discovered architecture
///
/// This validates the "node atomic" pattern: discover by capability, not name.
#[test]
fn live_discovery_compute_dispatch_gpu_target_to_compile() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut xdg = EnvGuard::capture("XDG_RUNTIME_DIR");
    let tmp = tempfile::tempdir().expect("tempdir");
    let discovery_dir = tmp.path().join(config::ECOSYSTEM_NAMESPACE);
    std::fs::create_dir_all(&discovery_dir).expect("create discovery dir");

    let dispatch_entry = serde_json::json!({
        "primal": "compute-dispatch",
        "version": "0.2.0",
        "pid": 99999,
        "provides": [
            {"id": "compute.dispatch.submit", "version": "0.1.0"},
            {"id": "compute.dispatch.capabilities", "version": "0.1.0"},
            {"id": "gpu.dispatch", "version": "0.1.0"}
        ],
        "transports": {
            "jsonrpc": {"bind": "unix:///run/user/1000/biomeos/compute-dispatch.sock"}
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
    let path = discovery_dir.join("compute-dispatch.json");
    std::fs::write(&path, dispatch_entry.to_string()).expect("write discovery");

    xdg.set(tmp.path().to_str().expect("utf8"));

    let devices = coralreef_core::discovery::discover_gpu_devices();
    assert_eq!(
        devices.len(),
        2,
        "should discover 2 GPU devices from compute-dispatch provider"
    );
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
    let mut xdg = EnvGuard::capture("XDG_RUNTIME_DIR");
    let tmp = tempfile::tempdir().expect("tempdir");
    let discovery_dir = tmp.path().join(config::ECOSYSTEM_NAMESPACE);
    std::fs::create_dir_all(&discovery_dir).expect("create discovery dir");

    let compute_dispatch = serde_json::json!({
        "primal": "compute-dispatch",
        "version": "0.2.0",
        "pid": 10001,
        "provides": ["compute.dispatch.submit", "gpu.dispatch"],
        "devices": [{"vendor": "nvidia", "arch": "sm70"}]
    });
    std::fs::write(
        discovery_dir.join("compute-dispatch.json"),
        compute_dispatch.to_string(),
    )
    .expect("write");

    let security_provider = serde_json::json!({
        "primal": "security-provider",
        "version": "0.9.0",
        "pid": 10002,
        "provides": ["auth.check", "btsp.negotiate", "crypto.sign"],
        "devices": []
    });
    std::fs::write(
        discovery_dir.join("security-provider.json"),
        security_provider.to_string(),
    )
    .expect("write");

    let storage_provider = serde_json::json!({
        "primal": "storage-provider",
        "version": "0.1.0",
        "pid": 10003,
        "provides": ["storage.read", "storage.write", "storage.delete"],
        "devices": []
    });
    std::fs::write(
        discovery_dir.join("storage-provider.json"),
        storage_provider.to_string(),
    )
    .expect("write");

    xdg.set(tmp.path().to_str().expect("utf8"));

    let devices = coralreef_core::discovery::discover_gpu_devices();
    assert_eq!(
        devices.len(),
        1,
        "only the compute-dispatch provider's GPU device should appear"
    );
    assert_eq!(devices[0].vendor, "nvidia");
    assert_eq!(devices[0].arch.as_deref(), Some("sm70"));
}
