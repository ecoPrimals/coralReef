// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[tokio::test]
async fn cmd_server_jsonrpc_invalid_bind_returns_general_error() {
    let result = cmd_server("not-a-valid-address", "127.0.0.1:0").await;
    assert!(
        matches!(result, UniBinExit::GeneralError),
        "invalid JSON-RPC bind address should produce GeneralError"
    );
}

#[tokio::test]
async fn cmd_server_tarpc_invalid_bind_returns_general_error() {
    let result = cmd_server("127.0.0.1:0", "garbage:not-valid").await;
    assert!(
        matches!(result, UniBinExit::GeneralError),
        "invalid tarpc bind address should produce GeneralError"
    );
}
