// SPDX-License-Identifier: AGPL-3.0-or-later
//! BTSP client handshake for outbound connections.
//!
//! Implements the `ClientHello → ServerHello → ChallengeResponse →
//! HandshakeComplete` wire protocol per `BTSP_PROTOCOL_STANDARD` v1.0.
//!
//! All cryptographic operations are delegated to the security provider via
//! `btsp.session.create` and `btsp.session.verify` JSON-RPC calls — coralReef
//! never handles raw key material directly.
//!
//! ## Wire Flow
//!
//! ```text
//! coralReef (client)       Security Provider        Target Primal
//!     │                          │                        │
//!     │─ btsp.session.create ──▶ │                        │
//!     │◀ ephemeral_pub ─────── │                        │
//!     │                          │                        │
//!     │─ ClientHello ────────────────────────────────── ▶ │
//!     │◀ ServerHello (challenge) ──────────────────────── │
//!     │                          │                        │
//!     │─ btsp.session.verify ──▶ │                        │
//!     │◀ client_response ────── │                        │
//!     │                          │                        │
//!     │─ ChallengeResponse ──────────────────────────── ▶ │
//!     │◀ HandshakeComplete ──────────────────────────── │
//!     │                          │                        │
//!     │   (stream authenticated — send application RPC)   │
//! ```

#![allow(
    dead_code,
    clippy::duplicated_attributes,
    reason = "BTSP client outbound handshake API used by provenance signing and integration tests; parent module also cfg-gates dead_code for non-Unix"
)]

use super::transport::TransportEndpoint;

/// Result of a successful BTSP client handshake.
#[derive(Debug, Clone)]
pub struct BtspSession {
    /// Provider-issued session identifier.
    pub session_id: String,
    /// Negotiated cipher suite.
    pub cipher: String,
}

/// Errors from BTSP client handshake operations.
#[derive(Debug, thiserror::Error)]
pub enum BtspClientError {
    /// I/O error during socket communication.
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialization/deserialization error.
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// BTSP protocol error (unexpected response, missing fields, rejection).
    #[error("BTSP protocol: {0}")]
    Protocol(String),
}

const DEFAULT_CIPHER: &str = "chacha20_poly1305";

/// Timeout for each security provider RPC call during handshake.
const PROVIDER_RPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Perform a synchronous BTSP client handshake on an already-connected stream.
///
/// `target` is the connection to the peer primal (e.g., a `crypto.sign`
/// provider). `provider_endpoint` is the transport endpoint for the security
/// provider for session management RPCs (`btsp.session.create`,
/// `btsp.session.verify`).
///
/// **G68**: accepts [`TransportEndpoint`] — works over UDS or TCP depending
/// on how the security provider was discovered.
///
/// After successful return, `target` is authenticated and the caller can
/// send application-level JSON-RPC on it.
///
/// Transport-agnostic: operates on [`SyncTransportStream`] from the G66 layer.
///
/// # Errors
///
/// Returns [`BtspClientError`] if the security provider is unreachable,
/// the target rejects the handshake, or any wire protocol step fails.
pub fn handshake_on_stream_sync(
    target: &super::transport::SyncTransportStream,
    provider_endpoint: &TransportEndpoint,
) -> Result<BtspSession, BtspClientError> {
    let create_result = provider_rpc(
        provider_endpoint,
        "btsp.session.create",
        &serde_json::json!({
            "family_seed_ref": "env:FAMILY_SEED",
            "role": "client"
        }),
    )?;

    let client_ephemeral_pub = json_str(&create_result, "client_ephemeral_pub")
        .ok_or_else(|| BtspClientError::Protocol("missing client_ephemeral_pub".into()))?;
    let session_ref = json_str(&create_result, "session_id")
        .ok_or_else(|| BtspClientError::Protocol("missing session_id".into()))?;

    write_json_line(
        target,
        &serde_json::json!({
            "type": "ClientHello",
            "version": 1,
            "client_ephemeral_pub": client_ephemeral_pub
        }),
    )?;

    let server_hello = read_json_line(target)?;
    check_wire_error(&server_hello, "server rejected handshake")?;

    let server_ephemeral_pub = server_hello
        .get("server_ephemeral_pub")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| BtspClientError::Protocol("missing server_ephemeral_pub".into()))?;
    let challenge = server_hello
        .get("challenge")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| BtspClientError::Protocol("missing challenge".into()))?;

    let verify_result = provider_rpc(
        provider_endpoint,
        "btsp.session.verify",
        &serde_json::json!({
            "session_id": session_ref,
            "client_ephemeral_pub": client_ephemeral_pub,
            "server_ephemeral_pub": server_ephemeral_pub,
            "challenge": challenge,
            "role": "client"
        }),
    )?;

    let client_response = json_str(&verify_result, "client_response")
        .ok_or_else(|| BtspClientError::Protocol("missing client_response".into()))?;

    write_json_line(
        target,
        &serde_json::json!({
            "type": "ChallengeResponse",
            "response": client_response,
            "preferred_cipher": DEFAULT_CIPHER
        }),
    )?;

    let hs_complete = read_json_line(target)?;
    check_wire_error(&hs_complete, "handshake verification failed")?;

    let cipher = hs_complete
        .get("cipher")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(DEFAULT_CIPHER);
    let session_id = hs_complete
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            BtspClientError::Protocol("missing session_id in HandshakeComplete".into())
        })?;

    tracing::info!(
        session_id = %session_id,
        cipher = %cipher,
        "BTSP client handshake complete"
    );

    Ok(BtspSession {
        session_id: session_id.to_owned(),
        cipher: cipher.to_owned(),
    })
}

// ---------------------------------------------------------------------------
// Security provider RPC
// ---------------------------------------------------------------------------

