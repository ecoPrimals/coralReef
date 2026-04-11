// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::hold::HeldDevice;
use crate::journal::{Journal, JournalEntry};

use super::handlers_journal;
use super::jsonrpc::{make_jsonrpc_ok, write_jsonrpc_error};
use super::{JsonRpcRequest, handle_client, send_with_fds};

static IPC_TEST_LOCK: Mutex<()> = Mutex::new(());

fn empty_held() -> Arc<RwLock<HashMap<String, HeldDevice>>> {
    Arc::new(RwLock::new(HashMap::new()))
}

fn managed(bdfs: &[&str]) -> HashSet<String> {
    bdfs.iter().map(|s| (*s).to_string()).collect()
}

const TEST_BDF: &str = "0000:01:00.0";
const BOGUS_BDF: &str = "9999:99:99.9";

fn first_json_line_bytes(buf: &[u8]) -> serde_json::Value {
    let s = std::str::from_utf8(buf).expect("utf8 response");
    let line = s.trim_end_matches('\n');
    serde_json::from_str(line).expect("one json line")
}

fn drain_json_line(stream: &mut UnixStream) -> serde_json::Value {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte).expect("read response byte");
        if byte[0] == b'\n' {
            break;
        }
        buf.push(byte[0]);
    }
    let s = std::str::from_utf8(&buf).expect("response is utf-8");
    serde_json::from_str(s).expect("drained line is json")
}

#[test]
fn handle_client_empty_read_returns_ok() {
    let (mut a, b) = UnixStream::pair().expect("unix stream pair");
    drop(b);
    let held = empty_held();
    let m = managed(&[]);
    handle_client(&mut a, &held, &m, Instant::now(), None).expect("handle_client completes");
    drop(a);
}

#[test]
fn handle_client_invalid_json_emits_parse_error() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    client
        .write_all(b"not json\n")
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[]);
    handle_client(&mut server, &held, &m, Instant::now(), None).expect("handle_client completes");
    let v = drain_json_line(&mut client);
    assert_eq!(
        v["error"]["code"].as_i64().expect("jsonrpc error code"),
        -32700
    );
}

#[test]
fn handle_client_wrong_jsonrpc_version() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req = r#"{"jsonrpc":"1.0","method":"ember.list","id":1}"#;
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[]);
    handle_client(&mut server, &held, &m, Instant::now(), None).expect("handle_client completes");
    let v = drain_json_line(&mut client);
    assert_eq!(
        v["error"]["code"].as_i64().expect("jsonrpc error code"),
        -32600
    );
}

#[test]
fn handle_client_ember_list_empty() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.list","id":7}"#;
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[]);
    handle_client(&mut server, &held, &m, Instant::now(), None).expect("handle_client completes");
    let v = drain_json_line(&mut client);
    assert_eq!(v["result"]["devices"], serde_json::json!([]));
}

#[test]
fn handle_client_ember_status_reports_uptime() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.status","id":2}"#;
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let started = Instant::now() - Duration::from_secs(10);
    let held = empty_held();
    let m = managed(&[]);
    handle_client(&mut server, &held, &m, started, None).expect("handle_client completes");
    let v = drain_json_line(&mut client);
    let uptime = v["result"]["uptime_secs"].as_u64().expect("uptime field");
    assert!(uptime >= 10);
}

#[test]
fn handle_client_unknown_method() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req = r#"{"jsonrpc":"2.0","method":"nope.not_found","id":3}"#;
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[]);
    handle_client(&mut server, &held, &m, Instant::now(), None).expect("handle_client completes");
    let v = drain_json_line(&mut client);
    assert_eq!(
        v["error"]["code"].as_i64().expect("jsonrpc error code"),
        -32601
    );
}

#[test]
fn handle_client_ember_vfio_fds_missing_device() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req =
        r#"{"jsonrpc":"2.0","method":"ember.vfio_fds","params":{"bdf":"0000:01:00.0"},"id":4}"#;
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[TEST_BDF]);
    handle_client(&mut server, &held, &m, Instant::now(), None).expect("handle_client completes");
    let v = drain_json_line(&mut client);
    assert_eq!(
        v["error"]["code"].as_i64().expect("jsonrpc error code"),
        -32000
    );
}

#[test]
fn handle_client_ember_vfio_fds_missing_bdf_errors() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.vfio_fds","params":{},"id":501}"#;
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[TEST_BDF]);
    let err = handle_client(&mut server, &held, &m, Instant::now(), None)
        .expect_err("handler returns error");
    assert!(err.to_string().contains("bdf"), "{err}");
}

