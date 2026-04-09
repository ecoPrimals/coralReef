// SPDX-License-Identifier: AGPL-3.0-or-later
//! BTSP (biomeOS Transport Security Protocol) Phase 2 scaffolding for glowplug.
//!
//! Mirrors the coralreef-core btsp module — each binary resolves its own BTSP
//! mode from the environment (primal self-knowledge, no cross-crate dependency).
//!
//! See wateringHole `BTSP_PROTOCOL_STANDARD` v1.0 for the full protocol.

use std::sync::OnceLock;

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
        let fid = std::env::var("BIOMEOS_FAMILY_ID").unwrap_or_else(|_| "default".into());
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
    /// Connection allowed — proceed to JSON-RPC dispatch.
    Allow,
    /// Connection refused — handshake required but not yet implemented.
    Refuse(String),
}

/// Per-connection BTSP gate.
///
/// Development mode: passthrough. Production mode: refuse until
/// BearDog handshake-as-a-service is wired (hotspring-sec2-hal).
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
            family_id: "test-42".into(),
        });
        assert!(matches!(verdict, GateVerdict::Refuse(_)));
    }
}
