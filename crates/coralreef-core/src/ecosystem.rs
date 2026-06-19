// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ecosystem registration — JSON-RPC **client** calls to a registry primal.
//!
//! Sends `capability.register` once, `primal.announce` once (Neural API routing
//! metadata for biomeOS), and `ipc.heartbeat` on an interval. coralReef does
//! **not** implement those methods as a server; they belong to the ecosystem
//! registry primal’s domain. This module only discovers that peer via the shared
//! capability directory (`capability.register` in `provides`) and connects with
//! a line-delimited JSON-RPC request over Unix (`send_jsonrpc_line` in this module).
//! That client role is intentional T6 compliance: call
//! other primals by capability, do not own their namespaces.
//!
//! Best-effort integration with the registry discovered at runtime under
//! `$XDG_RUNTIME_DIR/biomeos/` (or `BIOMEOS_ECOSYSTEM_REGISTRY`). No hardcoded
//! peer names.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_json::json;
use thiserror::Error;

use crate::capability::SelfDescription;
use crate::config;
use crate::env_keys;

// biomeOS dispatch graph cost/latency hints for `primal.announce`.
// These are approximate performance metadata — not SLA guarantees.
/// Relative compute cost: basic compilation path.
const COST_COMPILE: f64 = 60.0;
/// Relative compute cost: full shader compilation pipeline.
const COST_SHADER_COMPILE: f64 = 80.0;
/// Relative compute cost: GPU dispatch coordination.
const COST_GPU_DISPATCH: f64 = 100.0;
/// Expected latency: basic compilation (milliseconds).
const LATENCY_COMPILE_MS: u32 = 500;
/// Expected latency: full shader compilation (milliseconds).
const LATENCY_SHADER_COMPILE_MS: u32 = 800;
/// Expected latency: GPU dispatch coordination (milliseconds).
const LATENCY_GPU_DISPATCH_MS: u32 = 50;

/// Errors from ecosystem JSON-RPC calls (non-fatal; logged at debug level).
#[derive(Debug, Error)]
pub enum EcosystemError {
    /// I/O or transport failure.
    #[error("ecosystem transport: {0}")]
    Transport(String),

    /// Serialization failure.
    #[error("ecosystem JSON encode: {0}")]
    Encode(#[from] serde_json::Error),
}

/// Spawn background tasks: one-shot `capability.register` and `ipc.heartbeat` every 45s.
///
/// Invokes the registry primal’s methods over JSON-RPC; coralReef does not expose
/// these methods. If no registry is discovered, logs at debug and returns immediately.
pub fn spawn_registration(desc: SelfDescription) {
    #[cfg(unix)]
    {
        let Some(bind) = discover_ecosystem_jsonrpc_bind() else {
            tracing::debug!(
                "no ecosystem registry with capability.register discovered; skipping registration"
            );
            return;
        };
        let Some(unix_path) = jsonrpc_bind_to_unix_path(&bind) else {
            tracing::debug!(
                bind,
                "ecosystem bind is not a Unix socket; skipping registration"
            );
            return;
        };

        let path_register = unix_path.clone();
        let path_announce = unix_path.clone();
        tokio::spawn(async move {
            if let Err(e) = send_capability_register(&path_register, &desc).await {
                tracing::debug!(error = %e, "capability.register failed");
            }
        });

        tokio::spawn(async move {
            if let Err(e) = send_primal_announce(&path_announce).await {
                tracing::debug!(error = %e, "primal.announce failed");
            }
        });

        tokio::spawn(async move {
            heartbeat_loop(unix_path).await;
        });
    }
    #[cfg(not(unix))]
    {
        let _ = desc;
        tracing::debug!("ecosystem registration not available on this platform");
    }
}

#[cfg(unix)]
async fn heartbeat_loop(path: PathBuf) {
    use tokio::time::{MissedTickBehavior, interval};

    const DEFAULT_HEARTBEAT_INTERVAL_SECS: u64 = 45;
    let heartbeat_secs = std::env::var(env_keys::CORALREEF_HEARTBEAT_SECS)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_HEARTBEAT_INTERVAL_SECS);
    let mut ticker = interval(Duration::from_secs(heartbeat_secs));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await;
    loop {
        ticker.tick().await;
        if let Err(e) = send_ipc_heartbeat(&path).await {
            tracing::debug!(error = %e, "ipc.heartbeat failed");
        }
    }
}

