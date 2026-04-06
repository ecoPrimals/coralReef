// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ember socket + JSON-RPC integration tests (env mutation uses `unsafe` — not allowed in the library crate).

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::sync::Mutex;

use coral_glowplug::ember::EmberClient;
use coral_glowplug::error::EmberError;

fn read_request_line(stream: &mut impl Read) -> String {
    let mut buf = Vec::new();
    let mut one = [0u8; 1];
    loop {
        stream.read_exact(&mut one).expect("read byte");
        if one[0] == b'\n' {
            break;
        }
        buf.push(one[0]);
    }
    String::from_utf8(buf).expect("utf8")
}

static EMBER_ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn default_ember_socket_respects_env() {
    let _g = EMBER_ENV_LOCK.lock().expect("lock");
    // SAFETY: test mutex serializes env access; no other threads read these vars concurrently.
    unsafe {
        std::env::remove_var("CORALREEF_EMBER_SOCKET");
    }
    assert_eq!(
        coral_glowplug::test_support_default_ember_socket(),
        "/run/coralreef/ember.sock"
    );
    // SAFETY: same as above.
    unsafe {
        std::env::set_var(
            "CORALREEF_EMBER_SOCKET",
            "/tmp/coral-ember-integration.sock",
        );
    }
    assert_eq!(
        coral_glowplug::test_support_default_ember_socket(),
        "/tmp/coral-ember-integration.sock"
    );
    // SAFETY: same as above.
    unsafe {
        std::env::remove_var("CORALREEF_EMBER_SOCKET");
    }
}

#[test]
fn ember_list_release_reacquire_swap_roundtrip_with_mock_server() {
    let _g = EMBER_ENV_LOCK.lock().expect("lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("ember.sock");
    let _ = std::fs::remove_file(&sock_path);
    let listener = UnixListener::bind(&sock_path).expect("bind");
    // SAFETY: test mutex serializes env access.
    unsafe {
        std::env::set_var("CORALREEF_EMBER_SOCKET", sock_path.to_str().expect("utf8"));
    }

    let server = std::thread::spawn(move || {
        // `EmberClient::connect` probes the socket with a connect+drop before RPC calls.
        let (probe, _) = listener.accept().expect("probe accept");
        drop(probe);

        for _ in 0..4 {
            let (mut stream, _) = listener.accept().expect("accept");
            let line = read_request_line(&mut stream);
            if line.contains("ember.list") {
                let resp = r#"{"jsonrpc":"2.0","id":1,"result":{"devices":["0000:01:00.0"]}}"#;
                stream
                    .write_all(format!("{resp}\n").as_bytes())
                    .expect("write");
            } else if line.contains("ember.release") | line.contains("ember.reacquire") {
                let resp = r#"{"jsonrpc":"2.0","id":1,"result":{"bdf":"0000:01:00.0"}}"#;
                stream
                    .write_all(format!("{resp}\n").as_bytes())
                    .expect("write");
            } else if line.contains("ember.swap") {
                let resp = r#"{"jsonrpc":"2.0","id":1,"result":{"personality":"unbound"}}"#;
                stream
                    .write_all(format!("{resp}\n").as_bytes())
                    .expect("write");
            } else {
                panic!("unexpected line: {line}");
            }
        }
    });

    let client = EmberClient::connect().expect("ember available");
    assert_eq!(
        client.list_devices().expect("list"),
        vec!["0000:01:00.0".to_string()]
    );
    client.release_device("0000:01:00.0").expect("release");
    client.reacquire_device("0000:01:00.0").expect("reacquire");
    let swap_obs = client.swap_device("0000:01:00.0", "unbound").expect("swap");
    assert_eq!(swap_obs.to_personality, "unbound");

    server.join().expect("server join");
    // SAFETY: test mutex serializes env access.
    unsafe {
        std::env::remove_var("CORALREEF_EMBER_SOCKET");
    }
}

#[test]
fn ember_list_devices_errors_when_socket_removed() {
    let _g = EMBER_ENV_LOCK.lock().expect("lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("ember.sock");
    let _ = std::fs::remove_file(&sock_path);
    let listener = UnixListener::bind(&sock_path).expect("bind");
    // SAFETY: test mutex serializes env access.
    unsafe {
        std::env::set_var("CORALREEF_EMBER_SOCKET", sock_path.to_str().expect("utf8"));
    }
    let client = EmberClient::connect().expect("connect");
    drop(listener);
    let _ = std::fs::remove_file(&sock_path);
    let err = client.list_devices().expect_err("socket gone");
    assert!(matches!(err, EmberError::Connect(_)));
    // SAFETY: test mutex serializes env access.
    unsafe {
        std::env::remove_var("CORALREEF_EMBER_SOCKET");
    }
}
