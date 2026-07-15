// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unit tests for the BTSP session negotiation module.

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
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_dir = dir.path().join("biomeos");
    std::fs::create_dir_all(&sock_dir).expect("create");
    let result = discover_security_socket_in_dir(&sock_dir, "nonexistent-test-family-9f3a7b");
    assert!(
        result.is_none(),
        "expected None when no matching socket exists, got {result:?}"
    );
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

#[test]
fn outcome_session_id_authenticated() {
    let o = BtspOutcome::Authenticated {
        session_id: "test-session-42".into(),
    };
    assert_eq!(o.session_id(), Some("test-session-42"));
}

#[test]
fn outcome_session_id_none_for_non_authenticated() {
    assert!(BtspOutcome::DevMode.session_id().is_none());
    let d = BtspOutcome::Degraded {
        reason: "test".into(),
    };
    assert!(d.session_id().is_none());
    let r = BtspOutcome::Rejected {
        reason: "test".into(),
    };
    assert!(r.session_id().is_none());
}

#[test]
fn btsp_mode_display_variants() {
    let dev = BtspMode::Development;
    assert!(!dev.requires_handshake());
    let prod = BtspMode::Production {
        family_id: "fam-abc".into(),
    };
    assert!(prod.requires_handshake());
    assert_eq!(
        format!("{prod:?}"),
        r#"Production { family_id: "fam-abc" }"#
    );
}

#[test]
fn plain_jsonrpc_marker_is_open_brace() {
    assert_eq!(PLAIN_JSONRPC_MARKER, b'{');
}

#[tokio::test]
async fn guard_from_first_byte_dev_mode_any_byte_accepts() {
    if btsp_mode().requires_handshake() {
        return;
    }
    for byte in [Some(b'{'), Some(0xEC), Some(0x00), None] {
        let outcome = guard_from_first_byte(byte).await;
        assert!(
            outcome.should_accept(),
            "byte {byte:?} should accept in dev mode"
        );
    }
}

#[test]
fn discover_scoped_socket_preferred_over_unscoped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_dir = dir.path();

    let unscoped = sock_dir.join(format!("{SECURITY_DOMAIN}.sock"));
    std::fs::write(&unscoped, "").expect("create unscoped");

    let family = "test-family-1234";
    let scoped = sock_dir.join(format!("{SECURITY_DOMAIN}-{family}.sock"));
    std::fs::write(&scoped, "").expect("create scoped");

    let result = discover_security_socket_in_dir(sock_dir, family);
    assert_eq!(
        result,
        Some(scoped),
        "scoped socket should win over unscoped"
    );
}

#[test]
fn discover_falls_back_to_unscoped_socket() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_dir = dir.path();

    let unscoped = sock_dir.join(format!("{SECURITY_DOMAIN}.sock"));
    std::fs::write(&unscoped, "").expect("create unscoped");

    let result = discover_security_socket_in_dir(sock_dir, "no-scoped-here");
    assert_eq!(result, Some(unscoped));
}

#[test]
fn discover_by_capability_finds_matching_discovery_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_dir = dir.path();

    let mock_sock = sock_dir.join("mock-provider.sock");
    std::fs::write(&mock_sock, "").expect("create mock socket");

    let discovery = serde_json::json!({
        "primal": "mock-provider",
        "methods": ["btsp.session.create", "crypto.sign"],
        "transports": {
            "unix": format!("unix://{}", mock_sock.display())
        }
    });
    std::fs::write(sock_dir.join("mock-provider.json"), discovery.to_string())
        .expect("write discovery");

    let result = discover_by_capability(sock_dir, "btsp.session.create");
    assert_eq!(result, Some(mock_sock));
}

#[test]
fn discover_by_capability_ignores_non_matching_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_dir = dir.path();

    let discovery = serde_json::json!({
        "primal": "other",
        "methods": ["storage.read", "storage.write"],
        "transports": {"unix": "unix:///tmp/other.sock"}
    });
    std::fs::write(sock_dir.join("other.json"), discovery.to_string()).expect("write");

    let result = discover_by_capability(sock_dir, "btsp.session.create");
    assert!(result.is_none());
}

#[test]
fn discover_by_capability_skips_malformed_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("bad.json"), "not json").expect("write");
    let result = discover_by_capability(dir.path(), "btsp.session.create");
    assert!(result.is_none());
}

#[test]
fn check_discovery_file_missing_methods_field() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("no-methods.json");
    let data = serde_json::json!({"primal": "test"});
    std::fs::write(&path, data.to_string()).expect("write");
    assert!(check_discovery_file_for_method(&path, "btsp.session.create").is_none());
}

