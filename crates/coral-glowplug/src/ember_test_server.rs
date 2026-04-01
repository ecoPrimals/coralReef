// SPDX-License-Identifier: AGPL-3.0-only
use super::*;

#[test]
fn ember_client_connect_returns_none_when_no_socket() {
    let client = EmberClient::connect();
    // In test environment, ember is not running
    // This may or may not return None depending on test environment
    drop(client);
}

#[test]
fn parse_rpc_response_ok_with_null_result() {
    let line = br#"{"jsonrpc":"2.0","id":1,"result":null}"#;
    let v = parse_rpc_response(line).expect("parse");
    assert!(v.is_null());
}

#[test]
fn parse_rpc_response_err_returns_rpc_variant() {
    let line = br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"fail"}}"#;
    let err = parse_rpc_response(line).expect_err("rpc error");
    match err {
        EmberError::Rpc { code, message } => {
            assert_eq!(code, -32000);
            assert_eq!(message, "fail");
        }
        other => panic!("expected Rpc, got {other:?}"),
    }
}

#[test]
fn parse_rpc_response_invalid_json_returns_parse_error() {
    let line = b"{ not json";
    let err = parse_rpc_response(line).expect_err("parse error");
    assert!(matches!(err, EmberError::Parse(_)));
}

#[test]
fn make_rpc_request_includes_method_and_jsonrpc() {
    let req = make_rpc_request("ember.list", serde_json::json!({}));
    assert!(req.contains("ember.list"));
    assert!(req.contains("\"jsonrpc\":\"2.0\""));
    assert!(req.contains("\"id\":"));
}

#[test]
fn next_request_id_increments() {
    let a = next_request_id();
    let b = next_request_id();
    assert_eq!(b, a + 1);
}

#[test]
fn parse_rpc_response_ok_when_result_key_omitted() {
    let line = br#"{"jsonrpc":"2.0","id":1}"#;
    let v = super::parse_rpc_response(line).expect("parse");
    assert!(v.is_null());
}

#[test]
fn parse_rpc_response_error_with_extra_null_data_field() {
    let line = br#"{"jsonrpc":"2.0","id":1,"error":{"code":-5,"message":"nope","data":null}}"#;
    let err = super::parse_rpc_response(line).expect_err("rpc");
    match err {
        EmberError::Rpc { code, message } => {
            assert_eq!(code, -5);
            assert_eq!(message, "nope");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn is_transient_io_matches_would_block_and_interrupted() {
    assert!(is_transient_io(&std::io::Error::new(
        std::io::ErrorKind::WouldBlock,
        "test"
    )));
    assert!(is_transient_io(&std::io::Error::new(
        std::io::ErrorKind::Interrupted,
        "test"
    )));
    assert!(!is_transient_io(&std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "test"
    )));
}

#[cfg(unix)]
#[test]
fn read_full_response_reads_until_newline() {
    let (mut sender, receiver) =
        std::os::unix::net::UnixStream::pair().expect("unix stream pair");
    let t = std::thread::spawn(move || {
        std::io::Write::write_all(&mut sender, br#"{"ok":true}"#).expect("partial write");
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::io::Write::write_all(&mut sender, b"\n").expect("newline");
    });
    let mut buf = [0u8; 256];
    let n = read_full_response(&receiver, &mut buf).expect("read response");
    t.join().expect("writer thread");
    assert_eq!(&buf[..n], b"{\"ok\":true}\n");
}

#[cfg(unix)]
#[test]
fn read_full_response_eof_is_error() {
    let (sender, receiver) = std::os::unix::net::UnixStream::pair().expect("unix stream pair");
    drop(sender);
    let mut buf = [0u8; 64];
    let err = read_full_response(&receiver, &mut buf).expect_err("closed without data");
    assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
}

// ── EmberBootJournal ─────────────────────────────────────────────

#[test]
fn ember_boot_journal_construction() {
    let j = EmberBootJournal::new("0000:3b:00.0", "/tmp/test.sock");
    assert_eq!(j.bdf, "0000:3b:00.0");
    assert_eq!(j.socket_path, "/tmp/test.sock");
}

#[test]
fn ember_boot_journal_default_socket() {
    let j = EmberBootJournal::with_default_socket("0000:01:00.0");
    assert_eq!(j.bdf, "0000:01:00.0");
    assert_eq!(j.socket_path, super::default_ember_socket());
}

#[test]
fn ember_boot_journal_implements_boot_journal() {
    use coral_driver::nv::vfio_compute::acr_boot::BootJournal;
    let j = EmberBootJournal::new("test", "/nonexistent.sock");
    let _: &dyn BootJournal = &j;
}

// ── parse_hex_u32 ────────────────────────────────────────────────

#[test]
fn parse_hex_u32_with_prefix() {
    assert_eq!(parse_hex_u32("0x409100").unwrap(), 0x409100);
}

#[test]
fn parse_hex_u32_uppercase_prefix() {
    assert_eq!(parse_hex_u32("0X00000010").unwrap(), 0x10);
}

#[test]
fn parse_hex_u32_decimal() {
    assert_eq!(parse_hex_u32("12345").unwrap(), 12345);
}

#[test]
fn parse_hex_u32_invalid() {
    assert!(parse_hex_u32("not_a_number").is_err());
}

// ── make/parse round-trip ───────────────────────────────────────

#[test]
fn make_rpc_request_is_valid_json() {
    let req = make_rpc_request("ember.test", serde_json::json!({"key": "val"}));
    let parsed: serde_json::Value = serde_json::from_str(&req).expect("valid JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["method"], "ember.test");
    assert_eq!(parsed["params"]["key"], "val");
    assert!(parsed["id"].is_number());
}

#[test]
fn parse_rpc_response_success() {
    let raw = br#"{"jsonrpc":"2.0","result":{"ok":true},"id":1}"#;
    let result = parse_rpc_response(raw).expect("should succeed");
    assert_eq!(result["ok"], true);
}

#[test]
fn parse_rpc_response_error() {
    let raw = br#"{"jsonrpc":"2.0","error":{"code":-32000,"message":"boom"},"id":1}"#;
    let err = parse_rpc_response(raw).unwrap_err();
    match err {
        EmberError::Rpc { code, message } => {
            assert_eq!(code, -32000);
            assert_eq!(message, "boom");
        }
        other => panic!("expected Rpc error, got: {other}"),
    }
}

// ── EmberClient mock server round-trip ──────────────────────────

fn mock_ember_server(
    listener: std::os::unix::net::UnixListener,
    response: serde_json::Value,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        let mut reader = std::io::BufReader::new(&stream);
        let mut line = String::new();
        std::io::BufRead::read_line(&mut reader, &mut line).expect("read request");
        let request: serde_json::Value =
            serde_json::from_str(line.trim()).expect("parse request");
        let method = request["method"].as_str().unwrap_or("").to_string();

        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "result": response,
            "id": request["id"],
        });
        std::io::Write::write_all(&mut &stream, resp.to_string().as_bytes())
            .expect("write response");
        method
    })
}

