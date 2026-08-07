// SPDX-License-Identifier: AGPL-3.0-or-later
//! Artifact provenance — hash, sign, and tag compiled shader binaries.
//!
//! Implements Dark Forest Invariant 3: no unsigned artifacts cross trust
//! boundaries. The signing path discovers a `crypto.sign` provider at
//! runtime via ecosystem discovery files — if none is available, provenance
//! is emitted unsigned (hash + gate + compiler version only).
//!
//! ## Signing flow
//!
//! ```text
//! compile response binary
//!   → SHA-256 content_hash
//!   → discover crypto.sign provider (socket dir scan)
//!   → JSON-RPC: crypto.sign { algorithm, data }
//!   → populate signature + key_id in ArtifactProvenance
//! ```
//!
//! The signing call is best-effort: failures degrade to unsigned provenance
//! with a tracing warning.

#![allow(
    dead_code,
    reason = "artifact signing path used by CompileResponse::with_provenance and integration tests"
)]

use crate::config;
use crate::ipc::{btsp, btsp_client};
use crate::service::types::ArtifactProvenance;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::OnceLock;

/// Cached socket path for the `crypto.sign` provider.
///
/// Resolved once at first use. `None` means no provider was found.
/// Re-discovery requires process restart — consistent with ecosystem
/// primal lifecycle (primals restart on topology changes).
static CRYPTO_SIGN_SOCKET: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Discover and cache the `crypto.sign` provider socket.
fn crypto_sign_socket() -> Option<&'static PathBuf> {
    CRYPTO_SIGN_SOCKET
        .get_or_init(|| {
            let sock_dir = config::socket_base_dir();
            let socket = btsp::discover_by_capability(&sock_dir, "crypto.sign");
            if let Some(ref path) = socket {
                tracing::info!(
                    path = %path.display(),
                    "crypto.sign provider discovered — artifact signing enabled"
                );
            } else {
                tracing::debug!("no crypto.sign provider found — provenance will be unsigned");
            }
            socket
        })
        .as_ref()
}

/// Attempt to sign a content hash via the discovered `crypto.sign` provider.
///
/// Sends a newline-delimited JSON-RPC request over the Unix socket and
/// reads one response line. Returns `(signature_hex, key_id)` on success.
///
/// Uses `std::os::unix::net::UnixStream` for synchronous I/O — the signing
/// call is a single request/response exchange that completes in microseconds
/// on a local socket.
fn try_sign(content_hash: &str) -> Option<(String, Option<String>)> {
    use std::io::{BufRead, BufReader, Write};

    let socket_path = crypto_sign_socket()?;

    let stream = crate::local_transport::connect_local_sync(socket_path)
        .inspect_err(|e| {
            tracing::warn!(
                path = %socket_path.display(),
                error = %e,
                "failed to connect to crypto.sign provider"
            );
        })
        .ok()?;

    stream
        .set_read_timeout(Some(config::CRYPTO_SIGN_READ_TIMEOUT))
        .inspect_err(|e| tracing::warn!(error = %e, "crypto.sign: set_read_timeout failed"))
        .ok()?;
    stream
        .set_write_timeout(Some(config::CRYPTO_SIGN_WRITE_TIMEOUT))
        .inspect_err(|e| tracing::warn!(error = %e, "crypto.sign: set_write_timeout failed"))
        .ok()?;

    if let btsp::BtspMode::Production { family_id } = btsp::btsp_mode() {
        let Some(provider) = btsp::discover_security_socket(family_id) else {
            tracing::warn!("BTSP production mode but no security provider — provenance unsigned");
            return None;
        };
        match btsp_client::handshake_on_stream_sync(&stream, &provider) {
            Ok(session) => {
                tracing::debug!(
                    session_id = %session.session_id,
                    cipher = %session.cipher,
                    "BTSP handshake succeeded for crypto.sign"
                );
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    provider = %provider.display(),
                    "BTSP client handshake failed — provenance unsigned"
                );
                return None;
            }
        }
    }

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "crypto.sign",
        "params": {
            "algorithm": "ed25519",
            "data": content_hash,
        },
        "id": 1,
    });

    let mut payload = serde_json::to_vec(&request)
        .inspect_err(|e| tracing::warn!(error = %e, "crypto.sign: request serialization failed"))
        .ok()?;
    payload.push(b'\n');

    let mut stream_ref = &stream;
    stream_ref
        .write_all(&payload)
        .inspect_err(|e| tracing::warn!(error = %e, "crypto.sign: write failed"))
        .ok()?;
    stream_ref
        .flush()
        .inspect_err(|e| tracing::warn!(error = %e, "crypto.sign: flush failed"))
        .ok()?;

    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .inspect_err(|e| tracing::warn!(error = %e, "crypto.sign: read response failed"))
        .ok()?;

    let resp: serde_json::Value = serde_json::from_str(line.trim())
        .inspect_err(|e| tracing::warn!(error = %e, "crypto.sign: response parse failed"))
        .ok()?;

    if let Some(err) = resp.get("error") {
        tracing::warn!(
            error = %err,
            "crypto.sign returned error — provenance unsigned"
        );
        return None;
    }

    let result = resp.get("result")?;
    let signature = result.get("signature")?.as_str()?.to_owned();
    let key_id = result
        .get("key_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);

    tracing::debug!(key_id = ?key_id, "artifact signed by crypto.sign provider");
    Some((signature, key_id))
}

