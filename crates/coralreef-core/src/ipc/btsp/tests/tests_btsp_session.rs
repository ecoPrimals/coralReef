// SPDX-License-Identifier: AGPL-3.0-or-later
//! BTSP session creation tests with mock Unix socket providers.

use super::*;

fn uds_endpoint(path: &std::path::Path) -> TransportEndpoint {
    TransportEndpoint::Uds {
        path: path.to_string_lossy().into_owned(),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn create_btsp_session_connect_failure() {
    let ep = uds_endpoint(std::path::Path::new("/nonexistent/btsp.sock"));
    let result = create_btsp_session(&ep, "test-family").await;
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), BtspSessionError::Io(_)),
        "connect failure should produce Io error"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn create_btsp_session_success_with_session_id() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("btsp-mock.sock");
    let listener = tokio::net::UnixListener::bind(&sock).expect("bind");

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read");
        let parsed: serde_json::Value = serde_json::from_str(&line).expect("valid json");
        assert_eq!(parsed["method"], "btsp.session.create");
        let writer = reader.into_inner();
        let (_, mut w) = tokio::io::split(writer);
        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "result": { "session_id": "sess-abc-123" },
            "id": 1
        });
        w.write_all(format!("{resp}\n").as_bytes())
            .await
            .expect("write");
    });

    let result = create_btsp_session(&uds_endpoint(&sock), "test-fam").await;
    assert!(result.is_ok(), "should succeed: {result:?}");
    let (sid, key) = result.unwrap();
    assert_eq!(sid, "sess-abc-123");
    assert!(key.is_none(), "no handshake_key => None");

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn create_btsp_session_success_with_handshake_key() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("btsp-key.sock");
    let listener = tokio::net::UnixListener::bind(&sock).expect("bind");

    let key_bytes = [0x42u8; 32];
    let key_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, key_bytes);

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read");
        let writer = reader.into_inner();
        let (_, mut w) = tokio::io::split(writer);
        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "result": { "session_id": "sess-xyz", "handshake_key": key_b64 },
            "id": 1
        });
        w.write_all(format!("{resp}\n").as_bytes())
            .await
            .expect("write");
    });

    let result = create_btsp_session(&uds_endpoint(&sock), "test-fam").await;
    assert!(result.is_ok(), "should succeed: {result:?}");
    let (sid, key) = result.unwrap();
    assert_eq!(sid, "sess-xyz");
    assert_eq!(key, Some(key_bytes));

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn create_btsp_session_error_response() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("btsp-err.sock");
    let listener = tokio::net::UnixListener::bind(&sock).expect("bind");

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read");
        let writer = reader.into_inner();
        let (_, mut w) = tokio::io::split(writer);
        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "error": { "code": -32600, "message": "invalid family" },
            "id": 1
        });
        w.write_all(format!("{resp}\n").as_bytes())
            .await
            .expect("write");
    });

    let result = create_btsp_session(&uds_endpoint(&sock), "bad-family").await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, BtspSessionError::Protocol(_)),
        "error response should produce Protocol error: {err}"
    );
    assert!(err.to_string().contains("invalid family"));

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn create_btsp_session_missing_result() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("btsp-noresult.sock");
    let listener = tokio::net::UnixListener::bind(&sock).expect("bind");

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read");
        let writer = reader.into_inner();
        let (_, mut w) = tokio::io::split(writer);
        let resp = serde_json::json!({"jsonrpc": "2.0", "id": 1});
        w.write_all(format!("{resp}\n").as_bytes())
            .await
            .expect("write");
    });

    let result = create_btsp_session(&uds_endpoint(&sock), "fam").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("missing result"));

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn create_btsp_session_missing_session_id() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("btsp-nosid.sock");
    let listener = tokio::net::UnixListener::bind(&sock).expect("bind");

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read");
        let writer = reader.into_inner();
        let (_, mut w) = tokio::io::split(writer);
        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "result": { "other_field": 42 },
            "id": 1
        });
        w.write_all(format!("{resp}\n").as_bytes())
            .await
            .expect("write");
    });

    let result = create_btsp_session(&uds_endpoint(&sock), "fam").await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("missing session_id")
    );

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn create_btsp_session_empty_response() {
    use tokio::io::AsyncWriteExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("btsp-empty.sock");
    let listener = tokio::net::UnixListener::bind(&sock).expect("bind");

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (_, mut w) = stream.into_split();
        w.shutdown().await.expect("shutdown");
    });

    let result = create_btsp_session(&uds_endpoint(&sock), "fam").await;
    assert!(result.is_err(), "empty response should produce an error");

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn create_btsp_session_garbage_json() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("btsp-garbage.sock");
    let listener = tokio::net::UnixListener::bind(&sock).expect("bind");

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read");
        let writer = reader.into_inner();
        let (_, mut w) = tokio::io::split(writer);
        w.write_all(b"not json at all\n").await.expect("write");
    });

    let result = create_btsp_session(&uds_endpoint(&sock), "fam").await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), BtspSessionError::Json(_)));

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn create_btsp_session_invalid_handshake_key_length() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("btsp-badkey.sock");
    let listener = tokio::net::UnixListener::bind(&sock).expect("bind");

    let short_key =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0xABu8; 16]);

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read");
        let writer = reader.into_inner();
        let (_, mut w) = tokio::io::split(writer);
        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "result": { "session_id": "sess-short", "handshake_key": short_key },
            "id": 1
        });
        w.write_all(format!("{resp}\n").as_bytes())
            .await
            .expect("write");
    });

    let result = create_btsp_session(&uds_endpoint(&sock), "fam").await;
    assert!(result.is_ok());
    let (sid, key) = result.unwrap();
    assert_eq!(sid, "sess-short");
    assert!(
        key.is_none(),
        "16-byte key should fail try_from to [u8; 32]"
    );

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
}

#[test]
fn check_discovery_file_method_present_different_method() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("provider.sock");
    std::os::unix::net::UnixListener::bind(&sock).expect("bind");
    let json_path = dir.path().join("provider.json");
    let data = serde_json::json!({
        "methods": ["other.method", "health.check"],
        "transports": { "unix": format!("unix://{}", sock.display()) }
    });
    std::fs::write(&json_path, data.to_string()).expect("write");
    assert!(
        check_discovery_file_for_method(&json_path, "btsp.session.create").is_none(),
        "should not match when method is absent"
    );
}

#[test]
fn discover_by_capability_skips_non_json_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("notes.txt"), "irrelevant").expect("write");
    std::fs::write(dir.path().join("config.yaml"), "key: value").expect("write");
    let result = discover_by_capability(dir.path(), "btsp.session.create");
    assert!(result.is_none());
}
