// SPDX-License-Identifier: AGPL-3.0-or-later
//! BTSP Phase 3 — `btsp.negotiate` server handler and encrypted session keys.
//!
//! After a successful Phase 2 BTSP handshake (in [`super::btsp`]), the client may
//! send a `btsp.negotiate` JSON-RPC call to upgrade the channel from plaintext to
//! ChaCha20-Poly1305 AEAD. This module handles that negotiation and provides the
//! [`SessionKeys`] type for the encrypted frame loop.
//!
//! ## Key Export Pattern
//!
//! Per `BTSP_PROTOCOL_STANDARD` v1.0, the security provider (`BearDog`) returns a
//! `handshake_key` in the `btsp.session.create` response. When present, Phase 3
//! derives real AEAD keys via HKDF-SHA256. When absent, the null cipher fallback
//! keeps everything working on plaintext.
//!
//! ## Wire Format (encrypted channel)
//!
//! ```text
//! [4 bytes: length (big-endian u32)] [12 bytes: nonce] [ciphertext + Poly1305 tag]
//! ```

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::btsp::{BtspMode, btsp_mode};

// ──── Session Registry ─────────────────────────────────────────────────────

/// Per-session state stored after successful Phase 2 authentication.
pub(super) struct SessionEntry {
    /// The HKDF-derived handshake key from `btsp.session.create`, if available.
    ///
    /// `None` when the security provider didn't return key material (degraded mode).
    pub(super) handshake_key: Option<[u8; 32]>,
}

/// Global session registry — maps `session_id` → key material from Phase 2.
static SESSION_REGISTRY: OnceLock<Mutex<HashMap<String, SessionEntry>>> = OnceLock::new();

fn session_registry() -> &'static Mutex<HashMap<String, SessionEntry>> {
    SESSION_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a `session_id` from a successful Phase 2 BTSP authentication.
///
/// If the security provider returned a `handshake_key`, pass it here so Phase 3
/// negotiation can derive real AEAD session keys. If `None`, Phase 3 falls back
/// to null cipher.
pub fn register_session(session_id: String, handshake_key: Option<[u8; 32]>) {
    if let Ok(mut sessions) = session_registry().lock() {
        sessions.insert(session_id, SessionEntry { handshake_key });
    }
}

// ──── Negotiate Handler ────────────────────────────────────────────────────

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
    #[allow(
        dead_code,
        reason = "parsed from client JSON, used in tracing + future bond-aware routing"
    )]
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
/// and derives session keys via HKDF-SHA256 when the handshake key is available.
///
/// When the handshake key from `btsp.session.create` is present, returns
/// `"chacha20-poly1305"` and derives directional encrypt/decrypt keys. When
/// absent (provider didn't return key material), falls back to `"null"` cipher.
///
/// Derived keys are stored in [`NEGOTIATED_KEYS`] for the encrypted frame loop.
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

    let handshake_key = lookup_session(&req.session_id)?;

    if req.client_nonce.is_empty() {
        return Err(NegotiateError::InvalidParams(
            "client_nonce must not be empty".into(),
        ));
    }

    let client_nonce_bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        &req.client_nonce,
    )
    .map_err(|e| NegotiateError::InvalidParams(format!("client_nonce is not valid base64: {e}")))?;

    if client_nonce_bytes.len() < 12 {
        return Err(NegotiateError::InvalidParams(format!(
            "client_nonce too short: {} bytes (need >= 12)",
            client_nonce_bytes.len()
        )));
    }

    let nonce_bytes: [u8; 24] = rand::random();
    let server_nonce =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, nonce_bytes);

    let cipher = match handshake_key {
        Some(hk)
            if req.preferred_cipher == "chacha20-poly1305" || req.preferred_cipher.is_empty() =>
        {
            match SessionKeys::derive(&hk, &client_nonce_bytes, &nonce_bytes, false) {
                Ok(keys) => {
                    store_negotiated_keys(&req.session_id, keys);
                    tracing::debug!(
                        session_id = %req.session_id,
                        "btsp.negotiate: derived AEAD keys, returning chacha20-poly1305"
                    );
                    "chacha20-poly1305"
                }
                Err(e) => {
                    tracing::warn!(
                        session_id = %req.session_id,
                        error = %e,
                        "btsp.negotiate: HKDF derivation failed, falling back to null cipher"
                    );
                    "null"
                }
            }
        }
        Some(_) => {
            tracing::debug!(
                session_id = %req.session_id,
                preferred_cipher = %req.preferred_cipher,
                "btsp.negotiate: client requested unsupported cipher, returning null"
            );
            "null"
        }
        None => {
            tracing::debug!(
                session_id = %req.session_id,
                "btsp.negotiate: no handshake key available, returning null cipher"
            );
            "null"
        }
    };

    Ok(NegotiateResponse {
        cipher: cipher.into(),
        server_nonce,
    })
}