#[test]
fn handle_client_ember_reacquire_missing_bdf_errors() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.reacquire","params":{},"id":502}"#;
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[TEST_BDF]);
    let err = handle_client(&mut server, &held, &m, Instant::now(), None)
        .expect_err("handler returns error");
    assert!(err.to_string().contains("bdf"), "{err}");
}

#[test]
fn handle_client_ember_release_missing_bdf_errors() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.release","params":{},"id":5}"#;
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[]);
    let err = handle_client(&mut server, &held, &m, Instant::now(), None)
        .expect_err("handler returns error");
    assert!(err.to_string().contains("bdf"));
}

#[test]
fn handle_client_ember_release_not_held() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req =
        r#"{"jsonrpc":"2.0","method":"ember.release","params":{"bdf":"0000:01:00.0"},"id":6}"#;
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[TEST_BDF]);
    handle_client(&mut server, &held, &m, Instant::now(), None).expect("handle_client completes");
    let v = drain_json_line(&mut client);
    assert_eq!(
        v["error"]["code"].as_i64().expect("jsonrpc error code"),
        -32000
    );
}

#[test]
fn handle_client_ember_swap_missing_params() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.swap","params":{"bdf":"0000:01:00.0"},"id":8}"#;
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[TEST_BDF]);
    let err = handle_client(&mut server, &held, &m, Instant::now(), None)
        .expect_err("handler returns error");
    assert!(err.to_string().contains("target"));
}

#[test]
fn handle_client_ember_reacquire_open_failure_returns_jsonrpc_error() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req =
        r#"{"jsonrpc":"2.0","method":"ember.reacquire","params":{"bdf":"9999:99:99.9"},"id":11}"#;
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[BOGUS_BDF]);
    handle_client(&mut server, &held, &m, Instant::now(), None).expect("handle_client completes");
    let v = drain_json_line(&mut client);
    assert_eq!(
        v["error"]["code"].as_i64().expect("jsonrpc error code"),
        -32000
    );
    let msg = v["error"]["message"]
        .as_str()
        .expect("jsonrpc error message");
    assert!(msg.contains("reacquire"));
}

#[test]
fn handle_client_ember_swap_unbound_success() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.swap","params":{"bdf":"9999:99:99.9","target":"unbound"},"id":42}"#;
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[BOGUS_BDF]);
    handle_client(&mut server, &held, &m, Instant::now(), None).expect("handle_client completes");
    let v = drain_json_line(&mut client);
    assert_eq!(v["result"]["to_personality"], "unbound");
    assert_eq!(v["id"], serde_json::json!(42));
}

#[test]
fn handle_client_non_utf8_request_errors() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    client
        .write_all(&[0xff, 0xfe, b'\n'])
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[]);
    let err = handle_client(&mut server, &held, &m, Instant::now(), None)
        .expect_err("handler returns error");
    let es = err.to_string();
    assert!(
        es.to_lowercase().contains("utf"),
        "expected UTF-8 decode in error: {es}"
    );
}

#[test]
fn send_with_fds_fails_when_peer_closed() {
    let (a, b) = UnixStream::pair().expect("unix stream pair");
    drop(b);
    let file = std::fs::File::open("/dev/null").expect("open /dev/null");
    let fds = [file.as_fd()];
    let err = send_with_fds(&a, b"{}", &fds).expect_err("broken pipe");
    assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
}

#[test]
fn handle_client_ember_swap_reports_error_from_swap() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.swap","params":{"bdf":"9999:99:99.9","target":"bogus-target"},"id":9}"#;
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[BOGUS_BDF]);
    handle_client(&mut server, &held, &m, Instant::now(), None).expect("handle_client completes");
    let v = drain_json_line(&mut client);
    assert_eq!(
        v["error"]["code"].as_i64().expect("jsonrpc error code"),
        -32000
    );
    let msg = v["error"]["message"]
        .as_str()
        .expect("jsonrpc error message");
    assert!(
        msg.contains("preflight") || msg.contains("unknown target"),
        "expected preflight or unknown target error, got: {msg}"
    );
}

#[test]
fn handle_client_swap_rejects_unmanaged_bdf() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.swap","params":{"bdf":"0000:ff:00.0","target":"vfio"},"id":99}"#;
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[TEST_BDF]);
    handle_client(&mut server, &held, &m, Instant::now(), None).expect("handle_client completes");
    let v = drain_json_line(&mut client);
    assert_eq!(
        v["error"]["code"].as_i64().expect("jsonrpc error code"),
        -32001
    );
    let msg = v["error"]["message"]
        .as_str()
        .expect("jsonrpc error message");
    assert!(msg.contains("not managed"), "{msg}");
}