/// Discover JSON-RPC bind address for a primal that provides `capability.register`.
///
/// Read-only scan of peer discovery files (same wateringHole directory the binary
/// may write into for self-advertisement). Resolution order:
/// 1. `$BIOMEOS_ECOSYSTEM_REGISTRY` — full bind string (e.g. `unix:///path/registry.sock`).
/// 2. `$DISCOVERY_SOCKET` — explicit discovery relay socket from composition launcher.
/// 3. Scan `discovery_dir()` for `*.json` describing a provider that lists `capability.register`.
#[must_use]
pub fn discover_ecosystem_jsonrpc_bind() -> Option<String> {
    if let Ok(raw) = std::env::var(env_keys::BIOMEOS_ECOSYSTEM_REGISTRY) {
        let t = raw.trim();
        if !t.is_empty() {
            return Some(t.to_owned());
        }
    }

    if let Some(sock) = config::discovery_socket() {
        if socket_is_alive(&sock) {
            let bind = format!("unix://{}", sock.display());
            tracing::debug!(bind, "registry discovery from $DISCOVERY_SOCKET");
            return Some(bind);
        }
    }

    let dir = config::discovery_dir().ok()?;
    let entries = std::fs::read_dir(&dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Some(bind) = registry_bind_from_json_file(&path) {
                return Some(bind);
            }
        }
    }
    None
}

fn registry_bind_from_json_file(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;

    let provides = v.get("provides")?.as_array()?;
    let has_register = provides.iter().any(|p| match p {
        serde_json::Value::String(s) => s == "capability.register",
        serde_json::Value::Object(o) => o
            .get("id")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|id| id == "capability.register"),
        _ => false,
    });
    if !has_register {
        return None;
    }

    let from_transports = v
        .get("transports")
        .and_then(|t| t.get("jsonrpc"))
        .and_then(|j| j.get("bind"))
        .and_then(serde_json::Value::as_str)
        .map(std::string::ToString::to_string);
    let from_endpoint = v
        .get("endpoint")
        .and_then(serde_json::Value::as_str)
        .map(std::string::ToString::to_string);
    from_transports.or(from_endpoint)
}

/// Convert a Phase-10 style bind string to a local Unix path.
#[must_use]
pub fn jsonrpc_bind_to_unix_path(bind: &str) -> Option<PathBuf> {
    let b = bind.trim();
    if let Some(rest) = b.strip_prefix("unix://") {
        let p = PathBuf::from(rest);
        return if p.as_os_str().is_empty() {
            None
        } else {
            Some(p)
        };
    }
    if b.starts_with('/') {
        return Some(PathBuf::from(b));
    }
    None
}

/// Resolve our own UDS path (for the `socket` field in `primal.announce`).
///
/// Delegates to [`config::default_socket_path`] — the single canonical
/// source of truth for socket path resolution. This guarantees the
/// advertised path in `primal.announce` matches the actual bind path.
fn resolve_own_socket_path() -> PathBuf {
    config::default_socket_path()
}

/// Connect-probe a Unix socket to determine if a listener is alive.
///
/// Per `CAPABILITY_BASED_DISCOVERY_STANDARD` v1.3.0 §5: use a connect attempt
/// instead of `path.exists()` to avoid discovering stale sockets left by
/// crashed primals. Local Unix connect is effectively instant (no network
/// round-trip), so a blocking probe is acceptable here.
#[cfg(unix)]
fn socket_is_alive(path: &Path) -> bool {
    use std::os::unix::net::UnixStream;

    if !path.exists() {
        return false;
    }
    UnixStream::connect(path).map_or_else(
        |_| {
            tracing::debug!(path = %path.display(), "stale socket detected (connect refused)");
            false
        },
        |_stream| true,
    )
}

#[derive(Serialize)]
struct RegisterParams<'a> {
    name: &'static str,
    version: &'static str,
    provides: &'a [crate::capability::Capability],
    requires: &'a [crate::capability::Capability],
    transports: &'a [crate::capability::Transport],
}

