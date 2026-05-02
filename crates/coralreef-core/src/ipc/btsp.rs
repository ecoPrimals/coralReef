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

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

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
    // `dead_code` is not always emitted for this `pub` API; `#[expect(dead_code)]` would be
    // unfulfilled on normal lib builds.
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
// `dead_code` is not consistently emitted for enum variants only referenced via `Debug`;
// `#[expect(dead_code)]` is unfulfilled when rustc omits the lint.
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

/// First byte that indicates plain JSON-RPC (no BTSP handshake expected).
///
/// Per bearDog `ProtocolDetector` convention: a leading `{` means the peer
/// is sending newline-delimited JSON-RPC directly (e.g. biomeOS capability.call
/// forwarding). Any other leading byte triggers BTSP handshake.
const PLAIN_JSONRPC_MARKER: u8 = b'{';

/// BTSP decision from a peeked first byte — the core protocol detection logic.
///
/// Call sites peek the stream using transport-appropriate methods
/// (`TcpStream::peek`, `BufReader::fill_buf`) and pass the result here.
///
/// - `Some(b'{')` → plain JSON-RPC (biomeOS compatibility), BTSP skipped
/// - `Some(_)` → non-JSON first byte, BTSP handshake required
/// - `None` → peek failed/timed out, accept in degraded mode
pub async fn guard_from_first_byte(first_byte: Option<u8>) -> BtspOutcome {
    let mode = btsp_mode();
    if matches!(mode, BtspMode::Development) {
        return BtspOutcome::DevMode;
    }

    match first_byte {
        Some(PLAIN_JSONRPC_MARKER) => {
            tracing::debug!("first byte is '{{' — plain JSON-RPC, BTSP skipped");
            BtspOutcome::DevMode
        }
        Some(b) => {
            tracing::debug!(first_byte = b, "non-JSON first byte — BTSP handshake path");
            guard_connection_inner().await
        }
        None => {
            let reason = "first-byte peek failed or timed out — accepting in degraded mode";
            tracing::warn!("{reason}");
            BtspOutcome::Degraded {
                reason: reason.into(),
            }
        }
    }
}

/// Out-of-band BTSP guard — legacy API for accept loops without stream access.
///
/// Prefer [`guard_from_first_byte`] which inspects the actual stream and avoids
/// BTSP rejection of plain JSON-RPC peers (e.g. biomeOS).
#[allow(
    dead_code,
    reason = "retained for tarpc/HTTP paths that lack stream peek access"
)]
pub async fn guard_connection() -> BtspOutcome {
    guard_connection_inner().await
}

