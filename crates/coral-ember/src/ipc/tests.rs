// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::hold::HeldDevice;

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
    assert!(err.contains("bdf"), "{err}");
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
    assert!(err.contains("bdf"), "{err}");
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
    assert!(err.contains("bdf"));
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
    assert!(err.contains("target"));
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
    assert!(err.contains("utf8"), "{err}");
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
