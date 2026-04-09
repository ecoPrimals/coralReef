// SPDX-License-Identifier: AGPL-3.0-or-later
//! BTSP (biomeOS Transport Security Protocol) Phase 2 scaffolding.
//!
//! Per wateringHole `BTSP_PROTOCOL_STANDARD` v1.0 and `PRIMAL_SELF_KNOWLEDGE_STANDARD`
//! v1.1: when `FAMILY_ID` is set (production mode), every incoming socket connection
//! MUST complete a BTSP handshake before any JSON-RPC methods are exposed. Connections
//! that fail the handshake are refused.
//!
//! Consumer primals (like coralReef) call `BearDog`'s handshake-as-a-service RPC
//! (`btsp.session.create`, `btsp.session.verify`, `btsp.session.negotiate`) rather
//! than implementing the X25519 + HMAC-SHA256 challenge-response directly.
//!
//! This module provides:
//! - [`BtspMode`] — production vs development mode detection
//! - [`btsp_mode`] — cached mode resolution from environment
//! - [`gate_connection`] — per-connection gate for accept loops

use std::sync::OnceLock;

use crate::config;

/// BTSP operating mode derived from environment at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtspMode {
    /// Development: `FAMILY_ID` is unset or `"default"`. No handshake required.
    /// Raw cleartext JSON-RPC — `cargo test`, local dev, primalSpring experiments.
    Development,

    /// Production: `FAMILY_ID` is set to a non-default value. BTSP handshake
    /// is mandatory on every incoming connection. Connections that do not
    /// complete the handshake are refused.
    Production {
        /// The active family ID (non-default).
        family_id: String,
    },
}

impl BtspMode {
    /// `true` when the handshake is required on incoming connections.
    #[must_use]
    #[allow(dead_code, reason = "API for future BTSP handshake enforcement logic")]
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

/// Result of the BTSP connection gate.
#[derive(Debug)]
pub enum GateVerdict {
    /// Connection is allowed — proceed to JSON-RPC dispatch.
    Allow,
    /// Connection refused — handshake failed or not yet implemented.
    Refuse(String),
}

/// Per-connection BTSP gate for socket accept loops.
///
/// In `Development` mode, all connections pass through (no handshake).
///
/// In `Production` mode, the handshake must complete before any JSON-RPC
/// frames are exchanged. Until `BearDog` handshake-as-a-service is wired,
/// production connections are refused with a diagnostic message.
///
/// When the `hotspring-sec2-hal` branch lands, this function will:
/// 1. Read `ClientHello` from the stream
/// 2. Call `BearDog`'s `btsp.session.verify` to validate the challenge-response
/// 3. Negotiate cipher suite (`BTSP_NULL` for cleartext after auth)
/// 4. Return `GateVerdict::Allow` with session context on success
#[must_use]
pub fn gate_connection(mode: &BtspMode) -> GateVerdict {
    match mode {
        BtspMode::Development => GateVerdict::Allow,
        BtspMode::Production { family_id } => GateVerdict::Refuse(format!(
            "BTSP handshake required for family {family_id} but not yet implemented \
             (pending hotspring-sec2-hal integration)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_mode_allows_all() {
        let verdict = gate_connection(&BtspMode::Development);
        assert!(matches!(verdict, GateVerdict::Allow));
    }

    #[test]
    fn production_mode_refuses_pending_implementation() {
        let verdict = gate_connection(&BtspMode::Production {
            family_id: "test-family-42".into(),
        });
        assert!(matches!(verdict, GateVerdict::Refuse(_)));
        if let GateVerdict::Refuse(msg) = verdict {
            assert!(msg.contains("test-family-42"));
            assert!(msg.contains("hotspring-sec2-hal"));
        }
    }

    #[test]
    fn development_does_not_require_handshake() {
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
}
