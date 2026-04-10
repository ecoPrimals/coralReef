// SPDX-License-Identifier: AGPL-3.0-or-later
//! BTSP (biomeOS Transport Security Protocol) Phase 2: `BearDog` delegation.
//!
//! Per wateringHole `BTSP_PROTOCOL_STANDARD` v1.0 and `PRIMAL_SELF_KNOWLEDGE_STANDARD`
//! v1.1: when `FAMILY_ID` is set (production mode), every incoming socket connection
//! MUST complete a BTSP handshake before any JSON-RPC methods are exposed.
//!
//! ## Architecture
//!
//! Consumer primals (coralReef) delegate the handshake to the security-domain
//! provider (`BearDog`) via `btsp.session.create` over newline-delimited JSON-RPC
//! on a Unix socket. Discovery is capability-based: we look for a `crypto` domain
//! socket, never hardcoding a primal name.
//!
//! ## Degraded Mode
//!
//! When `FAMILY_ID` is set but the security provider is unreachable or its
//! session layer is incomplete, the guard logs a warning and **accepts** the
//! connection. This prevents a hard dependency on `BearDog` availability during
//! the Phase 2 rollout window.

use std::path::PathBuf;
use std::sync::OnceLock;

use crate::config;

/// Domain stem for security capability discovery.
///
/// Per `PRIMAL_SELF_KNOWLEDGE_STANDARD`: discover peers by capability domain,
/// not primal name. The "crypto" domain owns encryption, signing, and BTSP.
const SECURITY_DOMAIN: &str = "crypto";

/// BTSP operating mode derived from environment at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtspMode {
    /// `FAMILY_ID` is unset or `"default"` — no handshake required.
    Development,
    /// `FAMILY_ID` is set — BTSP handshake mandatory.
    Production {
        /// The active family ID (non-default).
        family_id: String,
    },
}

impl BtspMode {
    /// `true` when the handshake is required on incoming connections.
    #[must_use]
    #[allow(
        dead_code,
        reason = "public API used in tests and future guard_connection evolution"
    )]
    pub const fn requires_handshake(&self) -> bool {
        matches!(self, Self::Production { .. })
    }
}

/// Resolve BTSP mode from environment. Cached after first call.
#[must_use]
pub fn btsp_mode() -> &'static BtspMode {
    static MODE: OnceLock<BtspMode> = OnceLock::new();
    MODE.get_or_init(|| {
        let fid = config::family_id();
        if fid == "default" {
            BtspMode::Development
        } else {
            BtspMode::Production { family_id: fid }
        }
    })
}

/// Result of a BTSP handshake attempt on an incoming connection.
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "fields/variants used via Debug formatting in tracing + future btsp.session.verify"
)]
pub enum BtspOutcome {
    /// No `FAMILY_ID` set — development mode, no handshake required.
    DevMode,
    /// `FAMILY_ID` set, session creation succeeded.
    Authenticated {
        /// Security-provider-issued session identifier.
        session_id: String,
    },
    /// `FAMILY_ID` set, security provider unreachable or session RPC incomplete.
    /// Connection accepted with warning — operators see actionable log.
    Degraded {
        /// Human-readable explanation for monitoring/alerting.
        reason: String,
    },
    /// `FAMILY_ID` set, handshake explicitly failed — connection refused.
    Rejected {
        /// Why the handshake was rejected.
        reason: String,
    },
}

impl BtspOutcome {
    /// Whether the incoming connection should be accepted.
    #[must_use]
    pub const fn should_accept(&self) -> bool {
        matches!(
            self,
            Self::DevMode | Self::Authenticated { .. } | Self::Degraded { .. }
        )
    }
}

/// Per-connection BTSP guard — the primary API for accept loops.
///
/// In `Development` mode, returns immediately (`DevMode`).
/// In `Production` mode, discovers the security provider and attempts
/// `btsp.session.create`. Falls back to `Degraded` if the provider
/// is unavailable.
pub async fn guard_connection() -> BtspOutcome {
    let mode = btsp_mode();
    let family_id = match mode {
        BtspMode::Development => return BtspOutcome::DevMode,
        BtspMode::Production { family_id } => family_id,
    };

    let Some(security_sock) = discover_security_socket(family_id) else {
        let sock_dir = resolve_socket_dir();
        let reason = format!(
            "FAMILY_ID={family_id} but security provider not discoverable at {}. \
             BTSP handshake cannot be enforced — accepting in degraded mode. \
             Deploy a crypto-domain provider to enable BTSP authentication.",
            sock_dir.display()
        );
        tracing::warn!("{reason}");
        return BtspOutcome::Degraded { reason };
    };

    match create_btsp_session(&security_sock, family_id).await {
        Ok(session_id) => {
            tracing::debug!(session_id, "BTSP session created");
            BtspOutcome::Authenticated { session_id }
        }
        Err(e) => {
            let reason = format!(
                "BTSP session creation failed (provider at {}): {e}. \
                 Accepting in degraded mode.",
                security_sock.display()
            );
            tracing::warn!("{reason}");
            BtspOutcome::Degraded { reason }
        }
    }
}

/// Resolve the shared ecosystem socket directory.
fn resolve_socket_dir() -> PathBuf {
    config::discovery_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join(config::ecosystem_namespace()))
}

