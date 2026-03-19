// SPDX-License-Identifier: AGPL-3.0-only
//! Tests for the JSON-RPC socket server.

use super::*;
use std::sync::Arc;
use tokio::sync::Mutex;

#[test]
fn test_jsonrpc_request_parse_valid() {
    let line = r#"{"jsonrpc":"2.0","method":"health.check","params":{},"id":1}"#;
    let result: Result<JsonRpcRequest, _> = serde_json::from_str(line);
    let req = match result {
        Ok(r) => r,
        Err(e) => panic!("expected valid JSON-RPC request: {e}"),
    };
    assert_eq!(req.jsonrpc, "2.0");
    assert_eq!(req.method, "health.check");
    assert!(req.params.is_object());
    assert_eq!(req.id, serde_json::json!(1));
}

#[test]
fn test_jsonrpc_request_parse_with_params() {
    let line =
        r#"{"jsonrpc":"2.0","method":"device.get","params":{"bdf":"0000:01:00.0"},"id":"req-1"}"#;
    let result: Result<JsonRpcRequest, _> = serde_json::from_str(line);
    let req = match result {
        Ok(r) => r,
        Err(e) => panic!("expected valid JSON-RPC request: {e}"),
    };
    assert_eq!(req.method, "device.get");
    let bdf = req.params.get("bdf").and_then(serde_json::Value::as_str);
    assert_eq!(bdf, Some("0000:01:00.0"));
}

#[test]
fn test_jsonrpc_request_parse_default_params() {
    let line = r#"{"jsonrpc":"2.0","method":"daemon.status","id":null}"#;
    let result: Result<JsonRpcRequest, _> = serde_json::from_str(line);
    let req = match result {
        Ok(r) => r,
        Err(e) => panic!("expected valid JSON-RPC request: {e}"),
    };
    assert_eq!(req.method, "daemon.status");
    assert!(req.params.is_null() || req.params.is_object());
}

#[test]
fn test_jsonrpc_request_parse_invalid() {
    let line = r#"{"jsonrpc":"2.0","method":123}"#;
    let result: Result<JsonRpcRequest, _> = serde_json::from_str(line);
    assert!(result.is_err());
}

#[test]
fn test_device_info_serialization_roundtrip() {
    let info = DeviceInfo {
        bdf: "0000:01:00.0".into(),
        name: Some("Compute GPU".into()),
        chip: "GV100 (Titan V)".into(),
        vendor_id: 0x10de,
        device_id: 0x1d81,
        personality: "vfio (group 5)".into(),
        role: Some("compute".into()),
        power: "D0".into(),
        vram_alive: true,
        domains_alive: 8,
        domains_faulted: 0,
        has_vfio_fd: true,
        pci_link_width: Some(16),
    };
    let json = match serde_json::to_string(&info) {
        Ok(j) => j,
        Err(e) => panic!("serialize DeviceInfo: {e}"),
    };
    let parsed: DeviceInfo = match serde_json::from_str(&json) {
        Ok(p) => p,
        Err(e) => panic!("deserialize DeviceInfo: {e}"),
    };
    assert_eq!(parsed.bdf, info.bdf);
    assert_eq!(parsed.name, info.name);
    assert_eq!(parsed.chip, info.chip);
    assert_eq!(parsed.vendor_id, info.vendor_id);
    assert_eq!(parsed.device_id, info.device_id);
    assert_eq!(parsed.personality, info.personality);
    assert_eq!(parsed.power, info.power);
    assert_eq!(parsed.vram_alive, info.vram_alive);
    assert_eq!(parsed.domains_alive, info.domains_alive);
    assert_eq!(parsed.domains_faulted, info.domains_faulted);
    assert_eq!(parsed.has_vfio_fd, info.has_vfio_fd);
    assert_eq!(parsed.pci_link_width, info.pci_link_width);
}

#[test]
fn test_health_info_serialization_roundtrip() {
    let info = HealthInfo {
        bdf: "0000:02:00.0".into(),
        boot0: 0x1234_5678,
        pmc_enable: 0x9abc_def0,
        vram_alive: true,
        power: "D3hot".into(),
        domains_alive: 7,
        domains_faulted: 1,
    };
    let json = match serde_json::to_string(&info) {
        Ok(j) => j,
        Err(e) => panic!("serialize HealthInfo: {e}"),
    };
    let parsed: HealthInfo = match serde_json::from_str(&json) {
        Ok(p) => p,
        Err(e) => panic!("deserialize HealthInfo: {e}"),
    };
    assert_eq!(parsed.bdf, info.bdf);
    assert_eq!(parsed.boot0, info.boot0);
    assert_eq!(parsed.pmc_enable, info.pmc_enable);
    assert_eq!(parsed.vram_alive, info.vram_alive);
    assert_eq!(parsed.power, info.power);
    assert_eq!(parsed.domains_alive, info.domains_alive);
    assert_eq!(parsed.domains_faulted, info.domains_faulted);
}

