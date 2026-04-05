// SPDX-License-Identifier: AGPL-3.0-only
//! Ecosystem discovery file management for coral-ember.
//!
//! Writes a JSON discovery file to `$XDG_RUNTIME_DIR/biomeos/` on startup
//! so that other primals can discover coral-ember's capabilities and socket
//! address at runtime. Follows the same schema and directory convention as
//! the wateringHole `UNIVERSAL_IPC_STANDARD_V3`.

use std::path::PathBuf;

const ECOSYSTEM_NAMESPACE: &str = "biomeos";

fn discovery_dir() -> std::io::Result<PathBuf> {
    let base =
        std::env::var("XDG_RUNTIME_DIR").map_or_else(|_| std::env::temp_dir(), PathBuf::from);
    Ok(base.join(ECOSYSTEM_NAMESPACE))
}

/// Write a discovery JSON file advertising coral-ember's capabilities and socket bind.
///
/// Called after the Unix socket is bound. The file is placed in the shared
/// ecosystem directory so peers can discover ember without hardcoded paths.
pub fn write_discovery_file(socket_bind: &str, tcp_bind: Option<&str>, device_count: usize) {
    let dir = match discovery_dir() {
        Ok(d) => d,
        Err(e) => {
            tracing::debug!(error = %e, "cannot resolve discovery dir — skipping discovery file");
            return;
        }
    };
    if std::fs::create_dir_all(&dir).is_err() {
        tracing::debug!(dir = %dir.display(), "cannot create discovery dir");
        return;
    }

    let path = dir.join(format!("{}.json", env!("CARGO_PKG_NAME")));

    let mut transports = serde_json::json!({
        "jsonrpc_unix": { "bind": format!("unix://{socket_bind}") },
    });
    if let Some(tcp) = tcp_bind {
        transports["jsonrpc_tcp"] = serde_json::json!({ "bind": tcp });
    }

    let discovery = serde_json::json!({
        "primal": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
        "pid": std::process::id(),
        "provides": [
            "gpu.vfio.hold",
            "gpu.mmio.gateway",
            "gpu.device.manage",
        ],
        "requires": [],
        "transports": transports,
        "metadata": {
            "device_count": device_count,
        },
    });

    match serde_json::to_string_pretty(&discovery) {
        Ok(json) => match std::fs::write(&path, json) {
            Ok(()) => tracing::info!(path = %path.display(), "wrote ecosystem discovery file"),
            Err(e) => tracing::debug!(error = %e, "failed to write discovery file"),
        },
        Err(e) => tracing::debug!(error = %e, "failed to serialize discovery file"),
    }
}

/// Remove the discovery file on shutdown.
pub fn remove_discovery_file() {
    if let Ok(dir) = discovery_dir() {
        let path = dir.join(format!("{}.json", env!("CARGO_PKG_NAME")));
        if path.exists() {
            let _ = std::fs::remove_file(&path);
            tracing::debug!(path = %path.display(), "removed discovery file");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_dir_ends_with_namespace() {
        let dir = discovery_dir().expect("discovery_dir");
        assert_eq!(
            dir.file_name().and_then(|n| n.to_str()),
            Some(ECOSYSTEM_NAMESPACE),
        );
    }

    #[test]
    fn discovery_json_schema() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join(ECOSYSTEM_NAMESPACE);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{}.json", env!("CARGO_PKG_NAME")));

        let discovery = serde_json::json!({
            "primal": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
            "pid": std::process::id(),
            "provides": ["gpu.vfio.hold", "gpu.mmio.gateway", "gpu.device.manage"],
            "requires": [],
            "transports": {
                "jsonrpc_unix": { "bind": "unix:///tmp/test.sock" },
            },
            "metadata": { "device_count": 2 },
        });

        std::fs::write(&path, serde_json::to_string_pretty(&discovery).unwrap()).unwrap();
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        assert_eq!(content["primal"], env!("CARGO_PKG_NAME"));
        assert_eq!(content["metadata"]["device_count"], 2);
        assert!(content["provides"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "gpu.vfio.hold"));
        assert!(content["provides"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "gpu.mmio.gateway"));
    }

    #[test]
    fn remove_discovery_file_idempotent() {
        remove_discovery_file();
        remove_discovery_file();
    }
}