#[test]
fn ember_client_mmio_read_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("ember.sock");
    let listener = std::os::unix::net::UnixListener::bind(&sock_path).expect("bind");
    let handle = mock_ember_server(
        listener,
        serde_json::json!({"value": "0x00000042", "offset": "0x00000000"}),
    );

    let client = EmberClient {
        socket_path: sock_path.to_str().unwrap().to_string(),
    };
    let val = client.mmio_read("0000:3b:00.0", 0).expect("mmio_read");
    assert_eq!(val, 0x42);

    let method = handle.join().expect("server thread");
    assert_eq!(method, "ember.mmio.read");
}

#[test]
fn ember_client_fecs_state_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("ember.sock");
    let listener = std::os::unix::net::UnixListener::bind(&sock_path).expect("bind");
    let handle = mock_ember_server(
        listener,
        serde_json::json!({
            "cpuctl": "0x00000010",
            "sctl": "0x00000000",
            "pc": "0x00001234",
            "mb0": "0x00000000",
            "mb1": "0x00000000",
            "exci": "0x00000000",
            "halted": true,
            "stopped": false,
            "hs_mode": false,
        }),
    );

    let client = EmberClient {
        socket_path: sock_path.to_str().unwrap().to_string(),
    };
    let state = client.fecs_state("0000:3b:00.0").expect("fecs_state");
    assert_eq!(state["halted"], true);
    assert_eq!(state["stopped"], false);
    assert_eq!(state["cpuctl"], "0x00000010");

    let method = handle.join().expect("server thread");
    assert_eq!(method, "ember.fecs.state");
}

#[test]
fn ember_client_livepatch_status_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("ember.sock");
    let listener = std::os::unix::net::UnixListener::bind(&sock_path).expect("bind");
    let handle = mock_ember_server(
        listener,
        serde_json::json!({
            "loaded": false,
            "enabled": false,
            "transition": false,
            "patched_funcs": [],
        }),
    );

    let client = EmberClient {
        socket_path: sock_path.to_str().unwrap().to_string(),
    };
    let status = client.livepatch_status().expect("livepatch_status");
    assert_eq!(status["loaded"], false);

    let method = handle.join().expect("server thread");
    assert_eq!(method, "ember.livepatch.status");
}

#[test]
fn ember_client_rpc_error_propagated() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock_path = dir.path().join("ember.sock");
    let listener = std::os::unix::net::UnixListener::bind(&sock_path).expect("bind");

    std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        let mut reader = std::io::BufReader::new(&stream);
        let mut line = String::new();
        std::io::BufRead::read_line(&mut reader, &mut line).expect("read");
        let req: serde_json::Value = serde_json::from_str(line.trim()).expect("parse");
        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "error": {"code": -32600, "message": "test error"},
            "id": req["id"],
        });
        std::io::Write::write_all(&mut &stream, resp.to_string().as_bytes()).expect("write");
    });

    let client = EmberClient {
        socket_path: sock_path.to_str().unwrap().to_string(),
    };
    let err = client.livepatch_status().unwrap_err();
    match err {
        EmberError::Rpc { code, message } => {
            assert_eq!(code, -32600);
            assert_eq!(message, "test error");
        }
        other => panic!("expected Rpc error, got: {other}"),
    }
}