/// Discover the security-domain socket for BTSP handshake delegation.
///
/// Resolution chain (capability-based, not primal-name-based):
/// 1. `{sock_dir}/{SECURITY_DOMAIN}-{family_id}.sock`
/// 2. `{sock_dir}/{SECURITY_DOMAIN}.sock`
/// 3. Discovery files in `{sock_dir}/*.json` advertising `btsp.session.create`
fn discover_security_socket(family_id: &str) -> Option<PathBuf> {
    let sock_dir = resolve_socket_dir();

    let scoped = sock_dir.join(format!("{SECURITY_DOMAIN}-{family_id}.sock"));
    if scoped.exists() {
        return Some(scoped);
    }

    let unscoped = sock_dir.join(format!("{SECURITY_DOMAIN}.sock"));
    if unscoped.exists() {
        return Some(unscoped);
    }

    discover_by_capability(&sock_dir, "btsp.session.create")
}

/// Scan discovery files for a primal advertising a specific method.
fn discover_by_capability(sock_dir: &std::path::Path, method: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(sock_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json")
            && let Some(sock) = check_discovery_file_for_method(&path, method)
        {
            return Some(sock);
        }
    }
    None
}

/// Check a single discovery file for a primal advertising a given method.
fn check_discovery_file_for_method(path: &std::path::Path, method: &str) -> Option<PathBuf> {
    let content = std::fs::read_to_string(path).ok()?;
    let info: serde_json::Value = serde_json::from_str(&content).ok()?;
    let methods = info.get("methods")?.as_array()?;
    let has_method = methods
        .iter()
        .any(|m| m.as_str().is_some_and(|s| s == method));
    if !has_method {
        return None;
    }
    let unix_addr = info
        .get("transports")
        .and_then(|t| t.get("unix"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.strip_prefix("unix://"))?;
    let sock = PathBuf::from(unix_addr);
    sock.exists().then_some(sock)
}

/// Create a BTSP session via the security provider's `btsp.session.create` RPC.
///
/// Connects over UDS, sends one newline-delimited JSON-RPC request, reads one
/// response line. Returns the session ID on success.
///
/// # Phase 2 Evolution Path
///
/// Currently calls `btsp.session.create` only. The full flow will add:
/// 1. Parse challenge from the session create response
/// 2. Forward challenge to the connecting client over its stream
/// 3. Receive client's X25519 proof
/// 4. Call `btsp.session.verify` with the proof
/// 5. Return cipher parameters for encrypted framing
#[cfg(unix)]
async fn create_btsp_session(
    security_sock: &std::path::Path,
    family_id: &str,
) -> Result<String, BtspSessionError> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let stream = tokio::net::UnixStream::connect(security_sock).await?;
    let (reader, mut writer) = stream.into_split();

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "btsp.session.create",
        "params": { "family_id": family_id },
        "id": 1
    });

    let mut line = serde_json::to_string(&request)?;
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;
    writer.shutdown().await?;

    let mut lines = BufReader::new(reader).lines();
    let response_line = lines
        .next_line()
        .await?
        .ok_or_else(|| BtspSessionError::Protocol("no response from security provider".into()))?;

    let response: serde_json::Value = serde_json::from_str(&response_line)?;

    if let Some(error) = response.get("error") {
        return Err(BtspSessionError::Protocol(format!(
            "btsp.session.create: {error}"
        )));
    }

    response
        .get("result")
        .and_then(|r| r.get("session_id"))
        .and_then(|s| s.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            BtspSessionError::Protocol("missing session_id in btsp.session.create response".into())
        })
}

#[cfg(not(unix))]
async fn create_btsp_session(
    _security_sock: &std::path::Path,
    _family_id: &str,
) -> Result<String, BtspSessionError> {
    Err(BtspSessionError::Protocol(
        "BTSP handshake requires Unix domain sockets".into(),
    ))
}

/// Errors from the BTSP session creation RPC.
#[derive(Debug, thiserror::Error)]
enum BtspSessionError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Protocol(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_mode_allows_all() {
        assert!(!BtspMode::Development.requires_handshake());
    }

    #[test]
    fn production_requires_handshake() {
        let mode = BtspMode::Production {
            family_id: "any".into(),
        };
        assert!(mode.requires_handshake());
    }

    #[test]
    fn btsp_mode_resolves_without_panic() {
        let mode = btsp_mode();
        match mode {
            BtspMode::Development => {}
            BtspMode::Production { family_id } => {
                assert!(!family_id.is_empty());
                assert_ne!(family_id, "default");
            }
        }
    }

    #[test]
    fn outcome_dev_mode_accepts() {
        assert!(BtspOutcome::DevMode.should_accept());
    }

    #[test]
    fn outcome_authenticated_accepts() {
        let o = BtspOutcome::Authenticated {
            session_id: "s-1".into(),
        };
        assert!(o.should_accept());
    }

    #[test]
    fn outcome_degraded_accepts() {
        let o = BtspOutcome::Degraded {
            reason: "provider offline".into(),
        };
        assert!(o.should_accept());
    }

    #[test]
    fn outcome_rejected_refuses() {
        let o = BtspOutcome::Rejected {
            reason: "bad proof".into(),
        };
        assert!(!o.should_accept());
    }

    #[test]
    fn discover_returns_none_when_no_socket() {
        assert!(discover_security_socket("nonexistent-test-family").is_none());
    }

    #[tokio::test]
    async fn guard_connection_dev_mode() {
        if btsp_mode().requires_handshake() {
            return;
        }
        let outcome = guard_connection().await;
        assert!(matches!(outcome, BtspOutcome::DevMode));
    }
}
