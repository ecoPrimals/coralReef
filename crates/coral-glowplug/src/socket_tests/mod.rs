// SPDX-License-Identifier: AGPL-3.0-only
//! Tests for the JSON-RPC socket server.

use super::*;
use std::sync::Arc;
use tokio::sync::Mutex;

pub(crate) async fn spawn_test_server() -> (
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

pub(crate) async fn send_line(addr: &str, payload: &str) -> String {
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

mod chaos_tests;
mod dispatch_tests;
mod fault_tests;
mod parse_tests;
mod tcp_tests;
mod validate_bdf_tests;
