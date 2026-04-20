// SPDX-License-Identifier: AGPL-3.0-or-later
//! JSON-RPC handlers for coral-kmod lifecycle management.
//!
//! Methods:
//! - `ember.kmod.status`  — check if coral-kmod.ko is loaded and healthy
//! - `ember.kmod.load`    — load the kernel module (insmod / modprobe)
//! - `ember.kmod.unload`  — unload the kernel module (rmmod)

use std::io::Write;
use std::path::Path;

use super::jsonrpc::write_jsonrpc_ok;
use crate::error::EmberIpcError;

const KMOD_NAME: &str = "coral_kmod";
const KMOD_SYSFS: &str = "/sys/module/coral_kmod";
const KMOD_DEV: &str = "/dev/coral-rm";

fn is_loaded() -> bool {
    Path::new(KMOD_SYSFS).exists()
}

fn is_device_ready() -> bool {
    Path::new(KMOD_DEV).exists()
}

/// `ember.kmod.status` — report kernel module state.
pub fn status(stream: &mut impl Write, id: serde_json::Value) -> Result<(), EmberIpcError> {
    let loaded = is_loaded();
    let device_ready = is_device_ready();

    write_jsonrpc_ok(
        stream,
        id,
        serde_json::json!({
            "loaded": loaded,
            "device_ready": device_ready,
            "module_name": KMOD_NAME,
            "device_path": KMOD_DEV,
        }),
    )
    .map_err(EmberIpcError::from)
}

/// `ember.kmod.load` — load `coral_kmod.ko` if not already loaded.
///
/// Accepts an optional `path` param for an explicit `.ko` file path
/// (uses `insmod`). Without `path`, uses `modprobe coral_kmod`.
pub fn load(
    stream: &mut impl Write,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    if is_loaded() {
        write_jsonrpc_ok(
            stream,
            id,
            serde_json::json!({ "already_loaded": true, "device_ready": is_device_ready() }),
        )
        .map_err(EmberIpcError::from)?;
        return Ok(());
    }

    let ko_path = params.get("path").and_then(serde_json::Value::as_str);

    let output = if let Some(path) = ko_path {
        tracing::info!(path, "loading coral_kmod via insmod");
        std::process::Command::new("insmod").arg(path).output()
    } else {
        tracing::info!("loading coral_kmod via modprobe");
        std::process::Command::new("modprobe")
            .arg(KMOD_NAME)
            .output()
    };

    match output {
        Ok(out) if out.status.success() => {
            std::thread::sleep(std::time::Duration::from_millis(200));
            tracing::info!("coral_kmod loaded successfully");
            write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({
                    "loaded": is_loaded(),
                    "device_ready": is_device_ready(),
                }),
            )
            .map_err(EmberIpcError::from)?;
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            tracing::error!(%stderr, "coral_kmod load failed");
            super::jsonrpc::write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("kmod load failed: {stderr}"),
            )
            .map_err(EmberIpcError::from)?;
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to execute insmod/modprobe");
            super::jsonrpc::write_jsonrpc_error(stream, id, -32000, &format!("exec failed: {e}"))
                .map_err(EmberIpcError::from)?;
        }
    }

    Ok(())
}

/// `ember.kmod.unload` — unload `coral_kmod.ko`.
pub fn unload(stream: &mut impl Write, id: serde_json::Value) -> Result<(), EmberIpcError> {
    if !is_loaded() {
        write_jsonrpc_ok(stream, id, serde_json::json!({ "already_unloaded": true }))
            .map_err(EmberIpcError::from)?;
        return Ok(());
    }

    tracing::info!("unloading coral_kmod");
    match std::process::Command::new("rmmod").arg(KMOD_NAME).output() {
        Ok(out) if out.status.success() => {
            tracing::info!("coral_kmod unloaded");
            write_jsonrpc_ok(stream, id, serde_json::json!({ "unloaded": true }))
                .map_err(EmberIpcError::from)?;
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            super::jsonrpc::write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("rmmod failed: {stderr}"),
            )
            .map_err(EmberIpcError::from)?;
        }
        Err(e) => {
            super::jsonrpc::write_jsonrpc_error(stream, id, -32000, &format!("exec failed: {e}"))
                .map_err(EmberIpcError::from)?;
        }
    }

    Ok(())
}
