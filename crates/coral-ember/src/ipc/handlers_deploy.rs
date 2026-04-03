// SPDX-License-Identifier: AGPL-3.0-only
//! `ember.deploy` — Rust-native binary self-update via staging directory.
//!
//! Ember runs as root inside a systemd sandbox (ProtectHome=true,
//! ProtectSystem=strict). It cannot read /home/ or write /usr/local/bin/
//! directly. Instead:
//!
//!   1. The caller (coralctl) copies built binaries to the staging dir
//!      `/run/coralreef/staging/` (writable by coralreef group)
//!   2. Ember reads from the staging dir (within its sandbox)
//!   3. Ember copies to /usr/local/bin/ (requires ReadWritePaths override)
//!   4. Ember triggers systemd restart
//!
//! This eliminates pkexec/sudo from the development workflow.

use std::io::Write;

use super::jsonrpc::{ipc_io_error_string, write_jsonrpc_error, write_jsonrpc_ok};

/// Staging directory within ember's sandbox. Writable by coralreef group.
pub(crate) const STAGING_DIR: &str = "/run/coralreef/staging";

/// `ember.deploy` — replace daemon binaries from staging dir and restart.
///
/// Params:
///   - `binaries`: optional array of binary names (default: all three)
///   - `restart`: bool, whether to restart services after deploy (default: true)
///
/// Binaries must be pre-staged in `/run/coralreef/staging/` by the caller.
pub(crate) fn deploy(
    stream: &mut impl Write,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), String> {
    let default_binaries = vec!["coral-ember", "coral-glowplug", "coralctl"];
    let binaries: Vec<&str> = params
        .get("binaries")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<&str>>()
        })
        .unwrap_or(default_binaries);

    let restart = params
        .get("restart")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let staging = std::path::Path::new(STAGING_DIR);
    if !staging.is_dir() {
        return write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!(
                "staging directory {STAGING_DIR} does not exist. \
                 coralctl deploy stages binaries there before calling this RPC."
            ),
        )
        .map_err(ipc_io_error_string);
    }

    let install_dir = "/usr/local/bin";
    let mut deployed = Vec::new();
    let mut errors = Vec::new();

    for name in &binaries {
        let src = staging.join(name);
        if !src.exists() {
            errors.push(format!("{name}: not found in {STAGING_DIR}"));
            continue;
        }

        let dst = format!("{install_dir}/{name}");
        let tmp = format!("{dst}.deploy.tmp");

        match deploy_binary(&src, &tmp, &dst) {
            Ok(size) => {
                tracing::info!(binary = name, size, "ember.deploy: binary installed");
                // Clean up staging copy
                let _ = std::fs::remove_file(&src);
                deployed.push(serde_json::json!({
                    "name": name,
                    "size": size,
                    "path": dst,
                }));
            }
            Err(e) => {
                tracing::error!(binary = name, error = %e, "ember.deploy: failed");
                errors.push(format!("{name}: {e}"));
            }
        }
    }

    if deployed.is_empty() {
        return write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("no binaries deployed: {}", errors.join("; ")),
        )
        .map_err(ipc_io_error_string);
    }

    let result = serde_json::json!({
        "deployed": deployed,
        "errors": errors,
        "restart_pending": restart,
    });
    write_jsonrpc_ok(stream, id, result).map_err(ipc_io_error_string)?;

    if restart && !deployed.is_empty() {
        // Write a detached restart script. We can't restart both services from
        // within ember because `systemctl restart coral-ember` kills this process
        // before it can start glowplug. A detached shell script survives the kill.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let script = "/run/coralreef/deploy-restart.sh";
            let contents = "\
#!/bin/sh
sleep 1
systemctl stop coral-glowplug 2>/dev/null
sleep 1
systemctl restart coral-ember
sleep 4
systemctl start coral-glowplug
rm -f /run/coralreef/deploy-restart.sh
";
            if std::fs::write(script, contents).is_ok() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        script,
                        std::fs::Permissions::from_mode(0o755),
                    );
                }
                tracing::info!("ember.deploy: launching restart script");
                let _ = std::process::Command::new("setsid")
                    .args([script])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            } else {
                tracing::error!("ember.deploy: failed to write restart script");
            }
        });
    }

    Ok(())
}

fn deploy_binary(src: &std::path::Path, tmp: &str, dst: &str) -> Result<u64, String> {
    std::fs::copy(src, tmp).map_err(|e| format!("copy to staging: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(tmp, perms)
            .map_err(|e| format!("set permissions on staging: {e}"))?;
    }

    let meta = std::fs::metadata(tmp).map_err(|e| format!("stat staging: {e}"))?;
    let size = meta.len();

    std::fs::rename(tmp, dst).map_err(|e| format!("rename staging -> target: {e}"))?;

    Ok(size)
}
