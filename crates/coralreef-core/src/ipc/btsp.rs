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

use super::btsp_negotiate::register_session;
use crate::config;
#[cfg(unix)]
use crate::env_keys;

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

    /// The session ID from a successful Phase 2 authentication, if any.
    #[must_use]
    #[allow(
        dead_code,
        reason = "public API for Phase 3 encrypted transport session binding"
    )]
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::Authenticated { session_id } => Some(session_id),
            _ => None,
        }
    }
}

/// First byte that indicates plain JSON-RPC (no BTSP handshake expected).
///
/// Per bearDog `ProtocolDetector` convention: a leading `{` means the peer
/// is sending newline-delimited JSON-RPC directly (e.g. biomeOS capability.call
/// forwarding). Any other leading byte triggers BTSP handshake.
const PLAIN_JSONRPC_MARKER: u8 = b'{';

/// `true` when a newline‑terminated first line is a JSON BTSP `ClientHello`
/// (wateringHole JSON‑line form), as opposed to plain JSON‑RPC 2.0.
#[must_use]
#[allow(
    clippy::redundant_pub_crate,
    reason = "crate-visible helper shared by ipc submodules and service/provenance"
)]
pub(crate) fn line_looks_like_btsp_client_hello(line: &str) -> bool {
    line.contains("\"protocol\"") && line.contains("\"btsp\"")
}

