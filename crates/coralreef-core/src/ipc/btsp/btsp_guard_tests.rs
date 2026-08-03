// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

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
fn line_looks_like_btsp_client_hello_examples() {
    assert!(line_looks_like_btsp_client_hello(
        r#"{"protocol":"btsp","ver":1}"#
    ));
    assert!(!line_looks_like_btsp_client_hello(
        r#"{"jsonrpc":"2.0","method":"health.liveness"}"#
    ));
    assert!(!line_looks_like_btsp_client_hello(
        r#"{"foo":"btsp"}"# // "protocol" missing
    ));
}

#[test]
#[ignore = "machine-dependent: a real $XDG_RUNTIME_DIR/biomeos/crypto*.sock or discovery .json can make this fail"]
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
