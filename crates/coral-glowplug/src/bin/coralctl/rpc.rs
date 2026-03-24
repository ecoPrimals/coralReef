// SPDX-License-Identifier: AGPL-3.0-only
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
                eprintln!("error: permission denied connecting to {socket_path}");
                eprintln!("hint: add yourself to the coralreef group:");
                eprintln!("  sudo groupadd -r coralreef");
                eprintln!("  sudo usermod -aG coralreef $USER");
                eprintln!("  newgrp coralreef  # or log out and back in");
            } else if e.kind() == std::io::ErrorKind::NotFound {
                eprintln!("error: socket not found at {socket_path}");
                eprintln!("hint: is coral-glowplug running?  systemctl status coral-glowplug");
            } else {
                eprintln!("error: failed to connect to {socket_path}: {e}");
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
        eprintln!("error: failed to send RPC: {e}");
        std::process::exit(1);
    }

    let mut reader = std::io::BufReader::new(&stream);
    let mut response_line = String::new();
    if let Err(e) = reader.read_line(&mut response_line) {
        eprintln!("error: failed to read RPC response: {e}");
        std::process::exit(1);
    }

    match serde_json::from_str::<Value>(&response_line) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: invalid JSON response: {e}");
            eprintln!("raw: {response_line}");
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
        eprintln!("error [{code}]: {message}");
        std::process::exit(1);
    }
}
