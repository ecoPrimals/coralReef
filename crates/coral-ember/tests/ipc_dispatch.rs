// SPDX-License-Identifier: AGPL-3.0-only
//! Integration tests for JSON-RPC dispatch via the public `handle_client` API.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use coral_ember::{
    HeldDevice, JsonRpcError, JsonRpcRequest, JsonRpcResponse, handle_client, send_with_fds,
};

static IPC_TEST_LOCK: Mutex<()> = Mutex::new(());

const TEST_BDF: &str = "0000:01:00.0";
const BOGUS_BDF: &str = "9999:99:99.9";

fn empty_held() -> Arc<RwLock<HashMap<String, HeldDevice>>> {
    Arc::new(RwLock::new(HashMap::new()))
}

fn managed(bdfs: &[&str]) -> HashSet<String> {
    bdfs.iter().map(|s| (*s).to_string()).collect()
}

fn drain_json_line(stream: &mut UnixStream) -> serde_json::Value {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte).expect("read byte");
        if byte[0] == b'\n' {
            break;
        }
        buf.push(byte[0]);
    }
    let s = std::str::from_utf8(&buf).expect("utf8");
    serde_json::from_str(s).expect("json")
}

#[test]
fn dispatch_invalid_json_parse_error() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc lock");
    let (server, mut client) = UnixStream::pair().expect("pair");
    client.write_all(b"not json\n").expect("write");
    let held = empty_held();
    let m = managed(&[]);
    handle_client(&server, &held, &m, Instant::now()).expect("handler");
    let v = drain_json_line(&mut client);
    assert_eq!(v["error"]["code"].as_i64().expect("code"), -32700);
}

#[test]
fn dispatch_wrong_jsonrpc_version() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc lock");
    let (server, mut client) = UnixStream::pair().expect("pair");
    let req = r#"{"jsonrpc":"1.0","method":"ember.list","id":1}"#;
    client.write_all(req.as_bytes()).expect("write");
    client.write_all(b"\n").expect("newline");
    let held = empty_held();
    let m = managed(&[]);
    handle_client(&server, &held, &m, Instant::now()).expect("handler");
    let v = drain_json_line(&mut client);
    assert_eq!(v["error"]["code"].as_i64().expect("code"), -32600);
}

#[test]
fn dispatch_ember_list_empty() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc lock");
    let (server, mut client) = UnixStream::pair().expect("pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.list","id":7}"#;
    client.write_all(req.as_bytes()).expect("write");
    client.write_all(b"\n").expect("newline");
    let held = empty_held();
    let m = managed(&[]);
    handle_client(&server, &held, &m, Instant::now()).expect("handler");
    let v = drain_json_line(&mut client);
    assert_eq!(v["result"]["devices"], serde_json::json!([]));
}

#[test]
fn dispatch_ember_status_uptime() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc lock");
    let (server, mut client) = UnixStream::pair().expect("pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.status","id":2}"#;
    client.write_all(req.as_bytes()).expect("write");
    client.write_all(b"\n").expect("newline");
    let started = Instant::now() - Duration::from_secs(10);
    let held = empty_held();
    let m = managed(&[]);
    handle_client(&server, &held, &m, started).expect("handler");
    let v = drain_json_line(&mut client);
    let uptime = v["result"]["uptime_secs"].as_u64().expect("uptime");
    assert!(uptime >= 10);
    let devices = v["result"]["devices"].as_array().expect("devices array");
    assert!(devices.is_empty());
}

#[test]
fn dispatch_unknown_method() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc lock");
    let (server, mut client) = UnixStream::pair().expect("pair");
    let req = r#"{"jsonrpc":"2.0","method":"nope.not_found","id":3}"#;
    client.write_all(req.as_bytes()).expect("write");
    client.write_all(b"\n").expect("newline");
    let held = empty_held();
    let m = managed(&[]);
    handle_client(&server, &held, &m, Instant::now()).expect("handler");
    let v = drain_json_line(&mut client);
    assert_eq!(v["error"]["code"].as_i64().expect("code"), -32601);
}

#[test]
fn dispatch_ember_vfio_fds_missing_device() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc lock");
    let (server, mut client) = UnixStream::pair().expect("pair");
    let req =
        r#"{"jsonrpc":"2.0","method":"ember.vfio_fds","params":{"bdf":"0000:01:00.0"},"id":4}"#;
    client.write_all(req.as_bytes()).expect("write");
    client.write_all(b"\n").expect("newline");
    let held = empty_held();
    let m = managed(&[TEST_BDF]);
    handle_client(&server, &held, &m, Instant::now()).expect("handler");
    let v = drain_json_line(&mut client);
    assert_eq!(v["error"]["code"].as_i64().expect("code"), -32000);
}

