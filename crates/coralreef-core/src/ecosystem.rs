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

    let heartbeat_secs = std::env::var(env_keys::CORALREEF_HEARTBEAT_SECS)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(45u64);
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
/// 2. `$DISCOVERY_SOCKET` — explicit Songbird socket from composition launcher.
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
        "methods": crate::service::SERVED_METHODS,
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
    fn primal_announce_payload_has_required_fields() {
        let socket_path = resolve_own_socket_path();
        let params = serde_json::json!({
            "primal": config::PRIMAL_NAME,
            "version": config::PRIMAL_VERSION,
            "pid": std::process::id(),
            "socket": socket_path.to_string_lossy(),
            "capabilities": ["compile", "shader_compile", "gpu"],
            "methods": crate::service::SERVED_METHODS,
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
}