/// Look up a session and return its handshake key (if any).
///
/// In development mode, unknown sessions are accepted with `None` key.
fn lookup_session(session_id: &str) -> Result<Option<[u8; 32]>, NegotiateError> {
    let entry = session_registry()
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(session_id).map(|e| e.handshake_key));

    entry.map_or_else(
        || {
            if matches!(btsp_mode(), BtspMode::Production { .. }) {
                Err(NegotiateError::InvalidSession(format!(
                    "session_id '{session_id}' not found in active sessions"
                )))
            } else {
                tracing::debug!(
                    session_id,
                    "dev mode: accepting unknown session_id for negotiate"
                );
                Ok(None)
            }
        },
        Ok,
    )
}

// ──── Session Key Derivation & Storage ─────────────────────────────────────

/// Derived session keys for the encrypted post-handshake channel.
///
/// Both sides derive the same keys from the Phase 1 handshake key + nonces.
/// Server encrypts with `s2c` key, decrypts with `c2s` key.
#[derive(zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
pub struct SessionKeys {
    encrypt_key: [u8; 32],
    decrypt_key: [u8; 32],
}

impl std::fmt::Debug for SessionKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionKeys")
            .field("encrypt_key", &"[redacted]")
            .field("decrypt_key", &"[redacted]")
            .finish()
    }
}

impl SessionKeys {
    /// Derive session keys via HKDF-SHA256.
    ///
    /// ```text
    /// salt = client_nonce || server_nonce
    /// c2s  = HKDF-Expand(PRK, info="btsp-session-v1-c2s", L=32)
    /// s2c  = HKDF-Expand(PRK, info="btsp-session-v1-s2c", L=32)
    /// ```
    ///
    /// `is_client` flips the encrypt/decrypt assignment so both sides derive
    /// mirrored key pairs.
    ///
    /// # Errors
    ///
    /// Returns [`NegotiateError`] if HKDF expansion fails.
    pub fn derive(
        handshake_key: &[u8; 32],
        client_nonce: &[u8],
        server_nonce: &[u8],
        is_client: bool,
    ) -> Result<Self, NegotiateError> {
        use hkdf::Hkdf;
        use sha2::Sha256;

        let mut salt = Vec::with_capacity(client_nonce.len() + server_nonce.len());
        salt.extend_from_slice(client_nonce);
        salt.extend_from_slice(server_nonce);

        let hk = Hkdf::<Sha256>::new(Some(&salt), handshake_key);

        let mut client_to_server = [0u8; 32];
        hk.expand(b"btsp-session-v1-c2s", &mut client_to_server)
            .map_err(|e| {
                NegotiateError::InvalidParams(format!("HKDF c2s expansion failed: {e}"))
            })?;

        let mut server_to_client = [0u8; 32];
        hk.expand(b"btsp-session-v1-s2c", &mut server_to_client)
            .map_err(|e| {
                NegotiateError::InvalidParams(format!("HKDF s2c expansion failed: {e}"))
            })?;

        if is_client {
            Ok(Self {
                encrypt_key: client_to_server,
                decrypt_key: server_to_client,
            })
        } else {
            Ok(Self {
                encrypt_key: server_to_client,
                decrypt_key: client_to_server,
            })
        }
    }