/// BTSP decision from a peeked first byte — the core protocol detection logic.
///
/// Call sites peek the stream using transport-appropriate methods
/// (`TcpStream::peek`, `BufReader::fill_buf`) and pass the result here.
///
/// - `Some(b'{')` → **ambiguous** — the caller should read the first line and
///   use [`guard_from_first_line_after_brace`]; or treat as plain JSON for
///   legacy callers that only have the first byte
/// - `Some(_)` → non-JSON first byte, BTSP handshake required
/// - `None` → peek failed/timed out, accept in degraded mode
pub async fn guard_from_first_byte(first_byte: Option<u8>) -> BtspOutcome {
    let mode = btsp_mode();
    if matches!(mode, BtspMode::Development) {
        return BtspOutcome::DevMode;
    }

    match first_byte {
        Some(PLAIN_JSONRPC_MARKER) => {
            tracing::debug!("first byte is '{{' (no first line) — plain JSON-RPC, BTSP skipped");
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

/// After the first byte was `{` and the full first line was read, decide BTSP vs plain JSON.
///
/// If the line contains both `"protocol"` and `"btsp"`, the peer is using JSON-line BTSP
/// `ClientHello` and we run the same handshake path as a non-`{` first byte. Otherwise
/// the line is treated as the start of JSON-RPC and BTSP is skipped.
pub async fn guard_from_first_line_after_brace(first_line: &str) -> BtspOutcome {
    let mode = btsp_mode();
    if matches!(mode, BtspMode::Development) {
        return BtspOutcome::DevMode;
    }

    if line_looks_like_btsp_client_hello(first_line) {
        tracing::debug!("first line is JSON BTSP ClientHello — BTSP handshake path");
        guard_connection_inner().await
    } else {
        tracing::debug!("first line is plain JSON-RPC, BTSP skipped");
        BtspOutcome::DevMode
    }
}

/// Full JSON-line BTSP handshake relay on the client's stream.
///
/// Performs the complete 4-step handshake: reads the already-consumed `ClientHello`,
/// relays through `BearDog` `btsp.session.create` / `btsp.session.verify`, and
/// writes `ServerHello` + `HandshakeComplete` back to the client. Returns `Ok(session_id)`
/// on success or `Err` on failure (caller should close the connection).
///
/// # Errors
///
/// Returns an error when BTSP is disabled, the security provider is unreachable,
/// handshake messages are malformed, or family verification fails.
#[cfg(unix)]
pub async fn relay_json_line_handshake<W, R>(
    client_hello_line: &str,
    reader: &mut R,
    writer: &mut W,
) -> Result<String, BtspSessionError>
where
    W: tokio::io::AsyncWrite + Unpin,
    R: tokio::io::AsyncBufRead + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let mode = btsp_mode();
    let family_id = match mode {
        BtspMode::Development => {
            return Err(BtspSessionError::Protocol("dev mode, no BTSP".into()));
        }
        BtspMode::Production { family_id } => family_id.clone(),
    };

    let Some(security_sock) = discover_security_socket(&family_id) else {
        return Err(BtspSessionError::Protocol(
            "security provider not discoverable".into(),
        ));
    };

    let client_hello: serde_json::Value = serde_json::from_str(client_hello_line.trim())?;
    let client_ephemeral_pub = client_hello
        .get("client_ephemeral_pub")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            BtspSessionError::Protocol("ClientHello missing client_ephemeral_pub".into())
        })?;

    let raw_seed = std::env::var(env_keys::FAMILY_SEED)
        .or_else(|_| std::env::var(env_keys::BTSP_FAMILY_SEED))
        .unwrap_or_default();
    let family_seed = b64_encode(raw_seed.as_bytes());

    let create_result = security_rpc(
        &security_sock,
        "btsp.session.create",
        serde_json::json!({
            "family_seed": family_seed,
        }),
    )
    .await?;

    let session_id = create_result
        .get("session_id")
        .or_else(|| create_result.get("session_token"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| BtspSessionError::Protocol("missing session_id in create response".into()))?
        .to_string();

    let server_ephemeral_pub = create_result
        .get("server_ephemeral_pub")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let challenge_b64 = create_result
        .get("challenge")
        .and_then(|v| v.as_str())
        .ok_or_else(|| BtspSessionError::Protocol("missing challenge in create response".into()))?;

    let server_hello = serde_json::json!({
        "version": 1,
        "server_ephemeral_pub": server_ephemeral_pub,
        "challenge": challenge_b64,
    });
    let mut sh_line = serde_json::to_string(&server_hello)?;
    sh_line.push('\n');
    writer
        .write_all(sh_line.as_bytes())
        .await
        .map_err(|e| BtspSessionError::Protocol(format!("write ServerHello: {e}")))?;
    writer
        .flush()
        .await
        .map_err(|e| BtspSessionError::Protocol(format!("flush ServerHello: {e}")))?;

    let mut cr_line = String::new();
    reader
        .read_line(&mut cr_line)
        .await
        .map_err(|e| BtspSessionError::Protocol(format!("read ChallengeResponse: {e}")))?;
    let cr: serde_json::Value = serde_json::from_str(cr_line.trim())
        .map_err(|e| BtspSessionError::Protocol(format!("parse ChallengeResponse: {e}")))?;
    let hmac_response = cr.get("response").and_then(|v| v.as_str()).unwrap_or("");

    let verify_result = security_rpc(
        &security_sock,
        "btsp.session.verify",
        serde_json::json!({
            "session_token": session_id,
            "response": hmac_response,
            "client_ephemeral_pub": client_ephemeral_pub,
            "server_ephemeral_pub": server_ephemeral_pub,
            "challenge": challenge_b64,
        }),
    )
    .await?;

    let verified = verify_result
        .get("verified")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !verified {
        let err = serde_json::json!({
            "error": "handshake_failed",
            "reason": "family_verification",
        });
        let mut err_line = serde_json::to_string(&err)?;
        err_line.push('\n');
        let _ = writer.write_all(err_line.as_bytes()).await;
        return Err(BtspSessionError::Protocol("verification failed".into()));
    }

    let complete = serde_json::json!({
        "cipher": "BTSP_NULL",
        "session_id": session_id,
    });
    let mut cmp_line = serde_json::to_string(&complete)?;
    cmp_line.push('\n');
    writer
        .write_all(cmp_line.as_bytes())
        .await
        .map_err(|e| BtspSessionError::Protocol(format!("write HandshakeComplete: {e}")))?;
    writer
        .flush()
        .await
        .map_err(|e| BtspSessionError::Protocol(format!("flush HandshakeComplete: {e}")))?;

    tracing::info!(session_id, "BTSP JSON-line handshake complete");
    Ok(session_id)
}

#[cfg(unix)]
async fn security_rpc(
    security_sock: &std::path::Path,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, BtspSessionError> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let stream = tokio::net::UnixStream::connect(security_sock).await?;
    let (reader, mut writer) = stream.into_split();

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
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
        .ok_or_else(|| BtspSessionError::Protocol("no response from provider".into()))?;
    let response: serde_json::Value = serde_json::from_str(&response_line)?;
    if let Some(error) = response.get("error") {
        return Err(BtspSessionError::Protocol(format!("{method}: {error}")));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| BtspSessionError::Protocol(format!("{method}: missing result")))
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
        Ok((session_id, handshake_key)) => {
            tracing::debug!(
                session_id,
                has_key = handshake_key.is_some(),
                "BTSP session created"
            );
            register_session(session_id.clone(), handshake_key);
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
#[allow(
    clippy::redundant_pub_crate,
    reason = "crate-visible discovery helper shared by btsp handshake and service/provenance"
)]
pub(crate) fn discover_security_socket(family_id: &str) -> Option<PathBuf> {
    if let Some(path) = config::btsp_provider_socket().filter(|p| p.exists()) {
        tracing::debug!(path = %path.display(), "BTSP provider from $BTSP_PROVIDER_SOCKET");
        return Some(path);
    }

    if let Some(path) = config::security_provider_legacy_socket().filter(|p| p.exists()) {
        tracing::debug!(path = %path.display(), "BTSP provider from $BEARDOG_SOCKET (legacy alias)");
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
#[allow(
    clippy::redundant_pub_crate,
    reason = "crate-visible discovery helper shared by btsp handshake and service/provenance"
)]
pub(crate) fn discover_by_capability(sock_dir: &std::path::Path, method: &str) -> Option<PathBuf> {
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
/// response line. Returns `(session_id, Option<handshake_key>)`.
///
/// Per `BTSP_PROTOCOL_STANDARD` v1.0, the `btsp.session.create` response includes
/// a base64-encoded 32-byte `handshake_key`. When present, this key enables real
/// AEAD in Phase 3 negotiation. When absent (older provider), Phase 3 falls back
/// to null cipher.
#[cfg(unix)]
async fn create_btsp_session(
    security_sock: &std::path::Path,
    family_id: &str,
) -> Result<(String, Option<[u8; 32]>), BtspSessionError> {
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

    let result = response.get("result").ok_or_else(|| {
        BtspSessionError::Protocol("missing result in btsp.session.create response".into())
    })?;

    let session_id = result
        .get("session_id")
        .and_then(|s| s.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            BtspSessionError::Protocol("missing session_id in btsp.session.create response".into())
        })?;

    let handshake_key = result
        .get("handshake_key")
        .and_then(|v| v.as_str())
        .and_then(|b64| {
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64).ok()
        })
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok());

    if handshake_key.is_none() {
        tracing::debug!(
            "btsp.session.create: no handshake_key in response — Phase 3 will use null cipher"
        );
    }

    Ok((session_id, handshake_key))
}

#[cfg(not(unix))]
#[allow(clippy::unused_async, reason = "signature parity with Unix variant")]
async fn create_btsp_session(
    _security_sock: &std::path::Path,
    _family_id: &str,
) -> Result<(String, Option<[u8; 32]>), BtspSessionError> {
    Err(BtspSessionError::Protocol(
        "BTSP handshake requires Unix domain sockets".into(),
    ))
}

#[cfg(unix)]
fn b64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Errors from the BTSP session creation RPC.
#[derive(Debug, thiserror::Error)]
pub enum BtspSessionError {
    /// I/O failure while reading or writing handshake bytes.
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    /// JSON parse or serialize failure on handshake messages.
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// BTSP protocol violation or handshake rejection.
    #[error("{0}")]
    Protocol(String),
}

#[cfg(test)]
#[path = "btsp/btsp_guard_tests.rs"]
mod tests;