#[test]
fn check_discovery_file_missing_transport_unix() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("no-transport.json");
    let data = serde_json::json!({
        "methods": ["btsp.session.create"],
        "transports": {"tcp": "127.0.0.1:9999"}
    });
    std::fs::write(&path, data.to_string()).expect("write");
    assert!(check_discovery_file_for_method(&path, "btsp.session.create").is_none());
}

#[test]
fn resolve_socket_dir_returns_path() {
    let dir = resolve_socket_dir();
    assert!(
        !dir.as_os_str().is_empty(),
        "socket dir should be non-empty"
    );
}

#[test]
fn btsp_session_error_display() {
    let io_err = BtspSessionError::Io(std::io::Error::new(
        std::io::ErrorKind::ConnectionRefused,
        "refused",
    ));
    assert!(io_err.to_string().contains("refused"));

    let proto = BtspSessionError::Protocol("test protocol error".into());
    assert_eq!(proto.to_string(), "test protocol error");
}

#[test]
fn btsp_session_error_json() {
    let bad: Result<serde_json::Value, _> = serde_json::from_str("}{");
    let err = BtspSessionError::Json(bad.unwrap_err());
    assert!(err.to_string().contains("JSON"));
}

#[test]
fn discover_by_capability_empty_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let result = discover_by_capability(dir.path(), "btsp.session.create");
    assert!(result.is_none());
}

#[test]
fn discover_by_capability_nonexistent_dir() {
    let result = discover_by_capability(
        std::path::Path::new("/nonexistent/dir/coralreef-btsp-test"),
        "btsp.session.create",
    );
    assert!(result.is_none());
}

#[test]
fn check_discovery_file_nonexistent() {
    let result = check_discovery_file_for_method(
        std::path::Path::new("/nonexistent/file.json"),
        "btsp.session.create",
    );
    assert!(result.is_none());
}

#[test]
fn check_discovery_file_methods_not_array() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("methods-string.json");
    let data = serde_json::json!({
        "methods": "btsp.session.create",
        "transports": {"unix": "unix:///tmp/x.sock"}
    });
    std::fs::write(&path, data.to_string()).expect("write");
    assert!(check_discovery_file_for_method(&path, "btsp.session.create").is_none());
}

#[test]
fn check_discovery_file_method_present_but_socket_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("method-ok.json");
    let data = serde_json::json!({
        "methods": ["btsp.session.create"],
        "transports": {"unix": "unix:///nonexistent/socket.sock"}
    });
    std::fs::write(&path, data.to_string()).expect("write");
    assert!(
        check_discovery_file_for_method(&path, "btsp.session.create").is_none(),
        "should return None when socket file doesn't exist"
    );
}

#[test]
fn outcome_debug_format() {
    let dev = BtspOutcome::DevMode;
    assert!(format!("{dev:?}").contains("DevMode"));

    let auth = BtspOutcome::Authenticated {
        session_id: "sid-1".into(),
    };
    assert!(format!("{auth:?}").contains("sid-1"));

    let deg = BtspOutcome::Degraded {
        reason: "offline".into(),
    };
    assert!(format!("{deg:?}").contains("offline"));

    let rej = BtspOutcome::Rejected {
        reason: "bad".into(),
    };
    assert!(format!("{rej:?}").contains("bad"));
}

#[tokio::test]
async fn guard_connection_inner_dev_mode() {
    if btsp_mode().requires_handshake() {
        return;
    }
    let outcome = guard_connection_inner().await;
    assert!(matches!(outcome, BtspOutcome::DevMode));
}

#[test]
fn discover_security_socket_returns_none_in_clean_env() {
    if std::env::var("BTSP_PROVIDER_SOCKET").is_ok() || std::env::var("BEARDOG_SOCKET").is_ok()
    {
        return;
    }

    let sock_dir = resolve_socket_dir();
    let has_discovery_files = std::fs::read_dir(&sock_dir).ok().map_or(false, |entries| {
        entries
            .flatten()
            .any(|e| e.path().extension().is_some_and(|ext| ext == "json"))
    });
    if has_discovery_files {
        return;
    }

    let result = discover_security_socket("nonexistent-family-xz9p2");
    assert!(
        result.is_none(),
        "should be None when no sockets exist for a fake family"
    );
}

#[path = "tests/tests_btsp_session.rs"]
mod tests_btsp_session;