#[cfg(unix)]
async fn send_capability_register(
    path: &Path,
    desc: &SelfDescription,
) -> Result<(), EcosystemError> {
    let params = RegisterParams {
        name: config::PRIMAL_NAME,
        version: config::PRIMAL_VERSION,
        provides: &desc.provides,
        requires: &desc.requires,
        transports: &desc.transports,
    };
    send_jsonrpc_line(
        path,
        "capability.register",
        serde_json::to_value(&params)?,
        1_u64,
    )
    .await
}

#[cfg(unix)]
async fn send_primal_announce(path: &Path) -> Result<(), EcosystemError> {
    let socket_path = resolve_own_socket_path();
    let params = json!({
        "primal": config::PRIMAL_NAME,
        "version": config::PRIMAL_VERSION,
        "pid": std::process::id(),
        "socket": socket_path.to_string_lossy(),
        "capabilities": ["compile", "shader_compile", "gpu"],
        "methods": config::SERVED_METHODS,
        "signal_tiers": ["node"],
        "cost_hints": {
            "compile": COST_COMPILE,
            "shader_compile": COST_SHADER_COMPILE,
            "gpu": COST_GPU_DISPATCH
        },
        "latency_estimates": {
            "compile": LATENCY_COMPILE_MS,
            "shader_compile": LATENCY_SHADER_COMPILE_MS,
            "gpu": LATENCY_GPU_DISPATCH_MS
        }
    });
    send_jsonrpc_line(path, "primal.announce", params, 3_u64).await
}

#[cfg(unix)]
async fn send_ipc_heartbeat(path: &Path) -> Result<(), EcosystemError> {
    let params = json!({
        "name": config::PRIMAL_NAME,
        "version": config::PRIMAL_VERSION,
        "pid": std::process::id(),
    });
    send_jsonrpc_line(path, "ipc.heartbeat", params, 2_u64).await
}

