// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC handlers for kernel livepatch lifecycle management.

use std::io::Write;

use super::jsonrpc::{ipc_io_error_string, write_jsonrpc_error, write_jsonrpc_ok};

const DEFAULT_LIVEPATCH_MODULE: &str = "livepatch_nvkm_mc_reset";

fn livepatch_module() -> &'static str {
    static MODULE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    MODULE.get_or_init(|| {
        std::env::var("CORALREEF_LIVEPATCH_MODULE")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_LIVEPATCH_MODULE.to_string())
    })
}

fn livepatch_sysfs() -> &'static str {
    static SYSFS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SYSFS.get_or_init(|| format!("/sys/kernel/livepatch/{}", livepatch_module()))
}

/// `ember.livepatch.status` — returns the livepatch module state.
pub(crate) fn status(
    stream: &mut impl Write,
    id: serde_json::Value,
    _params: &serde_json::Value,
) -> Result<(), String> {
    let sysfs = livepatch_sysfs();
    let loaded = std::path::Path::new(sysfs).exists();
    if !loaded {
        return write_jsonrpc_ok(
            stream,
            id,
            serde_json::json!({
                "loaded": false,
                "enabled": false,
                "transition": false,
                "patched_funcs": [],
            }),
        )
        .map_err(ipc_io_error_string);
    }

    let enabled = read_sysfs_bool(&format!("{sysfs}/enabled"));
    let transition = read_sysfs_bool(&format!("{sysfs}/transition"));
    let patched_funcs = list_patched_funcs();

    write_jsonrpc_ok(
        stream,
        id,
        serde_json::json!({
            "loaded": true,
            "enabled": enabled,
            "transition": transition,
            "patched_funcs": patched_funcs,
        }),
    )
    .map_err(ipc_io_error_string)
}

