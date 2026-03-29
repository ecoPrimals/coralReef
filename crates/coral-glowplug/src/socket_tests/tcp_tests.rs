// SPDX-License-Identifier: AGPL-3.0-only

use super::super::*;
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::test]
async fn test_tcp_bind_127_0_0_1_0() {
    let server = SocketServer::bind("127.0.0.1:0")
        .await
        .expect("TCP bind should succeed");
    let addr = server.bound_addr();
    assert!(addr.contains("127.0.0.1"));
    assert!(addr.contains(':'));
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
async fn test_tcp_device_list_and_get() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    let server = SocketServer::bind("127.0.0.1:0").await.expect("bind");
    let addr = server.bound_addr();
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:aa:00.0".into(),
        name: Some("RPC GPU".into()),
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: Some("compute".into()),
        oracle_dump: None,
        shared: None,
    };
    let devices = Arc::new(Mutex::new(vec![coral_glowplug::device::DeviceSlot::new(
        config,
    )]));
    let (_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let devices_clone = devices.clone();

    let handle = tokio::spawn(async move {
        server.accept_loop(devices_clone, &mut shutdown_rx).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream = TcpStream::connect(&addr).await.expect("connect");
    let (reader, mut writer) = stream.into_split();

    let list_req = r#"{"jsonrpc":"2.0","method":"device.list","params":{},"id":1}"#;
    writer
        .write_all(format!("{list_req}\n").as_bytes())
        .await
        .expect("write");
    let mut lines = BufReader::new(reader).lines();
    let line1 = lines.next_line().await.expect("read").expect("line1");
    let v1: serde_json::Value = serde_json::from_str(&line1).expect("parse");
    assert_eq!(v1["result"][0]["bdf"], "0000:aa:00.0");
    assert_eq!(v1["result"][0]["name"], "RPC GPU");

    let get_req =
        r#"{"jsonrpc":"2.0","method":"device.get","params":{"bdf":"0000:aa:00.0"},"id":2}"#;
    writer
        .write_all(format!("{get_req}\n").as_bytes())
        .await
        .expect("write");
    let line2 = lines.next_line().await.expect("read").expect("line2");
    let v2: serde_json::Value = serde_json::from_str(&line2).expect("parse");
    assert_eq!(v2["result"]["bdf"], "0000:aa:00.0");
    assert!(v2["result"]["chip"].as_str().is_some());

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
async fn test_unknown_method_over_tcp() {
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

    let req = r#"{"jsonrpc":"2.0","method":"gpu.magic","params":{},"id":99}"#;
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .expect("write");

    let mut lines = BufReader::new(reader).lines();
    let resp_line = lines.next_line().await.expect("read").expect("line");
    let resp: serde_json::Value = serde_json::from_str(&resp_line).expect("parse");

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["error"]["code"], -32601);
    assert_eq!(resp["id"], 99);

    handle.abort();
}

#[tokio::test]
async fn test_empty_and_whitespace_lines_skipped_before_request() {
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

    writer.write_all(b"\n   \n").await.expect("write blanks");
    let req = r#"{"jsonrpc":"2.0","method":"daemon.status","params":{},"id":7}"#;
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .expect("write");

    let mut lines = BufReader::new(reader).lines();
    let resp_line = lines.next_line().await.expect("read").expect("line");
    let resp: serde_json::Value = serde_json::from_str(&resp_line).expect("parse");

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 7);
    assert!(resp["result"]["uptime_secs"].is_number());

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

    let next = lines.next_line().await.expect("read");
    assert!(
        next.is_none(),
        "connection should close after shutdown response"
    );

    handle.abort();
}