/// Send a sync JSON-RPC request to the security provider and extract `result`.
///
/// **G68**: connects via [`TransportEndpoint`] — UDS or TCP.
fn provider_rpc(
    endpoint: &TransportEndpoint,
    method: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, BtspClientError> {
    let stream = crate::transport::connect_transport_sync(endpoint).map_err(|e| {
        BtspClientError::Protocol(format!(
            "security provider at {} unreachable: {e}",
            endpoint.display_uri()
        ))
    })?;
    stream.set_read_timeout(Some(PROVIDER_RPC_TIMEOUT)).ok();
    stream.set_write_timeout(Some(PROVIDER_RPC_TIMEOUT)).ok();

    write_json_line(
        &stream,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        }),
    )?;

    let response = read_json_line(&stream)?;
    check_wire_error(&response, method)?;

    response
        .get("result")
        .cloned()
        .ok_or_else(|| BtspClientError::Protocol(format!("{method}: missing result")))
}

// ---------------------------------------------------------------------------
// Wire helpers — byte-by-byte I/O avoids BufReader buffering interference
// ---------------------------------------------------------------------------

/// Write a JSON value as a newline-delimited message.
fn write_json_line(
    mut stream: impl std::io::Write,
    value: &serde_json::Value,
) -> Result<(), BtspClientError> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}

/// Read one newline-delimited JSON message (byte-by-byte to avoid buffering).
fn read_json_line(mut stream: impl std::io::Read) -> Result<serde_json::Value, BtspClientError> {
    let mut buf = Vec::with_capacity(4096);
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte)?;
        if byte[0] == b'\n' {
            break;
        }
        buf.push(byte[0]);
    }
    Ok(serde_json::from_slice(&buf)?)
}

/// Check a JSON-RPC or wire response for an error field.
fn check_wire_error(response: &serde_json::Value, context: &str) -> Result<(), BtspClientError> {
    if let Some(error) = response.get("error") {
        let reason = error
            .get("reason")
            .or_else(|| error.get("message"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        return Err(BtspClientError::Protocol(format!("{context}: {reason}")));
    }
    Ok(())
}

/// Extract a string field from a JSON object.
fn json_str(obj: &serde_json::Value, key: &str) -> Option<String> {
    obj.get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn btsp_session_fields_accessible() {
        let session = BtspSession {
            session_id: "sess-42".to_owned(),
            cipher: "chacha20_poly1305".to_owned(),
        };
        assert_eq!(session.session_id, "sess-42");
        assert_eq!(session.cipher, "chacha20_poly1305");
    }

    #[test]
    fn btsp_session_clone_is_independent() {
        let session = BtspSession {
            session_id: "sess-1".to_owned(),
            cipher: "null".to_owned(),
        };
        let cloned = session.clone();
        assert_eq!(cloned.session_id, session.session_id);
        assert_eq!(cloned.cipher, session.cipher);
    }

    #[test]
    fn check_wire_error_passes_on_no_error() {
        let response = serde_json::json!({"result": {"session_id": "abc"}});
        assert!(check_wire_error(&response, "test").is_ok());
    }

    #[test]
    fn check_wire_error_extracts_reason() {
        let response = serde_json::json!({"error": {"reason": "auth_failed"}});
        let err = check_wire_error(&response, "test").expect_err("should fail");
        assert!(err.to_string().contains("auth_failed"), "got: {err}");
    }

    #[test]
    fn check_wire_error_extracts_message_fallback() {
        let response = serde_json::json!({"error": {"message": "bad request"}});
        let err = check_wire_error(&response, "ctx").expect_err("should fail");
        assert!(err.to_string().contains("bad request"), "got: {err}");
    }

    #[test]
    fn json_str_extracts_string() {
        let obj = serde_json::json!({"key": "value", "num": 42});
        assert_eq!(json_str(&obj, "key"), Some("value".to_owned()));
        assert_eq!(json_str(&obj, "missing"), None);
        assert_eq!(json_str(&obj, "num"), None);
    }

    #[test]
    fn read_json_line_parses_newline_delimited() {
        let data = b"{\"type\":\"test\"}\n";
        let val = read_json_line(&data[..]).expect("should parse");
        assert_eq!(val["type"], "test");
    }

    #[test]
    fn read_json_line_fails_on_invalid_json() {
        let data = b"not json\n";
        assert!(read_json_line(&data[..]).is_err());
    }

    #[test]
    fn write_json_line_appends_newline() {
        let mut buf = Vec::new();
        let val = serde_json::json!({"hello": "world"});
        write_json_line(&mut buf, &val).expect("should write");
        assert!(buf.ends_with(b"\n"));
        let parsed: serde_json::Value =
            serde_json::from_slice(&buf[..buf.len() - 1]).expect("should parse");
        assert_eq!(parsed["hello"], "world");
    }

    #[cfg(unix)]
    #[test]
    fn handshake_fails_on_nonexistent_provider() {
        let (target, _peer) = std::os::unix::net::UnixStream::pair().expect("stream pair");
        let target = super::super::transport::SyncTransportStream::Unix(target);
        let ep = TransportEndpoint::Uds {
            path: "/nonexistent/btsp-provider.sock".into(),
        };
        let err = handshake_on_stream_sync(&target, &ep);
        assert!(err.is_err(), "should fail with nonexistent provider");
        let msg = err.expect_err("err").to_string();
        assert!(
            msg.contains("unreachable"),
            "error should mention unreachable: {msg}"
        );
    }

    #[test]
    fn btsp_client_error_display() {
        let err = BtspClientError::Protocol("test failure".into());
        assert_eq!(err.to_string(), "BTSP protocol: test failure");

        let io_err = BtspClientError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "refused",
        ));
        assert!(io_err.to_string().contains("refused"));
    }
}