#[test]
fn test_health_check_response_format() {
    let response = serde_json::json!({
        "alive": true,
        "name": "coral-glowplug",
        "device_count": 0,
        "healthy_count": 0
    });
    assert_eq!(response["alive"], true);
    assert_eq!(response["name"], "coral-glowplug");
    assert_eq!(response["device_count"], 0);
    assert_eq!(response["healthy_count"], 0);
}

#[test]
fn test_dispatch_device_list_empty() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch("device.list", &serde_json::json!({}), &mut devices, started);
    let val = result.expect("device.list should succeed");
    let arr = val.as_array().expect("should be array");
    assert!(arr.is_empty());
}

#[test]
fn test_dispatch_device_list_with_devices() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: Some("Test GPU".into()),
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        role: Some("compute".into()),
        oracle_dump: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch("device.list", &serde_json::json!({}), &mut devices, started);
    let val = result.expect("device.list should succeed");
    let arr = val.as_array().expect("should be array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["bdf"], "0000:99:00.0");
    assert_eq!(arr[0]["name"], "Test GPU");
}

#[test]
fn test_dispatch_device_get_found() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: Some("Test".into()),
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        role: None,
        oracle_dump: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.get",
        &serde_json::json!({"bdf": "0000:99:00.0"}),
        &mut devices,
        started,
    );
    let val = result.expect("device.get should succeed");
    assert_eq!(val["bdf"], "0000:99:00.0");
}

#[test]
fn test_dispatch_device_get_missing_bdf() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch("device.get", &serde_json::json!({}), &mut devices, started);
    let err = result.expect_err("device.get without bdf should fail");
    assert_eq!(i32::from(err.code), -32602);
    assert!(err.message.contains("bdf"));
}

#[test]
fn test_dispatch_device_get_not_managed() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.get",
        &serde_json::json!({"bdf": "0000:01:00.0"}),
        &mut devices,
        started,
    );
    let err = result.expect_err("device.get for unmanaged device should fail");
    assert_eq!(i32::from(err.code), -32000);
    assert!(err.message.contains("not managed"));
}

#[test]
fn test_dispatch_health_check() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "health.check",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let val = result.expect("health.check should succeed");
    assert_eq!(val["alive"], true);
    assert_eq!(val["name"], "coral-glowplug");
    assert_eq!(val["device_count"], 0);
}

#[test]
fn test_dispatch_health_liveness() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "health.liveness",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let val = result.expect("health.liveness should succeed");
    assert_eq!(val["alive"], true);
}

#[test]
fn test_dispatch_daemon_status() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "daemon.status",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let val = result.expect("daemon.status should succeed");
    assert!(val["uptime_secs"].as_u64().is_some());
    assert_eq!(val["device_count"], 0);
}

#[test]
fn test_dispatch_daemon_shutdown_returns_error() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "daemon.shutdown",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let err = result.expect_err("daemon.shutdown should return Err for shutdown signal");
    assert_eq!(i32::from(err.code), -32000);
    assert_eq!(err.message, "shutdown");
}

#[test]
fn test_dispatch_unknown_method() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "nonexistent.method",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let err = result.expect_err("unknown method should fail");
    assert_eq!(i32::from(err.code), -32601);
    assert!(err.message.contains("method not found"));
}

#[test]
fn test_dispatch_device_swap_missing_params() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch("device.swap", &serde_json::json!({}), &mut devices, started);
    let err = result.expect_err("device.swap without params should fail");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn test_dispatch_device_health() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        role: None,
        oracle_dump: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.health",
        &serde_json::json!({"bdf": "0000:99:00.0"}),
        &mut devices,
        started,
    );
    let val = result.expect("device.health should succeed");
    assert_eq!(val["bdf"], "0000:99:00.0");
    assert!(val.get("vram_alive").is_some());
    assert!(val.get("domains_alive").is_some());
}