/// Build artifact provenance for a compiled binary.
///
/// Always populates content hashes (SHA-256 + BLAKE3), gate identity, and
/// compiler version. Attempts to sign via a discovered `crypto.sign`
/// provider; if unavailable or on error, emits unsigned provenance.
#[must_use]
pub fn build_provenance(binary: &[u8]) -> ArtifactProvenance {
    let sha256_hash = Sha256::digest(binary);
    let content_hash = hex_encode(&sha256_hash);

    let blake3_hash = blake3::hash(binary);
    let sporeprint_hash = blake3_hash.to_hex().to_string();

    let (signature, key_id) =
        try_sign(&content_hash).map_or((None, None), |(sig, kid)| (Some(sig), kid));

    ArtifactProvenance {
        content_hash,
        hash_algorithm: "sha256".to_owned(),
        sporeprint_hash: Some(sporeprint_hash),
        gate_of_compilation: config::gate_id(),
        compiler_version: config::compiler_version_string(),
        signature,
        key_id,
    }
}

/// Hex-encode a byte slice.
fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(s, "{byte:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_provenance_populates_hash() {
        let binary = b"test shader binary";
        let prov = build_provenance(binary);
        assert_eq!(prov.hash_algorithm, "sha256");
        assert_eq!(prov.content_hash.len(), 64, "SHA-256 hex is 64 chars");
        assert!(!prov.compiler_version.is_empty());
        assert!(!prov.gate_of_compilation.is_empty());
    }

    #[test]
    fn build_provenance_populates_sporeprint_hash() {
        let binary = b"test shader binary for sporeprint";
        let prov = build_provenance(binary);
        let sporeprint = prov
            .sporeprint_hash
            .expect("sporeprint_hash should be Some");
        assert_eq!(sporeprint.len(), 64, "BLAKE3 hex is 64 chars");
        assert_ne!(
            prov.content_hash, sporeprint,
            "SHA-256 and BLAKE3 should differ"
        );
    }

    #[test]
    fn sporeprint_hash_matches_direct_blake3() {
        let binary = b"blake3 consistency check";
        let prov = build_provenance(binary);
        let expected = blake3::hash(binary).to_hex().to_string();
        assert_eq!(prov.sporeprint_hash.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn build_provenance_deterministic() {
        let binary = b"deterministic test";
        let p1 = build_provenance(binary);
        let p2 = build_provenance(binary);
        assert_eq!(p1.content_hash, p2.content_hash);
        assert_eq!(p1.sporeprint_hash, p2.sporeprint_hash);
    }

    #[test]
    fn build_provenance_different_inputs_different_hashes() {
        let p1 = build_provenance(b"shader A");
        let p2 = build_provenance(b"shader B");
        assert_ne!(p1.content_hash, p2.content_hash);
        assert_ne!(p1.sporeprint_hash, p2.sporeprint_hash);
    }

    #[test]
    fn hex_encode_known_value() {
        let bytes = [0xde, 0xad, 0xbe, 0xef];
        assert_eq!(hex_encode(&bytes), "deadbeef");
    }

    #[test]
    fn hex_encode_empty() {
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn crypto_sign_socket_caches() {
        let first = crypto_sign_socket();
        let second = crypto_sign_socket();
        match (first, second) {
            (Some(a), Some(b)) => assert!(std::ptr::eq(a, b)),
            (None, None) => {}
            _ => panic!("cache should return consistent result"),
        }
    }

    #[test]
    fn try_sign_returns_none_without_provider() {
        let result = try_sign("abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890");
        assert!(
            result.is_none(),
            "should return None when no crypto.sign provider exists"
        );
    }

    #[test]
    fn build_provenance_unsigned_when_no_provider() {
        let prov = build_provenance(b"unsigned test binary");
        assert!(
            prov.signature.is_none(),
            "signature should be None without crypto.sign provider"
        );
        assert!(
            prov.key_id.is_none(),
            "key_id should be None without crypto.sign provider"
        );
        assert_eq!(prov.hash_algorithm, "sha256");
    }
}