    /// Encrypt a plaintext message for transmission.
    ///
    /// Returns `nonce || ciphertext` (12 + `plaintext.len()` + 16 bytes).
    ///
    /// # Errors
    ///
    /// Returns [`NegotiateError`] if encryption or nonce generation fails.
    #[allow(
        dead_code,
        reason = "public API for encrypted frame loop — wired after transport upgrade"
    )]
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, NegotiateError> {
        use chacha20poly1305::aead::{Aead, KeyInit};
        use chacha20poly1305::{ChaCha20Poly1305, Nonce};

        let cipher = ChaCha20Poly1305::new((&self.encrypt_key).into());

        let mut nonce_bytes = [0u8; 12];
        getrandom::fill(&mut nonce_bytes)
            .map_err(|e| NegotiateError::InvalidParams(format!("nonce generation failed: {e}")))?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| NegotiateError::InvalidParams(format!("AEAD encrypt failed: {e}")))?;

        let mut frame = Vec::with_capacity(12 + ciphertext.len());
        frame.extend_from_slice(&nonce_bytes);
        frame.extend_from_slice(&ciphertext);
        Ok(frame)
    }

    /// Decrypt a received frame (`nonce || ciphertext`).
    ///
    /// # Errors
    ///
    /// Returns [`NegotiateError`] if the frame is too short or decryption fails.
    #[allow(
        dead_code,
        reason = "public API for encrypted frame loop — wired after transport upgrade"
    )]
    pub fn decrypt(&self, frame: &[u8]) -> Result<Vec<u8>, NegotiateError> {
        use chacha20poly1305::aead::{Aead, KeyInit};
        use chacha20poly1305::{ChaCha20Poly1305, Nonce};

        if frame.len() < 12 + 16 {
            return Err(NegotiateError::InvalidParams(format!(
                "frame too short: {} bytes (need >= 28)",
                frame.len()
            )));
        }

        let (nonce_bytes, ciphertext) = frame.split_at(12);
        let cipher = ChaCha20Poly1305::new((&self.decrypt_key).into());
        let nonce = Nonce::from_slice(nonce_bytes);

        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| NegotiateError::InvalidParams(format!("AEAD decrypt failed: {e}")))
    }
}

/// Negotiated session keys, indexed by `session_id`, for use by the encrypted
/// frame loop after `btsp.negotiate` completes.
static NEGOTIATED_KEYS: OnceLock<Mutex<HashMap<String, SessionKeys>>> = OnceLock::new();

fn negotiated_keys_registry() -> &'static Mutex<HashMap<String, SessionKeys>> {
    NEGOTIATED_KEYS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn store_negotiated_keys(session_id: &str, keys: SessionKeys) {
    if let Ok(mut map) = negotiated_keys_registry().lock() {
        map.insert(session_id.to_owned(), keys);
    }
}

/// Take the negotiated [`SessionKeys`] for a session, removing them from the registry.
///
/// The transport layer calls this once after `btsp.negotiate` returns
/// `"chacha20-poly1305"` to switch to encrypted framing.
#[must_use]
#[allow(
    dead_code,
    reason = "public API for encrypted frame loop — wired after transport upgrade"
)]
pub fn take_negotiated_keys(session_id: &str) -> Option<SessionKeys> {
    negotiated_keys_registry()
        .lock()
        .ok()
        .and_then(|mut map| map.remove(session_id))
}

