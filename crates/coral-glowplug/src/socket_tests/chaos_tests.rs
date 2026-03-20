// SPDX-License-Identifier: AGPL-3.0-only

use super::{send_line, spawn_test_server};

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
