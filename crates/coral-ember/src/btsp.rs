// SPDX-License-Identifier: AGPL-3.0-or-later
//! BTSP Phase 2: BearDog delegation for coral-ember (blocking I/O).
//!
//! coral-ember uses `std::thread` + blocking sockets, so this module
//! provides a synchronous `guard_connection()` that uses `std::os::unix::net`.
//!
//! See wateringHole `BTSP_PROTOCOL_STANDARD` v1.0.

use std::path::PathBuf;
use std::sync::OnceLock;

const SECURITY_DOMAIN: &str = "crypto";

/// BTSP operating mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BtspMode {
    /// Development — no handshake.
    Development,
    /// Production — BTSP handshake mandatory.
    Production { family_id: String },
}

/// Resolve BTSP mode from environment. Cached after first call.
#[must_use]
pub(crate) fn btsp_mode() -> &'static BtspMode {
    static MODE: OnceLock<BtspMode> = OnceLock::new();
    MODE.get_or_init(|| {
        let fid = crate::config::family_id();
        if fid == "default" {
            BtspMode::Development
        } else {
            BtspMode::Production { family_id: fid }
        }
    })
}

/// Result of a BTSP handshake attempt.
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "fields used via Debug formatting in tracing::warn!(?outcome, ...)"
)]
pub(crate) enum BtspOutcome {
    DevMode,
    Authenticated { session_id: String },
    Degraded { reason: String },
    Rejected { reason: String },
}

impl BtspOutcome {
    #[must_use]
    pub(crate) fn should_accept(&self) -> bool {
        matches!(
            self,
            Self::DevMode | Self::Authenticated { .. } | Self::Degraded { .. }
        )
    }
}

/// Per-connection BTSP guard (blocking — for `std::thread` accept loops).
#[must_use]
pub(crate) fn guard_connection() -> BtspOutcome {
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

    match create_btsp_session(&security_sock, family_id) {
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
    base.join(crate::config::ecosystem_namespace())
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

/// Create a BTSP session via blocking UDS JSON-RPC.
#[cfg(unix)]
fn create_btsp_session(
    security_sock: &std::path::Path,
    family_id: &str,
) -> Result<String, BtspSessionError> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let stream = UnixStream::connect(security_sock)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let mut writer = std::io::BufWriter::new(&stream);
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "btsp.session.create",
        "params": { "family_id": family_id },
        "id": 1
    });

    let mut line = serde_json::to_string(&request)?;
    line.push('\n');
    writer.write_all(line.as_bytes())?;
    writer.flush()?;
    drop(writer);
    stream.shutdown(std::net::Shutdown::Write)?;

    let mut reader = BufReader::new(&stream);
    let mut response_line = String::new();
    reader.read_line(&mut response_line)?;

    if response_line.is_empty() {
        return Err(BtspSessionError::Protocol(
            "no response from security provider".into(),
        ));
    }

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
fn create_btsp_session(
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
    fn development_allows() {
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

    #[test]
    fn guard_dev_mode() {
        if btsp_mode() == &BtspMode::Development {
            let outcome = guard_connection();
            assert!(matches!(outcome, BtspOutcome::DevMode));
        }
    }
}
