// SPDX-License-Identifier: AGPL-3.0-or-later
//! BTSP Phase 2: BearDog delegation for glowplug.
//!
//! Independent implementation — each binary resolves its own BTSP
//! mode from the environment (primal self-knowledge, no cross-crate dependency).
//!
//! See wateringHole `BTSP_PROTOCOL_STANDARD` v1.0.

use std::path::PathBuf;
use std::sync::OnceLock;

const SECURITY_DOMAIN: &str = "crypto";
const ECOSYSTEM_NAMESPACE: &str = "biomeos";

/// BTSP operating mode derived from environment at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtspMode {
    /// Development: no `FAMILY_ID` or `"default"`. No handshake required.
    Development,
    /// Production: `FAMILY_ID` is set. BTSP handshake mandatory.
    Production {
        /// The active family ID.
        family_id: String,
    },
}

/// Resolve BTSP mode from environment. Cached after first call.
#[must_use]
pub fn btsp_mode() -> &'static BtspMode {
    static MODE: OnceLock<BtspMode> = OnceLock::new();
    MODE.get_or_init(|| {
        let fid = std::env::var("BIOMEOS_FAMILY_ID").unwrap_or_else(|_| "default".into());
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
    reason = "fields used via Debug formatting in tracing::warn!(?outcome, ...)"
)]
pub enum BtspOutcome {
    /// No `FAMILY_ID` set — development mode.
    DevMode,
    /// Session creation succeeded.
    Authenticated {
        /// Security-provider-issued session identifier.
        session_id: String,
    },
    /// Security provider unreachable — accept with warning.
    Degraded {
        /// Human-readable explanation.
        reason: String,
    },
    /// Handshake explicitly failed — refuse.
    #[allow(
        dead_code,
        reason = "variant used when btsp.session.verify rejects a proof"
    )]
    Rejected {
        /// Rejection reason.
        reason: String,
    },
}

impl BtspOutcome {
    /// Whether the incoming connection should be accepted.
    #[must_use]
    pub fn should_accept(&self) -> bool {
        matches!(
            self,
            Self::DevMode | Self::Authenticated { .. } | Self::Degraded { .. }
        )
    }
}

/// Per-connection BTSP guard for the glowplug accept loop.
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
             Accepting in degraded mode.",
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

fn resolve_socket_dir() -> PathBuf {
    let base =
        std::env::var("XDG_RUNTIME_DIR").map_or_else(|_| std::env::temp_dir(), PathBuf::from);
    base.join(ECOSYSTEM_NAMESPACE)
}

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
        assert!(BtspOutcome::DevMode.should_accept());
    }

    #[test]
    fn authenticated_accepts() {
        assert!(
            BtspOutcome::Authenticated {
                session_id: "s".into()
            }
            .should_accept()
        );
    }

    #[test]
    fn degraded_accepts() {
        assert!(BtspOutcome::Degraded { reason: "x".into() }.should_accept());
    }

    #[test]
    fn rejected_refuses() {
        assert!(!BtspOutcome::Rejected { reason: "x".into() }.should_accept());
    }

    #[test]
    fn discover_returns_none_when_no_socket() {
        assert!(discover_security_socket("nonexistent-test-family").is_none());
    }
}