#[test]
fn dispatch_ember_release_missing_bdf_errors() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc lock");
    let (server, mut client) = UnixStream::pair().expect("pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.release","params":{},"id":5}"#;
    client.write_all(req.as_bytes()).expect("write");
    client.write_all(b"\n").expect("newline");
    let held = empty_held();
    let m = managed(&[]);
    let err = handle_client(&server, &held, &m, Instant::now()).expect_err("missing bdf");
    assert!(err.contains("bdf"));
}

#[test]
fn dispatch_ember_release_not_held() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc lock");
    let (server, mut client) = UnixStream::pair().expect("pair");
    let req =
        r#"{"jsonrpc":"2.0","method":"ember.release","params":{"bdf":"0000:01:00.0"},"id":6}"#;
    client.write_all(req.as_bytes()).expect("write");
    client.write_all(b"\n").expect("newline");
    let held = empty_held();
    let m = managed(&[TEST_BDF]);
    handle_client(&server, &held, &m, Instant::now()).expect("handler");
    let v = drain_json_line(&mut client);
    assert_eq!(v["error"]["code"].as_i64().expect("code"), -32000);
}

#[test]
fn dispatch_ember_reacquire_open_failure() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc lock");
    let (server, mut client) = UnixStream::pair().expect("pair");
    let req =
        r#"{"jsonrpc":"2.0","method":"ember.reacquire","params":{"bdf":"9999:99:99.9"},"id":11}"#;
    client.write_all(req.as_bytes()).expect("write");
    client.write_all(b"\n").expect("newline");
    let held = empty_held();
    let m = managed(&[BOGUS_BDF]);
    handle_client(&server, &held, &m, Instant::now()).expect("handler");
    let v = drain_json_line(&mut client);
    assert_eq!(v["error"]["code"].as_i64().expect("code"), -32000);
    let msg = v["error"]["message"].as_str().expect("msg");
    assert!(msg.contains("reacquire"));
}

#[test]
fn dispatch_ember_swap_missing_target() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc lock");
    let (server, mut client) = UnixStream::pair().expect("pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.swap","params":{"bdf":"0000:01:00.0"},"id":8}"#;
    client.write_all(req.as_bytes()).expect("write");
    client.write_all(b"\n").expect("newline");
    let held = empty_held();
    let m = managed(&[]);
    let err = handle_client(&server, &held, &m, Instant::now()).expect_err("missing target");
    assert!(err.contains("target"));
}

#[test]
fn dispatch_ember_swap_unknown_target() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc lock");
    let (server, mut client) = UnixStream::pair().expect("pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.swap","params":{"bdf":"9999:99:99.9","target":"bogus-target"},"id":9}"#;
    client.write_all(req.as_bytes()).expect("write");
    client.write_all(b"\n").expect("newline");
    let held = empty_held();
    let m = managed(&[BOGUS_BDF]);
    handle_client(&server, &held, &m, Instant::now()).expect("handler");
    let v = drain_json_line(&mut client);
    assert_eq!(v["error"]["code"].as_i64().expect("code"), -32000);
    let msg = v["error"]["message"].as_str().expect("msg");
    assert!(
        msg.contains("preflight") || msg.contains("unknown target"),
        "expected preflight or unknown target error, got: {msg}"
    );
}

#[test]
fn dispatch_ember_vfio_fds_missing_bdf_param() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc lock");
    let (server, mut client) = UnixStream::pair().expect("pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.vfio_fds","params":{},"id":10}"#;
    client.write_all(req.as_bytes()).expect("write");
    client.write_all(b"\n").expect("newline");
    let held = empty_held();
    let m = managed(&[]);
    let err = handle_client(&server, &held, &m, Instant::now()).expect_err("missing bdf");
    assert!(err.contains("bdf"));
}

#[test]
fn dispatch_ember_reacquire_missing_bdf_param() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc lock");
    let (server, mut client) = UnixStream::pair().expect("pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.reacquire","params":{},"id":12}"#;
    client.write_all(req.as_bytes()).expect("write");
    client.write_all(b"\n").expect("newline");
    let held = empty_held();
    let m = managed(&[]);
    let err = handle_client(&server, &held, &m, Instant::now()).expect_err("missing bdf");
    assert!(err.contains("bdf"));
}

#[test]
fn dispatch_ember_swap_missing_bdf_param() {
    let _guard = IPC_TEST_LOCK.lock().expect("ipc lock");
    let (server, mut client) = UnixStream::pair().expect("pair");
    let req = r#"{"jsonrpc":"2.0","method":"ember.swap","params":{"target":"unbound"},"id":13}"#;
    client.write_all(req.as_bytes()).expect("write");
    client.write_all(b"\n").expect("newline");
    let held = empty_held();
    let m = managed(&[]);
    let err = handle_client(&server, &held, &m, Instant::now()).expect_err("missing bdf");
    assert!(err.contains("bdf"));
}