// ──── Errors ───────────────────────────────────────────────────────────────

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
    #[allow(
        dead_code,
        reason = "public API for downstream error formatting evolution"
    )]
    pub const fn jsonrpc_code(&self) -> i64 {
        match self {
            Self::InvalidSession(_) | Self::InvalidParams(_) => -32_602,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 24 bytes of test nonce, base64-encoded (>= 12 decoded bytes).
    fn test_client_nonce() -> String {
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0xAA; 24])
    }

    #[test]
    fn negotiate_empty_session_id_rejected() {
        let req = NegotiateRequest {
            session_id: String::new(),
            preferred_cipher: "chacha20-poly1305".into(),
            client_nonce: test_client_nonce(),
            bond_type: "Covalent".into(),
        };
        let err = handle_negotiate(&req).unwrap_err();
        assert!(matches!(err, NegotiateError::InvalidSession(_)));
    }

    #[test]
    fn negotiate_empty_client_nonce_rejected() {
        register_session("test-session-1".into(), None);
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
    fn negotiate_no_key_returns_null_cipher() {
        register_session("test-session-2".into(), None);
        let req = NegotiateRequest {
            session_id: "test-session-2".into(),
            preferred_cipher: "chacha20-poly1305".into(),
            client_nonce: test_client_nonce(),
            bond_type: "Covalent".into(),
        };
        let resp = handle_negotiate(&req).unwrap();
        assert_eq!(resp.cipher, "null");
        assert!(!resp.server_nonce.is_empty());
    }

    #[test]
    fn negotiate_with_key_returns_chacha20() {
        let hk = [0x42; 32];
        register_session("test-session-chacha".into(), Some(hk));
        let req = NegotiateRequest {
            session_id: "test-session-chacha".into(),
            preferred_cipher: "chacha20-poly1305".into(),
            client_nonce: test_client_nonce(),
            bond_type: "Covalent".into(),
        };
        let resp = handle_negotiate(&req).unwrap();
        assert_eq!(resp.cipher, "chacha20-poly1305");
        assert!(!resp.server_nonce.is_empty());
    }

    #[test]
    fn negotiate_with_key_stores_session_keys() {
        let hk = [0x43; 32];
        register_session("test-session-keys-stored".into(), Some(hk));
        let req = NegotiateRequest {
            session_id: "test-session-keys-stored".into(),
            preferred_cipher: "chacha20-poly1305".into(),
            client_nonce: test_client_nonce(),
            bond_type: "Covalent".into(),
        };
        let _resp = handle_negotiate(&req).unwrap();
        let keys = take_negotiated_keys("test-session-keys-stored");
        assert!(
            keys.is_some(),
            "session keys should be stored after negotiate"
        );
    }

    #[test]
    fn negotiate_unsupported_cipher_returns_null() {
        let hk = [0x44; 32];
        register_session("test-session-unsupported".into(), Some(hk));
        let req = NegotiateRequest {
            session_id: "test-session-unsupported".into(),
            preferred_cipher: "aes-256-gcm".into(),
            client_nonce: test_client_nonce(),
            bond_type: "Covalent".into(),
        };
        let resp = handle_negotiate(&req).unwrap();
        assert_eq!(resp.cipher, "null");
    }

    #[test]
    fn negotiate_server_nonce_is_valid_base64() {
        register_session("test-session-3".into(), None);
        let req = NegotiateRequest {
            session_id: "test-session-3".into(),
            preferred_cipher: "chacha20-poly1305".into(),
            client_nonce: test_client_nonce(),
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
            client_nonce: test_client_nonce(),
            bond_type: "Ionic".into(),
        };
        let resp = handle_negotiate(&req).unwrap();
        assert_eq!(resp.cipher, "null");
    }

    #[test]
    fn session_registry_tracks_multiple_sessions() {
        register_session("sess-a".into(), None);
        register_session("sess-b".into(), Some([0xFF; 32]));
        let sessions = session_registry().lock().unwrap();
        assert!(sessions.contains_key("sess-a"));
        assert!(sessions.contains_key("sess-b"));
        assert!(sessions["sess-b"].handshake_key.is_some());
    }

    // ─── SessionKeys crypto tests ──────────────────────────────────────

    #[test]
    fn session_keys_derive_deterministic() {
        let hk = [0xAA; 32];
        let cn = [0xBB; 24];
        let sn = [0xCC; 24];
        let k1 = SessionKeys::derive(&hk, &cn, &sn, false).unwrap();
        let k2 = SessionKeys::derive(&hk, &cn, &sn, false).unwrap();
        assert_eq!(k1.encrypt_key, k2.encrypt_key);
        assert_eq!(k1.decrypt_key, k2.decrypt_key);
    }

    #[test]
    fn session_keys_client_server_mirror() {
        let hk = [0xAA; 32];
        let cn = [0xBB; 24];
        let sn = [0xCC; 24];
        let client = SessionKeys::derive(&hk, &cn, &sn, true).unwrap();
        let server = SessionKeys::derive(&hk, &cn, &sn, false).unwrap();
        assert_eq!(client.encrypt_key, server.decrypt_key);
        assert_eq!(client.decrypt_key, server.encrypt_key);
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        let hk = [0x42; 32];
        let cn = [0x01; 24];
        let sn = [0x02; 24];
        let client = SessionKeys::derive(&hk, &cn, &sn, true).unwrap();
        let server = SessionKeys::derive(&hk, &cn, &sn, false).unwrap();

        let plaintext = b"hello from BTSP Phase 3";
        let frame = client.encrypt(plaintext).unwrap();
        let decrypted = server.decrypt(&frame).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn encrypt_decrypt_large_payload() {
        let hk = [0x42; 32];
        let cn = [0x01; 24];
        let sn = [0x02; 24];
        let client = SessionKeys::derive(&hk, &cn, &sn, true).unwrap();
        let server = SessionKeys::derive(&hk, &cn, &sn, false).unwrap();

        let plaintext = vec![0xAB; 64 * 1024];
        let frame = client.encrypt(&plaintext).unwrap();
        let decrypted = server.decrypt(&frame).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypt_rejects_tampered_frame() {
        let hk = [0x42; 32];
        let cn = [0x01; 24];
        let sn = [0x02; 24];
        let client = SessionKeys::derive(&hk, &cn, &sn, true).unwrap();
        let server = SessionKeys::derive(&hk, &cn, &sn, false).unwrap();

        let mut frame = client.encrypt(b"authentic data").unwrap();
        let last = frame.len() - 1;
        frame[last] ^= 0xFF;
        assert!(server.decrypt(&frame).is_err());
    }

    #[test]
    fn decrypt_rejects_short_frame() {
        let server = SessionKeys::derive(&[0x42; 32], &[0x01; 24], &[0x02; 24], false).unwrap();
        assert!(server.decrypt(&[0u8; 10]).is_err());
    }

    #[test]
    fn decrypt_rejects_wrong_key() {
        let cn = [0x01; 24];
        let sn = [0x02; 24];
        let client = SessionKeys::derive(&[0xAA; 32], &cn, &sn, true).unwrap();
        let wrong_server = SessionKeys::derive(&[0xBB; 32], &cn, &sn, false).unwrap();
        let frame = client.encrypt(b"secret").unwrap();
        assert!(wrong_server.decrypt(&frame).is_err());
    }

    #[test]
    fn negotiate_client_nonce_too_short_rejected() {
        register_session("test-session-short-nonce".into(), Some([0x42; 32]));
        let short_nonce =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0xAA; 8]);
        let req = NegotiateRequest {
            session_id: "test-session-short-nonce".into(),
            preferred_cipher: "chacha20-poly1305".into(),
            client_nonce: short_nonce,
            bond_type: "Covalent".into(),
        };
        let err = handle_negotiate(&req).unwrap_err();
        assert!(matches!(err, NegotiateError::InvalidParams(_)));
    }
}