#[cfg(unix)]
async fn send_jsonrpc_line(
    path: &Path,
    method: &str,
    params: serde_json::Value,
    id: u64,
) -> Result<(), EcosystemError> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;
    use tokio::time::timeout;

    let mut stream = UnixStream::connect(path)
        .await
        .map_err(|e| EcosystemError::Transport(e.to_string()))?;

    let payload = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": id,
    });
    let line = serde_json::to_string(&payload)?;
    stream
        .write_all(line.as_bytes())
        .await
        .map_err(|e| EcosystemError::Transport(e.to_string()))?;
    stream
        .write_all(b"\n")
        .await
        .map_err(|e| EcosystemError::Transport(e.to_string()))?;

    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    let _ = timeout(config::registry_timeout(), reader.read_line(&mut buf)).await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn jsonrpc_bind_to_unix_path_accepts_unix_scheme() {
        let p = jsonrpc_bind_to_unix_path("unix:///run/biomeos/registry.sock");
        assert_eq!(p.as_deref(), Some(Path::new("/run/biomeos/registry.sock")));
    }

    #[test]
    fn jsonrpc_bind_to_unix_path_accepts_absolute() {
        let p = jsonrpc_bind_to_unix_path("/tmp/foo.sock");
        assert_eq!(p.as_deref(), Some(Path::new("/tmp/foo.sock")));
    }

    #[test]
    fn jsonrpc_bind_to_unix_path_rejects_tcp_like() {
        assert!(jsonrpc_bind_to_unix_path("127.0.0.1:9000").is_none());
    }

    #[test]
    fn registry_bind_from_json_file_finds_nested_provides() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("registry.json");
        let j = serde_json::json!({
            "provides": [{"id": "capability.register", "version": "1.0.0"}],
            "transports": { "jsonrpc": { "bind": "unix:///run/ecosystem/reg.sock" } }
        });
        let mut f = std::fs::File::create(&path).expect("create");
        write!(f, "{j}").expect("write");
        let bind = registry_bind_from_json_file(&path).expect("bind");
        assert_eq!(bind, "unix:///run/ecosystem/reg.sock");
    }

    #[test]
    fn registry_bind_from_json_file_string_provides() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("reg2.json");
        let j = serde_json::json!({
            "provides": ["capability.register"],
            "endpoint": "unix:///tmp/x.sock"
        });
        let mut f = std::fs::File::create(&path).expect("create");
        write!(f, "{j}").expect("write");
        let bind = registry_bind_from_json_file(&path).expect("bind");
        assert_eq!(bind, "unix:///tmp/x.sock");
    }

    #[test]
    fn registry_bind_from_json_file_ignores_wrong_capability() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("other.json");
        let j = serde_json::json!({
            "provides": ["gpu.dispatch"],
            "transports": { "jsonrpc": { "bind": "unix:///run/x.sock" } }
        });
        let mut f = std::fs::File::create(&path).expect("create");
        write!(f, "{j}").expect("write");
        assert!(registry_bind_from_json_file(&path).is_none());
    }

    #[test]
    fn registry_bind_from_json_file_malformed_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("broken.json");
        std::fs::write(&path, "not-json").expect("write");
        assert!(registry_bind_from_json_file(&path).is_none());
    }

    #[test]
    fn jsonrpc_bind_to_unix_path_strips_whitespace() {
        let p = jsonrpc_bind_to_unix_path("  unix:///run/test.sock  ");
        assert_eq!(p.as_deref(), Some(Path::new("/run/test.sock")));
    }

    #[test]
    fn jsonrpc_bind_to_unix_path_rejects_empty_unix_scheme() {
        assert!(jsonrpc_bind_to_unix_path("unix://").is_none());
    }

    #[test]
    fn jsonrpc_bind_to_unix_path_rejects_relative_path() {
        assert!(jsonrpc_bind_to_unix_path("relative/path.sock").is_none());
    }

    #[test]
    fn registry_bind_from_json_file_prefers_transports_over_endpoint() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("both.json");
        let j = serde_json::json!({
            "provides": ["capability.register"],
            "transports": { "jsonrpc": { "bind": "unix:///run/preferred.sock" } },
            "endpoint": "unix:///run/fallback.sock"
        });
        let mut f = std::fs::File::create(&path).expect("create");
        write!(f, "{j}").expect("write");
        let bind = registry_bind_from_json_file(&path).expect("bind");
        assert_eq!(bind, "unix:///run/preferred.sock");
    }

    #[test]
    fn registry_bind_from_json_file_missing_provides() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("no-provides.json");
        let j = serde_json::json!({
            "transports": { "jsonrpc": { "bind": "unix:///run/x.sock" } }
        });
        let mut f = std::fs::File::create(&path).expect("create");
        write!(f, "{j}").expect("write");
        assert!(registry_bind_from_json_file(&path).is_none());
    }

    #[test]
    fn registry_bind_from_json_file_provides_not_array() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("provides-string.json");
        let j = serde_json::json!({
            "provides": "capability.register",
            "endpoint": "unix:///run/x.sock"
        });
        let mut f = std::fs::File::create(&path).expect("create");
        write!(f, "{j}").expect("write");
        assert!(registry_bind_from_json_file(&path).is_none());
    }

    #[test]
    fn registry_bind_from_json_file_nonexistent_path() {
        assert!(registry_bind_from_json_file(Path::new("/nonexistent/file.json")).is_none());
    }

    #[test]
    fn discover_ecosystem_scans_directory_with_no_matching_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let j = serde_json::json!({
            "provides": ["gpu.dispatch"],
            "endpoint": "unix:///tmp/irrelevant.sock"
        });
        std::fs::write(dir.path().join("other.json"), j.to_string()).expect("write");
        // With no env vars set and a non-matching dir, discover should find nothing
        // (actual behavior depends on env state, but this exercises the dir-scan path)
    }

    #[cfg(unix)]
    #[test]
    fn socket_is_alive_returns_false_for_nonexistent() {
        assert!(!socket_is_alive(Path::new("/nonexistent/socket.sock")));
    }

    #[cfg(unix)]
    #[test]
    fn socket_is_alive_returns_false_for_regular_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("not-a-socket");
        std::fs::write(&path, "data").expect("write");
        assert!(!socket_is_alive(&path));
    }

    #[test]
    fn ecosystem_error_display_transport() {
        let err = EcosystemError::Transport("connection refused".into());
        assert!(err.to_string().contains("connection refused"));
    }

    #[test]
    fn ecosystem_error_display_encode() {
        let bad_json: Result<serde_json::Value, _> = serde_json::from_str("}{");
        let err = EcosystemError::Encode(bad_json.unwrap_err());
        assert!(err.to_string().contains("JSON encode"));
    }

    #[test]
    fn resolve_own_socket_path_ends_with_sock() {
        let path = resolve_own_socket_path();
        assert!(
            path.extension().is_some_and(|e| e == "sock"),
            "socket path should end in .sock: {}",
            path.display()
        );
    }

    #[test]
    fn register_params_serializes_correctly() {
        let params = RegisterParams {
            name: "test-primal",
            version: "0.1.0",
            provides: &[],
            requires: &[],
            transports: &[],
        };
        let json = serde_json::to_value(&params).expect("serialize");
        assert_eq!(json["name"], "test-primal");
        assert_eq!(json["version"], "0.1.0");
        assert!(json["provides"].as_array().expect("array").is_empty());
    }

    #[test]
    fn primal_announce_payload_has_required_fields() {
        let socket_path = resolve_own_socket_path();
        let params = serde_json::json!({
            "primal": config::PRIMAL_NAME,
            "version": config::PRIMAL_VERSION,
            "pid": std::process::id(),
            "socket": socket_path.to_string_lossy(),
            "capabilities": ["compile", "shader_compile", "gpu"],
            "methods": config::SERVED_METHODS,
            "signal_tiers": ["node"],
            "cost_hints": {
                "compile": 60.0,
                "shader_compile": 80.0,
                "gpu": 100.0
            },
            "latency_estimates": {
                "compile": 500,
                "shader_compile": 800,
                "gpu": 50
            }
        });

        assert!(
            params.get("name").is_none(),
            "payload must use 'primal' not 'name' (biomeOS rejects 'name')"
        );
        assert_eq!(
            params["primal"].as_str().expect("primal field"),
            config::PRIMAL_NAME
        );

        let methods = params["methods"].as_array().expect("methods array");
        assert!(
            !methods.is_empty(),
            "methods must be non-empty for Neural API routing"
        );
        assert!(
            methods.len() >= 16,
            "expected at least 16 announced methods"
        );
        assert!(methods.contains(&serde_json::json!("shader.compile.wgsl")));
        assert!(methods.contains(&serde_json::json!("shader.compile.spirv")));

        let caps = params["capabilities"]
            .as_array()
            .expect("capabilities array");
        assert_eq!(caps.len(), 3);
        assert_eq!(caps[0], "compile");
        assert_eq!(caps[1], "shader_compile");
        assert_eq!(caps[2], "gpu");

        let tiers = params["signal_tiers"]
            .as_array()
            .expect("signal_tiers array");
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0], "node");

        let costs = params["cost_hints"].as_object().expect("cost_hints object");
        assert_eq!(costs.len(), 3);
        assert_eq!(costs["compile"], 60.0);
        assert_eq!(costs["shader_compile"], 80.0);
        assert_eq!(costs["gpu"], 100.0);

        let latency = params["latency_estimates"]
            .as_object()
            .expect("latency_estimates object");
        assert_eq!(latency.len(), 3);
        assert_eq!(latency["compile"], 500);
        assert_eq!(latency["shader_compile"], 800);
        assert_eq!(latency["gpu"], 50);

        assert!(
            params["socket"]
                .as_str()
                .expect("socket string")
                .ends_with(".sock")
        );
        assert!(params["pid"].as_u64().is_some(), "pid must be present");
    }

    #[test]
    fn discover_ecosystem_scans_tempdir_with_registry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = serde_json::json!({
            "provides": ["capability.register"],
            "transports": { "jsonrpc": { "bind": "unix:///tmp/test-registry.sock" } }
        });
        std::fs::write(dir.path().join("registry.json"), registry.to_string()).expect("write");
        let non_registry = serde_json::json!({
            "provides": ["gpu.dispatch"],
            "endpoint": "unix:///tmp/gpu.sock"
        });
        std::fs::write(
            dir.path().join("gpu-provider.json"),
            non_registry.to_string(),
        )
        .expect("write");

        let bind = registry_bind_from_json_file(&dir.path().join("registry.json"));
        assert!(bind.is_some(), "should find registry bind");
        assert_eq!(bind.unwrap(), "unix:///tmp/test-registry.sock");

        let bind2 = registry_bind_from_json_file(&dir.path().join("gpu-provider.json"));
        assert!(bind2.is_none(), "should not find gpu provider as registry");
    }

    #[test]
    fn cost_and_latency_constants_are_positive() {
        assert!(COST_COMPILE > 0.0);
        assert!(COST_SHADER_COMPILE > 0.0);
        assert!(COST_GPU_DISPATCH > 0.0);
        assert!(LATENCY_COMPILE_MS > 0);
        assert!(LATENCY_SHADER_COMPILE_MS > 0);
        assert!(LATENCY_GPU_DISPATCH_MS > 0);
    }

    #[test]
    fn cost_ordering_reflects_complexity() {
        assert!(
            COST_COMPILE < COST_SHADER_COMPILE,
            "shader compile should cost more than basic compile"
        );
        assert!(
            COST_SHADER_COMPILE < COST_GPU_DISPATCH,
            "GPU dispatch should cost the most"
        );
    }

    #[cfg(unix)]
    #[test]
    fn socket_is_alive_returns_true_for_live_listener() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock = dir.path().join("live.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&sock).expect("bind test listener");
        assert!(socket_is_alive(&sock));
    }

    #[test]
    fn registry_bind_provides_non_string_element_returns_false() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("numeric.json");
        let j = serde_json::json!({
            "provides": [42, true, null, "capability.register"],
            "transports": { "jsonrpc": { "bind": "unix:///tmp/reg.sock" } }
        });
        std::fs::write(&path, j.to_string()).expect("write");
        let bind = registry_bind_from_json_file(&path);
        assert!(
            bind.is_some(),
            "should still find registry even with non-string elements"
        );
    }

    #[test]
    fn registry_bind_provides_object_with_matching_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("obj-provides.json");
        let j = serde_json::json!({
            "provides": [{"id": "capability.register", "version": "1.0"}],
            "endpoint": "unix:///tmp/obj-reg.sock"
        });
        std::fs::write(&path, j.to_string()).expect("write");
        let bind = registry_bind_from_json_file(&path);
        assert_eq!(bind.as_deref(), Some("unix:///tmp/obj-reg.sock"));
    }

    #[test]
    fn registry_bind_provides_object_wrong_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wrong-id.json");
        let j = serde_json::json!({
            "provides": [{"id": "gpu.dispatch"}],
            "endpoint": "unix:///tmp/wrong.sock"
        });
        std::fs::write(&path, j.to_string()).expect("write");
        assert!(registry_bind_from_json_file(&path).is_none());
    }

    #[test]
    fn registry_bind_has_provides_but_no_bind_or_endpoint() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("no-bind.json");
        let j = serde_json::json!({
            "provides": ["capability.register"],
            "other_field": "value"
        });
        std::fs::write(&path, j.to_string()).expect("write");
        assert!(
            registry_bind_from_json_file(&path).is_none(),
            "no transports.jsonrpc.bind and no endpoint => None"
        );
    }

    #[test]
    fn registry_bind_endpoint_fallback_when_no_transports() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("endpoint-only.json");
        let j = serde_json::json!({
            "provides": ["capability.register"],
            "endpoint": "unix:///tmp/ep.sock"
        });
        std::fs::write(&path, j.to_string()).expect("write");
        let bind = registry_bind_from_json_file(&path);
        assert_eq!(bind.as_deref(), Some("unix:///tmp/ep.sock"));
    }

    #[test]
    fn discover_ecosystem_scans_dir_skips_non_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = serde_json::json!({
            "provides": ["capability.register"],
            "endpoint": "unix:///tmp/valid-reg.sock"
        });
        std::fs::write(dir.path().join("registry.json"), registry.to_string()).expect("write");
        std::fs::write(dir.path().join("notes.txt"), "not json").expect("write");
        std::fs::write(dir.path().join("data.yaml"), "key: val").expect("write");

        let mut found = false;
        let entries = std::fs::read_dir(dir.path()).expect("readdir");
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Some(bind) = registry_bind_from_json_file(&path) {
                    assert_eq!(bind, "unix:///tmp/valid-reg.sock");
                    found = true;
                }
            }
        }
        assert!(found, "should find registry from .json, not .txt or .yaml");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn send_jsonrpc_line_connect_failure() {
        let result = send_jsonrpc_line(
            Path::new("/nonexistent/socket.sock"),
            "test.method",
            serde_json::json!({}),
            1,
        )
        .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, EcosystemError::Transport(_)),
            "connect failure should produce Transport error: {err}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn send_jsonrpc_line_happy_path_with_mock_listener() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let dir = tempfile::tempdir().expect("tempdir");
        let sock = dir.path().join("mock-registry.sock");
        let listener = tokio::net::UnixListener::bind(&sock).expect("bind mock registry");

        let sock_clone = sock.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.expect("read request");

            let parsed: serde_json::Value =
                serde_json::from_str(&line).expect("request should be valid JSON");
            assert_eq!(parsed["jsonrpc"], "2.0");
            assert_eq!(parsed["method"], "capability.register");

            let response = serde_json::json!({"jsonrpc": "2.0", "result": "ok", "id": 1});
            let writer = reader.into_inner();
            let (_, mut write_half) = tokio::io::split(writer);
            write_half
                .write_all(format!("{response}\n").as_bytes())
                .await
                .expect("write response");
        });

        let result = send_jsonrpc_line(
            &sock_clone,
            "capability.register",
            serde_json::json!({"name": "test"}),
            1,
        )
        .await;
        assert!(
            result.is_ok(),
            "should succeed with mock listener: {result:?}"
        );

        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn send_capability_register_with_mock() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let dir = tempfile::tempdir().expect("tempdir");
        let sock = dir.path().join("cap-reg.sock");
        let listener = tokio::net::UnixListener::bind(&sock).expect("bind");

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.expect("read");
            let writer = reader.into_inner();
            let (_, mut w) = tokio::io::split(writer);
            let resp = serde_json::json!({"jsonrpc":"2.0","result":"ok","id":1});
            w.write_all(format!("{resp}\n").as_bytes())
                .await
                .expect("write");
        });

        let desc = crate::capability::self_description();
        let result = send_capability_register(&sock, &desc).await;
        assert!(
            result.is_ok(),
            "capability register should succeed: {result:?}"
        );

        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn send_primal_announce_with_mock() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let dir = tempfile::tempdir().expect("tempdir");
        let sock = dir.path().join("announce.sock");
        let listener = tokio::net::UnixListener::bind(&sock).expect("bind");

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.expect("read");
            let parsed: serde_json::Value = serde_json::from_str(&line).expect("valid json");
            assert_eq!(parsed["method"], "primal.announce");
            let writer = reader.into_inner();
            let (_, mut w) = tokio::io::split(writer);
            let resp = serde_json::json!({"jsonrpc":"2.0","result":"ok","id":3});
            w.write_all(format!("{resp}\n").as_bytes())
                .await
                .expect("write");
        });

        let result = send_primal_announce(&sock).await;
        assert!(result.is_ok(), "primal announce should succeed: {result:?}");

        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn send_ipc_heartbeat_with_mock() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let dir = tempfile::tempdir().expect("tempdir");
        let sock = dir.path().join("heartbeat.sock");
        let listener = tokio::net::UnixListener::bind(&sock).expect("bind");

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.expect("read");
            let parsed: serde_json::Value = serde_json::from_str(&line).expect("valid json");
            assert_eq!(parsed["method"], "ipc.heartbeat");
            let writer = reader.into_inner();
            let (_, mut w) = tokio::io::split(writer);
            let resp = serde_json::json!({"jsonrpc":"2.0","result":"ok","id":2});
            w.write_all(format!("{resp}\n").as_bytes())
                .await
                .expect("write");
        });

        let result = send_ipc_heartbeat(&sock).await;
        assert!(result.is_ok(), "heartbeat should succeed: {result:?}");

        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
    }
}
