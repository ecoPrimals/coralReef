// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ecosystem registration — JSON-RPC **client** calls to a registry primal.
//!
//! Sends `capability.register` once, `primal.announce` once (Neural API routing
//! metadata for biomeOS), and `ipc.heartbeat` on an interval. coralReef does
//! **not** implement those methods as a server; they belong to the ecosystem
//! registry primal's domain. This module discovers that peer via the shared
//! capability directory (`capability.register` in `provides`) and connects via
//! the G66 transport layer — UDS on Unix, TCP when a TCP bind is discovered.
//! That client role is intentional T6 compliance: call
//! other primals by capability, do not own their namespaces.
//!
//! **G68 evolution (Wave 157a)**: all IPC paths go through
//! [`TransportEndpoint`] → [`connect_transport`]. No `#[cfg(unix)]` gates
//! remain; non-Unix platforms work whenever a TCP registry is available.
//!
//! Best-effort integration with the registry discovered at runtime under
//! `$XDG_RUNTIME_DIR/biomeos/` (or `BIOMEOS_ECOSYSTEM_REGISTRY`). No hardcoded
//! peer names.

use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

use crate::capability::SelfDescription;
use crate::config;
use crate::env_keys;
use crate::transport::TransportEndpoint;

mod announce_hints {
    pub const COST_COMPILE: f64 = 60.0;
    pub const COST_SHADER_COMPILE: f64 = 80.0;
    pub const COST_GPU_DISPATCH: f64 = 100.0;
    pub const LATENCY_COMPILE_MS: u32 = 500;
    pub const LATENCY_SHADER_COMPILE_MS: u32 = 800;
    pub const LATENCY_GPU_DISPATCH_MS: u32 = 50;
}
use announce_hints::{
    COST_COMPILE, COST_GPU_DISPATCH, COST_SHADER_COMPILE, LATENCY_COMPILE_MS,
    LATENCY_GPU_DISPATCH_MS, LATENCY_SHADER_COMPILE_MS,
};

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
/// Invokes the registry primal's methods over JSON-RPC; coralReef does not expose
/// these methods. If no registry is discovered, logs at debug and returns immediately.
///
/// **G68**: works on all platforms — UDS when the bind is `unix://…`, TCP when
/// the bind is `host:port`.
#[allow(
    clippy::needless_pass_by_value,
    reason = "desc is moved into tokio::spawn; by-value signature required"
)]
pub fn spawn_registration(desc: SelfDescription) {
    let Some(bind) = discover_ecosystem_jsonrpc_bind() else {
        tracing::debug!(
            "no ecosystem registry with capability.register discovered; skipping registration"
        );
        return;
    };
    let Some(endpoint) = parse_bind_to_endpoint(&bind) else {
        tracing::debug!(
            bind,
            "ecosystem bind string is not a recognised transport; skipping registration"
        );
        return;
    };

    let ep_register = endpoint.clone();
    let ep_announce = endpoint.clone();
    tokio::spawn(async move {
        if let Err(e) = send_capability_register(&ep_register, &desc).await {
            tracing::debug!(error = %e, "capability.register failed");
        }
    });

    tokio::spawn(async move {
        if let Err(e) = send_primal_announce(&ep_announce).await {
            tracing::debug!(error = %e, "primal.announce failed");
        }
    });

    tokio::spawn(async move {
        heartbeat_loop(endpoint).await;
    });
}

async fn heartbeat_loop(endpoint: TransportEndpoint) {
    use std::time::Duration;
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
        if let Err(e) = send_ipc_heartbeat(&endpoint).await {
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

/// Parse a Phase-10 / ecosystem bind string into a transport endpoint.
///
/// Delegates to [`TransportEndpoint::from_bind_string`] — the canonical
/// parser lives in the G66 transport layer so it's accessible from both
/// lib and bin targets.
#[must_use]
pub fn parse_bind_to_endpoint(bind: &str) -> Option<TransportEndpoint> {
    TransportEndpoint::from_bind_string(bind)
}

/// Convert a Phase-10 style bind string to a local Unix path.
///
/// **Deprecated in favour of [`parse_bind_to_endpoint`]** which handles both
/// UDS and TCP binds. Retained for backward compatibility with callers that
/// require a filesystem path.
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

/// Connect-probe to determine if a listener is alive at `path`.
///
/// Per `CAPABILITY_BASED_DISCOVERY_STANDARD` v1.3.0 §5: use a connect attempt
/// instead of `path.exists()` to avoid discovering stale sockets left by
/// crashed primals.
fn socket_is_alive(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    crate::local_transport::connect_local_sync(path).map_or_else(
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

async fn send_capability_register(
    endpoint: &TransportEndpoint,
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
        endpoint,
        "capability.register",
        serde_json::to_value(&params)?,
        1_u64,
    )
    .await
}

async fn send_primal_announce(endpoint: &TransportEndpoint) -> Result<(), EcosystemError> {
    use serde_json::json;
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
    send_jsonrpc_line(endpoint, "primal.announce", params, 3_u64).await
}

async fn send_ipc_heartbeat(endpoint: &TransportEndpoint) -> Result<(), EcosystemError> {
    use serde_json::json;
    let params = json!({
        "name": config::PRIMAL_NAME,
        "version": config::PRIMAL_VERSION,
        "pid": std::process::id(),
    });
    send_jsonrpc_line(endpoint, "ipc.heartbeat", params, 2_u64).await
}

/// Send a single line-delimited JSON-RPC request to a registry endpoint.
///
/// **G68**: connects via [`crate::transport::connect_transport`] which
/// handles UDS, TCP, or any future transport backend.
async fn send_jsonrpc_line(
    endpoint: &TransportEndpoint,
    method: &str,
    params: serde_json::Value,
    id: u64,
) -> Result<(), EcosystemError> {
    use serde_json::json;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::time::timeout;

    let mut stream = crate::transport::connect_transport(endpoint)
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
    mod tests_ecosystem;
}