#[test]
fn test_make_response_success() {
    let resp = make_response(serde_json::json!(1), Ok(serde_json::json!({"ok": true})));
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["result"]["ok"], true);
    assert_eq!(parsed["id"], 1);
}

#[test]
fn test_make_response_error() {
    let resp = make_response(
        serde_json::json!("req-1"),
        Err(coral_glowplug::error::RpcError::invalid_params("missing parameter")),
    );
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert!(parsed["error"].is_object());
    assert_eq!(parsed["error"]["code"], -32602);
    assert_eq!(parsed["error"]["message"], "missing parameter");
}

#[tokio::test]
async fn test_tcp_bind_127_0_0_1_0() {
    let server = SocketServer::bind("127.0.0.1:0")
        .await
        .expect("TCP bind should succeed");
    let addr = server.bound_addr();
    assert!(addr.contains("127.0.0.1"));
    assert!(addr.contains(':'));
    // Port 0 means OS assigns; we should get a non-zero port
    let port_part: &str = addr.rsplit(':').next().unwrap_or("");
    let port: u16 = port_part.parse().expect("port should parse");
    assert!(port > 0, "OS should assign non-zero port");
}

#[tokio::test]
async fn test_tcp_client_health_check() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    let server = SocketServer::bind("127.0.0.1:0").await.expect("bind");
    let addr = server.bound_addr();
    let devices = Arc::new(Mutex::new(Vec::<coral_glowplug::device::DeviceSlot>::new()));
    let (_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let devices_clone = devices.clone();

    let handle = tokio::spawn(async move {
        server.accept_loop(devices_clone, &mut shutdown_rx).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream = TcpStream::connect(&addr).await.expect("connect");
    let (reader, mut writer) = stream.into_split();

    let req = r#"{"jsonrpc":"2.0","method":"health.check","params":{},"id":1}"#;
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .expect("write");

    let mut lines = BufReader::new(reader).lines();
    let resp_line = lines.next_line().await.expect("read").expect("line");
    let resp: serde_json::Value = serde_json::from_str(&resp_line).expect("parse");

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["result"]["alive"], true);
    assert_eq!(resp["result"]["name"], "coral-glowplug");
    assert_eq!(resp["id"], 1);

    handle.abort();
}

#[tokio::test]
async fn test_invalid_jsonrpc_version() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    let server = SocketServer::bind("127.0.0.1:0").await.expect("bind");
    let addr = server.bound_addr();
    let devices = Arc::new(Mutex::new(Vec::<coral_glowplug::device::DeviceSlot>::new()));
    let (_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let devices_clone = devices.clone();

    let handle = tokio::spawn(async move {
        server.accept_loop(devices_clone, &mut shutdown_rx).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream = TcpStream::connect(&addr).await.expect("connect");
    let (reader, mut writer) = stream.into_split();

    let req = r#"{"jsonrpc":"1.0","method":"health.check","params":{},"id":1}"#;
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .expect("write");

    let mut lines = BufReader::new(reader).lines();
    let resp_line = lines.next_line().await.expect("read").expect("line");
    let resp: serde_json::Value = serde_json::from_str(&resp_line).expect("parse");

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["error"]["code"], -32600);
    assert_eq!(resp["id"], 1);

    handle.abort();
}

#[tokio::test]
async fn test_parse_error() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    let server = SocketServer::bind("127.0.0.1:0").await.expect("bind");
    let addr = server.bound_addr();
    let devices = Arc::new(Mutex::new(Vec::<coral_glowplug::device::DeviceSlot>::new()));
    let (_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let devices_clone = devices.clone();

    let handle = tokio::spawn(async move {
        server.accept_loop(devices_clone, &mut shutdown_rx).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream = TcpStream::connect(&addr).await.expect("connect");
    let (reader, mut writer) = stream.into_split();

    writer.write_all(b"not valid json\n").await.expect("write");

    let mut lines = BufReader::new(reader).lines();
    let resp_line = lines.next_line().await.expect("read").expect("line");
    let resp: serde_json::Value = serde_json::from_str(&resp_line).expect("parse");

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["error"]["code"], -32700);
    assert_eq!(resp["id"], serde_json::Value::Null);

    handle.abort();
}

#[tokio::test]
async fn test_daemon_shutdown_via_jsonrpc() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    let server = SocketServer::bind("127.0.0.1:0").await.expect("bind");
    let addr = server.bound_addr();
    let devices = Arc::new(Mutex::new(Vec::<coral_glowplug::device::DeviceSlot>::new()));
    let (_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let devices_clone = devices.clone();

    let handle = tokio::spawn(async move {
        server.accept_loop(devices_clone, &mut shutdown_rx).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream = TcpStream::connect(&addr).await.expect("connect");
    let (reader, mut writer) = stream.into_split();

    let req = r#"{"jsonrpc":"2.0","method":"daemon.shutdown","params":{},"id":1}"#;
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .expect("write");
    writer.flush().await.expect("flush");

    let mut lines = BufReader::new(reader).lines();
    let resp_line = lines.next_line().await.expect("read").expect("line");
    let resp: serde_json::Value = serde_json::from_str(&resp_line).expect("parse");

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["result"]["ok"], true);
    assert_eq!(resp["id"], 1);

    // Connection should close cleanly; next read should return None (EOF)
    let next = lines.next_line().await.expect("read");
    assert!(
        next.is_none(),
        "connection should close after shutdown response"
    );

    handle.abort();
}

// ── Lend / Reclaim ────────────────────────────────────────────────────

#[test]
fn test_dispatch_device_lend_missing_bdf() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch("device.lend", &serde_json::json!({}), &mut devices, started);
    let err = result.expect_err("device.lend without bdf should fail");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn test_dispatch_device_lend_not_managed() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.lend",
        &serde_json::json!({"bdf": "0000:01:00.0"}),
        &mut devices,
        started,
    );
    let err = result.expect_err("device.lend for unmanaged device should fail");
    assert_eq!(i32::from(err.code), -32000);
}

#[test]
fn test_dispatch_device_lend_not_vfio() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        role: None,
        oracle_dump: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.lend",
        &serde_json::json!({"bdf": "0000:99:00.0"}),
        &mut devices,
        started,
    );
    let err = result.expect_err("device.lend on unbound personality should fail");
    assert_eq!(i32::from(err.code), -32000);
}