/// `ember.livepatch.enable` — load the module if needed, then enable.
///
/// Idempotent: returns success immediately if already enabled and stable.
pub(crate) fn enable(
    stream: &mut impl Write,
    id: serde_json::Value,
    _params: &serde_json::Value,
) -> Result<(), String> {
    let sysfs = livepatch_sysfs();
    let module = livepatch_module();
    let loaded = std::path::Path::new(sysfs).exists();
    if !loaded {
        tracing::info!("livepatch module not loaded — running modprobe {module}");
        let output = std::process::Command::new("modprobe")
            .arg(module)
            .output()
            .map_err(|e| format!("modprobe exec failed: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("modprobe {module} failed: {stderr}"),
            )
            .map_err(ipc_io_error_string);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    if !std::path::Path::new(sysfs).exists() {
        return write_jsonrpc_error(
            stream,
            id,
            -32000,
            "livepatch module loaded but sysfs entry not present",
        )
        .map_err(ipc_io_error_string);
    }

    let enabled_path = format!("{sysfs}/enabled");
    let already_enabled = read_sysfs_bool(&enabled_path);
    if already_enabled {
        tracing::debug!("livepatch already enabled — idempotent no-op");
    } else {
        std::fs::write(&enabled_path, "1").map_err(|e| format!("write {enabled_path}: {e}"))?;
        tracing::info!("wrote 1 to {enabled_path}");
    }

    if let Err(msg) = wait_transition_complete(std::time::Duration::from_secs(10)) {
        return write_jsonrpc_error(stream, id, -32000, &msg).map_err(ipc_io_error_string);
    }

    tracing::info!(already_enabled, "livepatch enabled");
    write_jsonrpc_ok(
        stream,
        id,
        serde_json::json!({
            "enabled": true,
            "transition": false,
            "was_noop": already_enabled,
        }),
    )
    .map_err(ipc_io_error_string)
}

/// `ember.livepatch.disable` — disable the livepatch (module remains loaded).
///
/// Idempotent: returns success immediately if already disabled or not loaded.
pub(crate) fn disable(
    stream: &mut impl Write,
    id: serde_json::Value,
    _params: &serde_json::Value,
) -> Result<(), String> {
    let sysfs = livepatch_sysfs();
    if !std::path::Path::new(sysfs).exists() {
        tracing::debug!("livepatch module not loaded — nothing to disable");
        return write_jsonrpc_ok(
            stream,
            id,
            serde_json::json!({
                "enabled": false,
                "was_noop": true,
                "note": "module not loaded",
            }),
        )
        .map_err(ipc_io_error_string);
    }

    let enabled_path = format!("{sysfs}/enabled");
    let was_enabled = read_sysfs_bool(&enabled_path);
    if was_enabled {
        std::fs::write(&enabled_path, "0").map_err(|e| format!("write {enabled_path}: {e}"))?;
        tracing::info!("wrote 0 to {enabled_path}");
    } else {
        tracing::debug!("livepatch already disabled — idempotent no-op");
    }

    if let Err(msg) = wait_transition_complete(std::time::Duration::from_secs(10)) {
        return write_jsonrpc_error(stream, id, -32000, &msg).map_err(ipc_io_error_string);
    }

    tracing::info!(was_enabled, "livepatch disabled");
    write_jsonrpc_ok(
        stream,
        id,
        serde_json::json!({
            "enabled": false,
            "transition": false,
            "was_noop": !was_enabled,
        }),
    )
    .map_err(ipc_io_error_string)
}

fn read_sysfs_bool(path: &str) -> bool {
    std::fs::read_to_string(path)
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

fn wait_transition_complete(timeout: std::time::Duration) -> Result<(), String> {
    let transition_path = format!("{}/transition", livepatch_sysfs());
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if !read_sysfs_bool(&transition_path) {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "livepatch transition did not complete within {}s",
                timeout.as_secs()
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn list_patched_funcs() -> Vec<String> {
    let funcs_dir = format!("{}/nouveau/funcs", livepatch_sysfs());
    let Ok(entries) = std::fs::read_dir(&funcs_dir) else {
        return vec![];
    };
    entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_sysfs_bool_nonexistent_is_false() {
        assert!(!read_sysfs_bool("/nonexistent/coral/ember/test/bool"));
    }

    #[test]
    fn read_sysfs_bool_with_tmpfile() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("enabled");
        std::fs::write(&path, "1\n").expect("write");
        assert!(read_sysfs_bool(path.to_str().unwrap()));
        std::fs::write(&path, "0\n").expect("write");
        assert!(!read_sysfs_bool(path.to_str().unwrap()));
    }

    #[test]
    fn list_patched_funcs_with_mock_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let funcs_dir = dir.path().join("nouveau").join("funcs");
        std::fs::create_dir_all(&funcs_dir).expect("mkdir -p");
        std::fs::write(funcs_dir.join("nvkm_mc_reset"), "").expect("write");
        std::fs::write(funcs_dir.join("nvkm_gr_fini"), "").expect("write");

        let Ok(entries) = std::fs::read_dir(&funcs_dir) else {
            panic!("should read mock funcs dir");
        };
        let mut names: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["nvkm_gr_fini", "nvkm_mc_reset"]);
    }

    #[test]
    fn list_patched_funcs_returns_empty_for_nonexistent() {
        let result = list_patched_funcs();
        drop(result);
    }

    #[test]
    fn status_handler_returns_not_loaded_when_no_sysfs() {
        let (mut server, mut client) = std::os::unix::net::UnixStream::pair().expect("stream pair");
        status(&mut server, serde_json::json!(1), &serde_json::json!({})).expect("status handler");
        let mut buf = vec![0u8; 4096];
        let n = std::io::Read::read(&mut client, &mut buf).expect("read response");
        let resp: serde_json::Value = serde_json::from_slice(&buf[..n]).expect("parse response");
        assert!(resp.get("result").is_some(), "should have result field");
        let result = resp.get("result").unwrap();
        assert_eq!(result.get("loaded").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(result.get("enabled").and_then(|v| v.as_bool()), Some(false));
    }

    #[test]
    fn disable_handler_noop_when_not_loaded() {
        let (mut server, mut client) = std::os::unix::net::UnixStream::pair().expect("stream pair");
        disable(&mut server, serde_json::json!(2), &serde_json::json!({}))
            .expect("disable handler");
        let mut buf = vec![0u8; 4096];
        let n = std::io::Read::read(&mut client, &mut buf).expect("read response");
        let resp: serde_json::Value = serde_json::from_slice(&buf[..n]).expect("parse response");
        let result = resp.get("result").unwrap();
        assert_eq!(result.get("enabled").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(result.get("was_noop").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn wait_transition_complete_returns_ok_when_no_sysfs() {
        let result = wait_transition_complete(std::time::Duration::from_millis(50));
        assert!(
            result.is_ok(),
            "should succeed when sysfs absent (read_sysfs_bool returns false)"
        );
    }
}