#[test]
fn handle_client_reacquire_rejects_unmanaged_bdf() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req =
        r#"{"jsonrpc":"2.0","method":"ember.reacquire","params":{"bdf":"0000:ff:00.0"},"id":100}"#;
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[TEST_BDF]);
    handle_client(&mut server, &held, &m, Instant::now(), None).expect("handle_client completes");
    let v = drain_json_line(&mut client);
    assert_eq!(
        v["error"]["code"].as_i64().expect("jsonrpc error code"),
        -32001
    );
}

#[test]
fn handle_client_ember_vfio_fds_rejects_unmanaged_bdf() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc test lock");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req =
        r#"{"jsonrpc":"2.0","method":"ember.vfio_fds","params":{"bdf":"0000:ff:00.0"},"id":101}"#;
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let held = empty_held();
    let m = managed(&[TEST_BDF]);
    handle_client(&mut server, &held, &m, Instant::now(), None).expect("handle_client completes");
    let v = drain_json_line(&mut client);
    assert_eq!(
        v["error"]["code"].as_i64().expect("jsonrpc error code"),
        -32001
    );
    let msg = v["error"]["message"]
        .as_str()
        .expect("jsonrpc error message");
    assert!(msg.contains("not managed"), "{msg}");
}

#[test]
fn jsonrpc_request_deserializes_omitted_params_as_null() {
    let json = r#"{"jsonrpc":"2.0","method":"ember.list","id":1}"#;
    let req: JsonRpcRequest = serde_json::from_str(json).expect("deserialize JsonRpcRequest");
    assert_eq!(req.params, serde_json::Value::Null);
    assert_eq!(req.jsonrpc, "2.0");
    assert_eq!(req.method, "ember.list");
}

#[test]
fn write_jsonrpc_error_writes_line_client_can_parse() {
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    write_jsonrpc_error(
        &mut server,
        serde_json::json!("correlation-id"),
        -32000,
        "test failure message",
    )
    .expect("write_jsonrpc_error");
    let v = drain_json_line(&mut client);
    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["error"]["code"], -32000);
    assert_eq!(v["error"]["message"], "test failure message");
    assert_eq!(v["id"], serde_json::json!("correlation-id"));
}

#[test]
fn make_jsonrpc_ok_shape() {
    let v = make_jsonrpc_ok(serde_json::json!(1), serde_json::json!({"a": 1}));
    assert_eq!(v.jsonrpc, "2.0");
    assert!(v.error.is_none());
    assert_eq!(v.result, Some(serde_json::json!({"a": 1})));
}

#[test]
fn send_with_fds_unix_stream() {
    let (a, _b) = UnixStream::pair().expect("unix stream pair");
    let file = std::fs::File::open("/dev/null").expect("open /dev/null");
    let fds = [file.as_fd()];
    send_with_fds(&a, b"ok", &fds).expect("send_with_fds with /dev/null fd");
}

#[test]
#[ignore = "requires GPU bound to vfio-pci and a real BDF"]
fn handle_client_ember_vfio_fds_with_hardware() {
    let bdf = std::env::var("CORAL_EMBER_TEST_BDF").expect("set CORAL_EMBER_TEST_BDF");
    let (mut server, mut client) = UnixStream::pair().expect("unix stream pair");
    let req = format!(
        r#"{{"jsonrpc":"2.0","method":"ember.vfio_fds","params":{{"bdf":"{bdf}"}},"id":1}}"#
    );
    client
        .write_all(req.as_bytes())
        .expect("write request to test socket");
    client
        .write_all(b"\n")
        .expect("write request to test socket");
    let device = coral_driver::vfio::VfioDevice::open(&bdf).expect("open vfio device for hw test");
    let mut map = HashMap::new();
    map.insert(
        bdf.clone(),
        crate::hold::HeldDevice {
            bdf: bdf.clone(),
            device,
            ring_meta: crate::hold::RingMeta::default(),
            req_eventfd: None,
        },
    );
    let held = Arc::new(RwLock::new(map));
    let m = managed(&[&bdf]);
    handle_client(&mut server, &held, &m, Instant::now(), None).expect("handle_client completes");
}

#[test]
fn handlers_journal_query_without_journal_returns_not_available() {
    let mut buf = Vec::new();
    handlers_journal::query(
        &mut buf,
        serde_json::json!(901),
        &serde_json::json!({}),
        None,
    )
    .expect("handler writes JSON-RPC line");
    let v = first_json_line_bytes(&buf);
    assert_eq!(v["error"]["code"], -32000);
    let msg = v["error"]["message"].as_str().expect("message");
    assert!(
        msg.contains("journal not available"),
        "unexpected message: {msg}"
    );
}

#[test]
fn handlers_journal_stats_without_journal_returns_not_available() {
    let mut buf = Vec::new();
    handlers_journal::stats(
        &mut buf,
        serde_json::json!(902),
        &serde_json::json!({}),
        None,
    )
    .expect("handler writes JSON-RPC line");
    let v = first_json_line_bytes(&buf);
    assert_eq!(v["error"]["code"], -32000);
}