#[test]
fn test_dispatch_device_reclaim_missing_bdf() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.reclaim",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let err = result.expect_err("device.reclaim without bdf should fail");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn test_dispatch_device_reclaim_not_managed() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.reclaim",
        &serde_json::json!({"bdf": "0000:01:00.0"}),
        &mut devices,
        started,
    );
    let err = result.expect_err("device.reclaim for unmanaged device should fail");
    assert_eq!(i32::from(err.code), -32000);
}

// ── Chaos / Fault / Penetration Tests ──────────────────────────────────

/// Helper: start a server on TCP loopback and return (addr, `abort_handle`, _`tx_keepalive`).
///
/// The returned `watch::Sender` must be held alive for the server to
/// keep accepting — dropping it signals shutdown via the watch channel.
async fn spawn_test_server() -> (
    String,
    tokio::task::JoinHandle<()>,
    tokio::sync::watch::Sender<bool>,
) {
    let server = SocketServer::bind("127.0.0.1:0")
        .await
        .expect("bind should succeed");
    let addr = server.bound_addr();
    let devices = Arc::new(Mutex::new(Vec::<coral_glowplug::device::DeviceSlot>::new()));
    let (tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let mut rx = shutdown_rx.clone();
    let handle = tokio::spawn(async move {
        server.accept_loop(devices, &mut rx).await;
    });
    for _ in 0..50 {
        if tokio::net::TcpStream::connect(&addr).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    (addr, handle, tx)
}

async fn send_line(addr: &str, payload: &str) -> String {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;
    let stream = tokio::time::timeout(std::time::Duration::from_secs(2), TcpStream::connect(addr))
        .await
        .expect("connect timeout")
        .expect("connect");
    let (reader, mut writer) = stream.into_split();
    writer
        .write_all(format!("{payload}\n").as_bytes())
        .await
        .expect("write");
    let mut lines = BufReader::new(reader).lines();
    tokio::time::timeout(std::time::Duration::from_secs(2), lines.next_line())
        .await
        .expect("read timeout")
        .expect("read")
        .unwrap_or_default()
}

// ── BDF Injection / Path Traversal ────────────────────────

#[test]
fn test_validate_bdf_valid() {
    assert!(validate_bdf("0000:01:00.0").is_ok());
    assert!(validate_bdf("0000:4a:00.0").is_ok());
}

#[test]
fn test_validate_bdf_rejects_path_traversal() {
    assert!(validate_bdf("../../../etc/shadow").is_err());
    assert!(validate_bdf("0000:01:00.0/../../root").is_err());
}

#[test]
fn test_validate_bdf_rejects_null_bytes() {
    assert!(validate_bdf("0000:01:00.0\0rm -rf /").is_err());
}

#[test]
fn test_validate_bdf_rejects_empty() {
    assert!(validate_bdf("").is_err());
}

#[test]
fn test_validate_bdf_rejects_overlong() {
    let long = "0".repeat(100);
    assert!(validate_bdf(&long).is_err());
}

#[test]
fn test_validate_bdf_rejects_shell_injection() {
    assert!(validate_bdf("0000:01:00.0; rm -rf /").is_err());
    assert!(validate_bdf("$(cat /etc/passwd)").is_err());
    assert!(validate_bdf("`whoami`").is_err());
}

#[test]
fn test_validate_bdf_rejects_unicode() {
    assert!(validate_bdf("0000:01:00.0\u{200B}").is_err());
    assert!(validate_bdf("０000:01:00.0").is_err()); // fullwidth zero
}

#[tokio::test]
async fn test_dispatch_bdf_traversal_rejected() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.get",
        &serde_json::json!({"bdf": "../../../etc/shadow"}),
        &mut devices,
        started,
    );
    let err = result.expect_err("traversal should be rejected");
    assert_eq!(i32::from(err.code), -32602);
    assert!(err.message.contains("invalid BDF"));
}

