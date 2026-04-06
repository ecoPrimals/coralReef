// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

use tokio::net::TcpListener;

#[tokio::test]
async fn cmd_server_jsonrpc_invalid_bind_returns_general_error() {
    let result = cmd_server("not-a-valid-address", "127.0.0.1:0", None).await;
    assert!(
        matches!(result, UniBinExit::GeneralError),
        "invalid JSON-RPC bind address should produce GeneralError"
    );
}

#[tokio::test]
async fn cmd_server_tarpc_invalid_bind_returns_general_error() {
    // JSON-RPC binds successfully; tarpc fails with invalid address
    let result = cmd_server("127.0.0.1:0", "garbage:not-valid", None).await;
    assert!(
        matches!(result, UniBinExit::GeneralError),
        "invalid tarpc bind address should produce GeneralError"
    );
}

#[tokio::test]
async fn cmd_server_newline_port_bind_conflict_returns_general_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let result = cmd_server("127.0.0.1:0", "127.0.0.1:0", Some(port)).await;
    drop(listener);
    assert!(
        matches!(result, UniBinExit::GeneralError),
        "newline TCP bind should fail when port is already in use"
    );
}