#[test]
fn jsonrpc_request_roundtrip() {
    let json = r#"{"jsonrpc":"2.0","method":"ember.list","params":{},"id":"a"}"#;
    let req: JsonRpcRequest = serde_json::from_str(json).expect("deserialize");
    assert_eq!(req.jsonrpc, "2.0");
    assert_eq!(req.method, "ember.list");
    assert_eq!(req.id, serde_json::json!("a"));
}

#[test]
fn jsonrpc_response_serializes_error_branch() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0",
        result: None,
        error: Some(JsonRpcError {
            code: -32000,
            message: "x".to_string(),
        }),
        id: serde_json::json!(1),
    };
    let s = serde_json::to_string(&resp).expect("serialize");
    assert!(s.contains("-32000"));
    assert!(!s.contains("\"result\""));
}

#[test]
fn send_with_fds_accepts_unix_stream_and_dev_null_fd() {
    let (a, _b) = UnixStream::pair().expect("pair");
    let file = std::fs::File::open("/dev/null").expect("open /dev/null");
    let fds = [file.as_fd()];
    send_with_fds(&a, b"payload", &fds).expect("send_with_fds");
}

#[test]
#[ignore = "requires GPU bound to vfio-pci and a real BDF"]
fn dispatch_ember_vfio_fds_with_hardware() {
    let bdf = std::env::var("CORAL_EMBER_TEST_BDF").expect("set CORAL_EMBER_TEST_BDF");
    let (server, mut client) = UnixStream::pair().expect("pair");
    let req = format!(
        r#"{{"jsonrpc":"2.0","method":"ember.vfio_fds","params":{{"bdf":"{bdf}"}},"id":1}}"#
    );
    client.write_all(req.as_bytes()).expect("write");
    client.write_all(b"\n").expect("newline");
    let device = coral_driver::vfio::VfioDevice::open(&bdf).expect("open");
    let mut map = HashMap::new();
    map.insert(bdf.clone(), HeldDevice { bdf: bdf.clone(), device });
    let held = Arc::new(RwLock::new(map));
    let m = managed(&[&bdf]);
    handle_client(&server, &held, &m, Instant::now()).expect("handler");
}

#[test]
#[ignore = "requires GPU bound to vfio-pci and a real BDF"]
fn dispatch_ember_release_success_when_held() {
    let bdf = std::env::var("CORAL_EMBER_TEST_BDF").expect("set CORAL_EMBER_TEST_BDF");
    let _guard = IPC_TEST_LOCK.lock().expect("ipc lock");
    let (server, mut client) = UnixStream::pair().expect("pair");
    let device = coral_driver::vfio::VfioDevice::open(&bdf).expect("open");
    let mut map = HashMap::new();
    map.insert(
        bdf.clone(),
        HeldDevice {
            bdf: bdf.clone(),
            device,
        },
    );
    let held = Arc::new(RwLock::new(map));
    let m = managed(&[&bdf]);
    let req = format!(
        r#"{{"jsonrpc":"2.0","method":"ember.release","params":{{"bdf":"{bdf}"}},"id":2}}"#
    );
    client.write_all(req.as_bytes()).expect("write");
    client.write_all(b"\n").expect("newline");
    handle_client(&server, &held, &m, Instant::now()).expect("handler");
    let v = drain_json_line(&mut client);
    assert_eq!(v["result"]["bdf"].as_str().expect("bdf"), bdf.as_str());
    assert!(held.read().unwrap().is_empty());
}

#[test]
#[ignore = "requires GPU bound to vfio-pci and a real BDF"]
fn dispatch_ember_reacquire_skips_open_when_already_held() {
    let bdf = std::env::var("CORAL_EMBER_TEST_BDF").expect("set CORAL_EMBER_TEST_BDF");
    let _guard = IPC_TEST_LOCK.lock().expect("ipc lock");
    let (server, mut client) = UnixStream::pair().expect("pair");
    let device = coral_driver::vfio::VfioDevice::open(&bdf).expect("open");
    let mut map = HashMap::new();
    map.insert(
        bdf.clone(),
        HeldDevice {
            bdf: bdf.clone(),
            device,
        },
    );
    let held = Arc::new(RwLock::new(map));
    let m = managed(&[&bdf]);
    let req = format!(
        r#"{{"jsonrpc":"2.0","method":"ember.reacquire","params":{{"bdf":"{bdf}"}},"id":3}}"#
    );
    client.write_all(req.as_bytes()).expect("write");
    client.write_all(b"\n").expect("newline");
    handle_client(&server, &held, &m, Instant::now()).expect("handler");
    let v = drain_json_line(&mut client);
    assert_eq!(v["result"]["bdf"].as_str().expect("bdf"), bdf.as_str());
    assert_eq!(held.read().unwrap().len(), 1);
}