#[tokio::test]
async fn test_dispatch_bdf_null_byte_rejected() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.health",
        &serde_json::json!({"bdf": "0000:01:00.0\u{0000}"}),
        &mut devices,
        started,
    );
    let err = result.expect_err("null byte should be rejected");
    assert_eq!(i32::from(err.code), -32602);
}

// ── Malformed JSON-RPC Fuzzing ────────────────────────────

#[tokio::test]
async fn test_fuzz_empty_object() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let resp = send_line(&addr, "{}").await;
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
    assert!(v["error"].is_object(), "empty object should be an error");
    handle.abort();
}

#[tokio::test]
async fn test_fuzz_null_payload() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let resp = send_line(&addr, "null").await;
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
    assert_eq!(v["error"]["code"], -32700);
    handle.abort();
}

#[tokio::test]
async fn test_fuzz_array_payload() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let resp = send_line(&addr, "[1,2,3]").await;
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
    assert_eq!(v["error"]["code"], -32700);
    handle.abort();
}

#[tokio::test]
async fn test_fuzz_deeply_nested_json() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let nested = "{".repeat(100) + &"}".repeat(100);
    let resp = send_line(&addr, &nested).await;
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
    assert!(v["error"].is_object());
    handle.abort();
}

#[tokio::test]
async fn test_fuzz_huge_string_value() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let big_method = "x".repeat(10_000);
    let payload = format!(r#"{{"jsonrpc":"2.0","method":"{big_method}","params":{{}},"id":1}}"#);
    let resp = send_line(&addr, &payload).await;
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
    assert!(v["error"].is_object());
    assert_eq!(v["error"]["code"], -32601);
    handle.abort();
}

#[tokio::test]
async fn test_fuzz_numeric_method() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let resp = send_line(
        &addr,
        r#"{"jsonrpc":"2.0","method":12345,"params":{},"id":1}"#,
    )
    .await;
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
    assert_eq!(v["error"]["code"], -32700);
    handle.abort();
}

#[tokio::test]
async fn test_fuzz_negative_id() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let resp = send_line(
        &addr,
        r#"{"jsonrpc":"2.0","method":"health.check","params":{},"id":-999}"#,
    )
    .await;
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
    assert_eq!(v["id"], -999);
    assert!(v["result"]["alive"].as_bool().unwrap_or(false));
    handle.abort();
}

// ── Connection Chaos ──────────────────────────────────────

#[tokio::test]
async fn test_chaos_connect_and_disconnect_immediately() {
    let (addr, handle, _tx) = spawn_test_server().await;
    for _ in 0..20 {
        let stream = tokio::net::TcpStream::connect(&addr)
            .await
            .expect("connect");
        drop(stream);
    }
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Server should still accept new requests
    let resp = send_line(
        &addr,
        r#"{"jsonrpc":"2.0","method":"health.liveness","params":{},"id":1}"#,
    )
    .await;
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
    assert_eq!(v["result"]["alive"], true);
    handle.abort();
}

