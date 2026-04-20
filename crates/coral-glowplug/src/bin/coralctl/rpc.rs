// SPDX-License-Identifier: AGPL-3.0-or-later
//! JSON-RPC client over the glowplug Unix socket (pure `std`, no extra deps).

use serde_json::Value;

/// Invoke a JSON-RPC method and return the parsed response object.
pub(crate) fn rpc_call(socket_path: &str, method: &str, params: Value) -> Value {
    use std::io::{BufRead, Write};
    use std::os::unix::net::UnixStream;

    let mut stream = match UnixStream::connect(socket_path) {
        Ok(s) => s,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                tracing::error!(%socket_path, "permission denied connecting to glowplug socket");
                tracing::error!("hint: add yourself to the coralreef group:");
                tracing::error!("  sudo groupadd -r coralreef");
                tracing::error!("  sudo usermod -aG coralreef $USER");
                tracing::error!("  newgrp coralreef  # or log out and back in");
            } else if e.kind() == std::io::ErrorKind::NotFound {
                tracing::error!(%socket_path, "socket not found");
                tracing::error!(
                    "hint: is coral-glowplug running?  systemctl status coral-glowplug"
                );
            } else {
                tracing::error!(%socket_path, error = %e, "failed to connect to glowplug socket");
            }
            std::process::exit(1);
        }
    };

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });

    let mut payload =
        serde_json::to_string(&request).expect("JSON Value serialization is infallible");
    payload.push('\n');

    if let Err(e) = stream.write_all(payload.as_bytes()) {
        tracing::error!(error = %e, "failed to send RPC");
        std::process::exit(1);
    }

    let mut reader = std::io::BufReader::new(&stream);
    let mut response_line = String::new();
    if let Err(e) = reader.read_line(&mut response_line) {
        tracing::error!(error = %e, "failed to read RPC response");
        std::process::exit(1);
    }

    match serde_json::from_str::<Value>(&response_line) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "invalid JSON response");
            tracing::error!(raw = %response_line.trim_end(), "RPC response body");
            std::process::exit(1);
        }
    }
}

/// Exit the process if the JSON-RPC envelope contains an `error` object.
pub(crate) fn check_rpc_error(response: &Value) {
    if let Some(error) = response.get("error") {
        let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        let message = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        tracing::error!(code, %message, "JSON-RPC error");
        std::process::exit(1);
    }
}
