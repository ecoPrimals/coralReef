// SPDX-License-Identifier: AGPL-3.0-or-later
//! BTSP Phase 2 scaffolding for coral-ember.
//!
//! See wateringHole `BTSP_PROTOCOL_STANDARD` v1.0.

use std::sync::OnceLock;

/// BTSP operating mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BtspMode {
    /// Development — no handshake.
    Development,
    /// Production — BTSP handshake mandatory.
    Production { family_id: String },
}

impl BtspMode {
    /// `true` when the handshake is required on incoming connections.
    #[must_use]
    #[allow(dead_code, reason = "API for future BTSP handshake enforcement logic")]
    pub(crate) const fn requires_handshake(&self) -> bool {
        matches!(self, Self::Production { .. })
    }
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

/// Result of the BTSP connection gate.
#[derive(Debug)]
pub(crate) enum GateVerdict {
    Allow,
    Refuse(String),
}

/// Per-connection BTSP gate.
#[must_use]
pub(crate) fn gate_connection(mode: &BtspMode) -> GateVerdict {
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
    fn development_allows() {
        assert!(matches!(
            gate_connection(&BtspMode::Development),
            GateVerdict::Allow
        ));
    }

    #[test]
    fn production_refuses() {
        assert!(matches!(
            gate_connection(&BtspMode::Production {
                family_id: "x".into()
            }),
            GateVerdict::Refuse(_)
        ));
    }
}