#[tokio::test]
async fn test_chaos_partial_write_then_disconnect() {
    let (addr, handle, _tx) = spawn_test_server().await;
    for _ in 0..10 {
        let mut stream = tokio::net::TcpStream::connect(&addr)
            .await
            .expect("connect");
        use tokio::io::AsyncWriteExt;
        let _ = stream.write_all(b"{\"jsonrpc\":\"2.0\",\"met").await;
        drop(stream);
    }
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let resp = send_line(
        &addr,
        r#"{"jsonrpc":"2.0","method":"health.check","params":{},"id":1}"#,
    )
    .await;
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
    assert_eq!(v["result"]["alive"], true);
    handle.abort();
}

#[tokio::test]
async fn test_chaos_rapid_sequential_requests() {
    let (addr, handle, _tx) = spawn_test_server().await;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let stream = tokio::net::TcpStream::connect(&addr)
        .await
        .expect("connect");
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    for i in 0..50 {
        let req =
            format!(r#"{{"jsonrpc":"2.0","method":"health.liveness","params":{{}},"id":{i}}}"#);
        writer
            .write_all(format!("{req}\n").as_bytes())
            .await
            .expect("write");
    }

    for i in 0..50 {
        let resp_line = lines.next_line().await.expect("read").expect("line");
        let v: serde_json::Value = serde_json::from_str(&resp_line).expect("valid json");
        assert_eq!(v["id"], i);
        assert_eq!(v["result"]["alive"], true);
    }
    handle.abort();
}

#[tokio::test]
async fn test_chaos_concurrent_connections() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let mut tasks = Vec::new();
    for i in 0..20 {
        let addr = addr.clone();
        tasks.push(tokio::spawn(async move {
            let resp = send_line(
                &addr,
                &format!(r#"{{"jsonrpc":"2.0","method":"daemon.status","params":{{}},"id":{i}}}"#),
            )
            .await;
            let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
            assert!(v["result"]["uptime_secs"].is_number());
        }));
    }
    for t in tasks {
        t.await.expect("task should complete");
    }
    handle.abort();
}

// ── Fault Injection ───────────────────────────────────────

#[tokio::test]
async fn test_fault_invalid_method_does_not_crash() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let methods = [
        "device.list.extra",
        "",
        ".",
        "device.",
        ".list",
        "DEVICE.LIST",
        "device.swap",
        "💀",
    ];
    for method in &methods {
        let resp = send_line(
            &addr,
            &format!(r#"{{"jsonrpc":"2.0","method":"{method}","params":{{}},"id":1}}"#),
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
        assert!(
            v["result"].is_object() || v["error"].is_object(),
            "method {method:?} should return valid JSON-RPC"
        );
    }
    handle.abort();
}

#[tokio::test]
async fn test_fault_swap_with_traversal_target() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.swap",
        &serde_json::json!({
            "bdf": "0000:01:00.0",
            "target": "../../../bin/sh"
        }),
        &mut devices,
        started,
    );
    assert!(result.is_err());
}

#[tokio::test]
async fn test_fault_device_get_with_empty_bdf() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.get",
        &serde_json::json!({"bdf": ""}),
        &mut devices,
        started,
    );
    let err = result.expect_err("empty bdf should fail");
    assert_eq!(i32::from(err.code), -32602);
}

#[tokio::test]
async fn test_fault_params_wrong_type() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.get",
        &serde_json::json!({"bdf": 12345}),
        &mut devices,
        started,
    );
    let err = result.expect_err("numeric bdf should fail");
    assert_eq!(i32::from(err.code), -32602);
}

#[tokio::test]
async fn test_fault_extra_params_ignored() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "health.check",
        &serde_json::json!({"extra_field": "should_be_ignored", "another": 42}),
        &mut devices,
        started,
    );
    assert!(result.is_ok());
}

// ── Penetration: Repeated shutdown / method abuse ─────────

#[test]
fn test_pen_repeated_shutdown_does_not_panic() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    for _ in 0..10 {
        let result = dispatch(
            "daemon.shutdown",
            &serde_json::json!({}),
            &mut devices,
            started,
        );
        assert!(result.is_err());
    }
}

#[test]
fn test_pen_method_not_found_does_not_leak_internals() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let probes = [
        "__proto__",
        "constructor",
        "toString",
        "system.listMethods",
        "rpc.discover",
        "admin.shutdown",
    ];
    for method in &probes {
        let result = dispatch(method, &serde_json::json!({}), &mut devices, started);
        let err = result.expect_err("should be method not found");
        assert_eq!(i32::from(err.code), -32601);
        assert!(
            !err.message.contains("panic"),
            "error should not reveal internal state"
        );
    }
}