async fn guard_connection_inner() -> BtspOutcome {
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
            register_session(session_id.clone());
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
/// Resolution chain (first match wins):
/// 1. `$BTSP_PROVIDER_SOCKET` — explicit from composition launcher
/// 2. `$BEARDOG_SOCKET` — composition launcher alias
/// 3. `{sock_dir}/{SECURITY_DOMAIN}-{family_id}.sock` — convention scan
/// 4. `{sock_dir}/{SECURITY_DOMAIN}.sock` — unscoped fallback
/// 5. Discovery files in `{sock_dir}/*.json` advertising `btsp.session.create`
fn discover_security_socket(family_id: &str) -> Option<PathBuf> {
    if let Some(path) = config::btsp_provider_socket().filter(|p| p.exists()) {
        tracing::debug!(path = %path.display(), "BTSP provider from $BTSP_PROVIDER_SOCKET");
        return Some(path);
    }

    if let Some(path) = config::beardog_socket().filter(|p| p.exists()) {
        tracing::debug!(path = %path.display(), "BTSP provider from $BEARDOG_SOCKET");
        return Some(path);
    }

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

// ──── Phase 3: `btsp.negotiate` Server Handler ────────────────────────────

/// Global session registry — tracks `session_id`s from successful Phase 2 authentications.
static SESSION_REGISTRY: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn session_registry() -> &'static Mutex<HashSet<String>> {
    SESSION_REGISTRY.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Register a `session_id` from a successful Phase 2 BTSP authentication.
pub fn register_session(session_id: String) {
    if let Ok(mut sessions) = session_registry().lock() {
        sessions.insert(session_id);
    }
}

/// Request payload for `btsp.negotiate`.
#[derive(Debug, serde::Deserialize)]
pub struct NegotiateRequest {
    /// Session ID from a successful Phase 2 handshake.
    pub session_id: String,
    /// Client's preferred cipher suite.
    pub preferred_cipher: String,
    /// Client nonce (base64-encoded, 12+ bytes).
    pub client_nonce: String,
    /// Bond type for this session.
    pub bond_type: String,
}

/// Response payload for `btsp.negotiate`.
#[derive(Debug, serde::Serialize)]
pub struct NegotiateResponse {
    /// Negotiated cipher (`"chacha20-poly1305"` or `"null"` for plaintext fallback).
    pub cipher: String,
    /// Server nonce (base64-encoded).
    pub server_nonce: String,
}

/// Handle the `btsp.negotiate` JSON-RPC method (Phase 3).
///
/// Validates the `session_id` against live sessions, generates a server nonce,
/// and returns the negotiated cipher. Currently returns `"null"` cipher because
/// `coralReef` delegates key material to the crypto-domain provider (`BearDog`) and
/// does not yet have access to the handshake key needed for HKDF derivation.
/// When `BearDog` exposes `btsp.session.key_export`, this upgrades to full AEAD.
///
/// # Errors
///
/// Returns [`NegotiateError`] if `session_id` is invalid or request is malformed.
pub fn handle_negotiate(req: &NegotiateRequest) -> Result<NegotiateResponse, NegotiateError> {
    if req.session_id.is_empty() {
        return Err(NegotiateError::InvalidSession(
            "session_id must not be empty".into(),
        ));
    }

    let session_valid = session_registry()
        .lock()
        .map(|sessions| sessions.contains(&req.session_id))
        .unwrap_or(false);

    if !session_valid {
        let mode = btsp_mode();
        if matches!(mode, BtspMode::Production { .. }) {
            return Err(NegotiateError::InvalidSession(format!(
                "session_id '{}' not found in active sessions",
                req.session_id
            )));
        }
        tracing::debug!(
            session_id = %req.session_id,
            "dev mode: accepting unknown session_id for negotiate"
        );
    }

    if req.client_nonce.is_empty() {
        return Err(NegotiateError::InvalidParams(
            "client_nonce must not be empty".into(),
        ));
    }

    let nonce_bytes: [u8; 24] = rand::random();
    let server_nonce = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, nonce_bytes);

    tracing::debug!(
        session_id = %req.session_id,
        preferred_cipher = %req.preferred_cipher,
        bond_type = %req.bond_type,
        "btsp.negotiate: returning null cipher (key export not yet available)"
    );

    Ok(NegotiateResponse {
        cipher: "null".into(),
        server_nonce,
    })
}

/// Errors from `btsp.negotiate` handler.
#[derive(Debug, thiserror::Error)]
pub enum NegotiateError {
    /// Session ID not found or not valid.
    #[error("invalid session: {0}")]
    InvalidSession(String),
    /// Request parameters malformed.
    #[error("invalid params: {0}")]
    InvalidParams(String),
}

impl NegotiateError {
    /// JSON-RPC error code for this variant.
    #[must_use]
    #[allow(dead_code, reason = "public API for downstream error formatting evolution")]
    pub const fn jsonrpc_code(&self) -> i64 {
        match self {
            Self::InvalidSession(_) | Self::InvalidParams(_) => -32_602,
        }
    }
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

    #[tokio::test]
    async fn guard_from_first_byte_json_marker_skips_btsp() {
        let outcome = guard_from_first_byte(Some(b'{')).await;
        assert!(outcome.should_accept());
    }

    #[tokio::test]
    async fn guard_from_first_byte_none_degrades() {
        if !btsp_mode().requires_handshake() {
            return;
        }
        let outcome = guard_from_first_byte(None).await;
        assert!(outcome.should_accept());
        assert!(matches!(outcome, BtspOutcome::Degraded { .. }));
    }

    // ─── Phase 3: btsp.negotiate tests ───────────────────────────────

    #[test]
    fn negotiate_empty_session_id_rejected() {
        let req = NegotiateRequest {
            session_id: String::new(),
            preferred_cipher: "chacha20-poly1305".into(),
            client_nonce: "dGVzdG5vbmNlMTIzNDU2".into(),
            bond_type: "Covalent".into(),
        };
        let err = handle_negotiate(&req).unwrap_err();
        assert!(matches!(err, NegotiateError::InvalidSession(_)));
    }

    #[test]
    fn negotiate_empty_client_nonce_rejected() {
        register_session("test-session-1".into());
        let req = NegotiateRequest {
            session_id: "test-session-1".into(),
            preferred_cipher: "chacha20-poly1305".into(),
            client_nonce: String::new(),
            bond_type: "Covalent".into(),
        };
        let err = handle_negotiate(&req).unwrap_err();
        assert!(matches!(err, NegotiateError::InvalidParams(_)));
    }

    #[test]
    fn negotiate_valid_session_returns_null_cipher() {
        register_session("test-session-2".into());
        let req = NegotiateRequest {
            session_id: "test-session-2".into(),
            preferred_cipher: "chacha20-poly1305".into(),
            client_nonce: "dGVzdG5vbmNlMTIzNDU2".into(),
            bond_type: "Covalent".into(),
        };
        let resp = handle_negotiate(&req).unwrap();
        assert_eq!(resp.cipher, "null");
        assert!(!resp.server_nonce.is_empty());
    }

    #[test]
    fn negotiate_server_nonce_is_valid_base64() {
        register_session("test-session-3".into());
        let req = NegotiateRequest {
            session_id: "test-session-3".into(),
            preferred_cipher: "chacha20-poly1305".into(),
            client_nonce: "dGVzdG5vbmNlMTIzNDU2".into(),
            bond_type: "Covalent".into(),
        };
        let resp = handle_negotiate(&req).unwrap();
        let decoded = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &resp.server_nonce,
        )
        .expect("server_nonce should be valid base64");
        assert_eq!(decoded.len(), 24);
    }

    #[test]
    fn negotiate_dev_mode_accepts_unknown_session() {
        if btsp_mode().requires_handshake() {
            return;
        }
        let req = NegotiateRequest {
            session_id: "nonexistent-session-xyz".into(),
            preferred_cipher: "chacha20-poly1305".into(),
            client_nonce: "dGVzdG5vbmNlMTIzNDU2".into(),
            bond_type: "Ionic".into(),
        };
        let resp = handle_negotiate(&req).unwrap();
        assert_eq!(resp.cipher, "null");
    }

    #[test]
    fn session_registry_tracks_multiple_sessions() {
        register_session("sess-a".into());
        register_session("sess-b".into());
        let sessions = session_registry().lock().unwrap();
        assert!(sessions.contains("sess-a"));
        assert!(sessions.contains("sess-b"));
    }
}