#[test]
fn handlers_journal_append_without_journal_returns_not_available() {
    let mut buf = Vec::new();
    handlers_journal::append(
        &mut buf,
        serde_json::json!(903),
        &serde_json::json!({}),
        None,
    )
    .expect("handler writes JSON-RPC line");
    let v = first_json_line_bytes(&buf);
    assert_eq!(v["error"]["code"], -32000);
}

#[test]
fn handlers_journal_query_empty_file_returns_empty_entries() {
    let tmp = tempfile::NamedTempFile::new().expect("temp journal");
    let journal = Arc::new(Journal::open(tmp.path()));
    let mut buf = Vec::new();
    handlers_journal::query(
        &mut buf,
        serde_json::json!(904),
        &serde_json::json!({}),
        Some(&journal),
    )
    .expect("handler writes JSON-RPC line");
    let v = first_json_line_bytes(&buf);
    assert_eq!(v["result"]["entries"], serde_json::json!([]));
}

#[test]
fn handlers_journal_append_rejects_malformed_entry() {
    let tmp = tempfile::NamedTempFile::new().expect("temp journal");
    let journal = Arc::new(Journal::open(tmp.path()));
    let mut buf = Vec::new();
    handlers_journal::append(
        &mut buf,
        serde_json::json!(905),
        &serde_json::json!({}),
        Some(&journal),
    )
    .expect("handler writes JSON-RPC line");
    let v = first_json_line_bytes(&buf);
    assert_eq!(v["error"]["code"], -32602);
    let msg = v["error"]["message"].as_str().expect("message");
    assert!(msg.contains("invalid journal entry"), "unexpected: {msg}");
}

#[test]
fn handlers_journal_append_boot_attempt_succeeds() {
    let tmp = tempfile::NamedTempFile::new().expect("temp journal");
    let journal = Arc::new(Journal::open(tmp.path()));
    let entry = JournalEntry::BootAttempt {
        bdf: "0000:01:00.0".into(),
        strategy: "unit-test".into(),
        success: true,
        sec2_exci: 0,
        fecs_pc: 0,
        gpccs_exci: 0,
        notes: vec![],
        timestamp_epoch_ms: 99,
    };
    let params = serde_json::to_value(&entry).expect("serialize entry");
    let mut buf = Vec::new();
    handlers_journal::append(&mut buf, serde_json::json!(906), &params, Some(&journal))
        .expect("handler writes JSON-RPC line");
    let v = first_json_line_bytes(&buf);
    assert_eq!(v["result"]["ok"], true);
}

// ---------------------------------------------------------------------------
// TCP IPC parity tests — exercises handle_client_tcp for coverage
// ---------------------------------------------------------------------------

#[test]
fn tcp_ember_status_returns_result() {
    let _lock = IPC_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let held = empty_held();
    let managed = managed(&[TEST_BDF]);
    let started = Instant::now();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let h = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        super::handle_client_tcp(&mut stream, &held, &managed, started, None).unwrap();
    });

    let mut client = std::net::TcpStream::connect(addr).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let req = serde_json::json!({
        "jsonrpc": "2.0", "method": "ember.list", "id": 1
    });
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    client.write_all(line.as_bytes()).unwrap();

    let mut buf = vec![0u8; 4096];
    std::thread::sleep(Duration::from_millis(50));
    let n = client.read(&mut buf).unwrap();
    let response = std::str::from_utf8(&buf[..n]).unwrap().trim();
    let v: serde_json::Value = serde_json::from_str(response).unwrap();
    assert!(v["result"]["devices"].is_array());

    h.join().unwrap();
}

#[test]
fn tcp_parse_error_returns_jsonrpc_error() {
    let _lock = IPC_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let held = empty_held();
    let managed = managed(&[]);
    let started = Instant::now();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let h = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        super::handle_client_tcp(&mut stream, &held, &managed, started, None).unwrap();
    });

    let mut client = std::net::TcpStream::connect(addr).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client.write_all(b"not valid json\n").unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();

    let mut buf = String::new();
    client.read_to_string(&mut buf).unwrap();
    let v: serde_json::Value = serde_json::from_str(buf.trim()).unwrap();
    assert!(v["error"]["code"].as_i64().is_some());

    h.join().unwrap();
}

#[test]
fn tcp_empty_request_returns_ok() {
    let _lock = IPC_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let held = empty_held();
    let managed = managed(&[]);
    let started = Instant::now();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let h = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        super::handle_client_tcp(&mut stream, &held, &managed, started, None).unwrap();
    });

    let client = std::net::TcpStream::connect(addr).unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();

    h.join().unwrap();
}
