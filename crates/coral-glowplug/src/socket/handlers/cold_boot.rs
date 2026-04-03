// SPDX-License-Identifier: AGPL-3.0-only
//! K80 sovereign cold boot via JSON-RPC.
//!
//! Accepts a BDF and recipe path (any format from agentReagents), opens the
//! VFIO device, and orchestrates the full cold boot sequence through
//! `coral-driver`'s `k80_cold_boot` module. Recipe format is auto-detected.

use std::sync::Arc;

use super::validate_bdf;

pub(crate) fn handle_cold_boot(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();

    let recipe_path = params
        .get("recipe")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'recipe' parameter (path to recipe JSON)"))?;

    let firmware_dir = params
        .get("firmware_dir")
        .and_then(serde_json::Value::as_str);

    let include_pgraph = params
        .get("pgraph")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let include_pccsr = params
        .get("pccsr")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let include_pramin = params
        .get("pramin")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let skip_firmware = params
        .get("skip_firmware")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    let slot = devices
        .iter_mut()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf.as_str()),
        })
        .map_err(RpcError::from)?;
    if slot.is_busy() {
        return Err(RpcError::device_error(format!(
            "device {bdf} is busy — cannot cold boot"
        )));
    }

    let start = std::time::Instant::now();

    tracing::info!(
        bdf = %bdf,
        recipe = recipe_path,
        pgraph = include_pgraph,
        pccsr = include_pccsr,
        pramin = include_pramin,
        skip_firmware,
        "cold_boot: starting K80 sovereign cold boot"
    );

    // Acquire VFIO fds from ember (which holds the immortal fd for this device).
    // Direct VfioDevice::open would fail with EBUSY when ember already holds the group.
    let ember = coral_glowplug::ember::EmberClient::connect().ok_or_else(|| {
        RpcError::device_error("ember not available — cold boot requires ember for VFIO fds")
    })?;
    let fds = ember.request_fds(&bdf).map_err(|e| {
        RpcError::device_error(format!("ember VFIO fds for {bdf}: {e}"))
    })?;
    let device = coral_driver::vfio::VfioDevice::from_received(&bdf, fds)
        .map_err(|e| RpcError::device_error(format!("VFIO from ember fds {bdf}: {e}")))?;
    let bar0 = device
        .map_bar(0)
        .map_err(|e| RpcError::device_error(format!("BAR0 map {bdf}: {e}")))?;

    let config = coral_driver::vfio::channel::diagnostic::k80_cold_boot::ColdBootConfig {
        include_pgraph,
        include_pccsr,
        include_pramin,
    };

    let (fecs_code, fecs_data, gpccs_code, gpccs_data) = if skip_firmware {
        (None, None, None, None)
    } else {
        let fw_dir = firmware_dir
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| resolve_firmware_dir(&bdf));

        match load_firmware_blobs(&fw_dir) {
            Ok((fc, fd, gc, gd)) => (Some(fc), Some(fd), Some(gc), Some(gd)),
            Err(e) => {
                tracing::warn!(bdf = %bdf, error = %e, "cold_boot: firmware load failed, proceeding without");
                (None, None, None, None)
            }
        }
    };

    let recipe = std::path::Path::new(recipe_path);
    let result = coral_driver::vfio::channel::diagnostic::k80_cold_boot::cold_boot(
        &bar0,
        recipe,
        &config,
        fecs_code.as_deref(),
        fecs_data.as_deref(),
        gpccs_code.as_deref(),
        gpccs_data.as_deref(),
    )
    .map_err(|e| RpcError::device_error(format!("cold boot: {e}")))?;

    let total_ms = start.elapsed().as_millis() as u64;

    tracing::info!(
        bdf = %bdf,
        total_ms,
        fecs_running = result.fecs_running,
        clock_applied = result.clock_replay.applied,
        clock_failed = result.clock_replay.failed,
        "cold_boot complete"
    );

    Ok(serde_json::json!({
        "bdf": bdf,
        "total_ms": total_ms,
        "fecs_running": result.fecs_running,
        "clock_replay": {
            "applied": result.clock_replay.applied,
            "failed": result.clock_replay.failed,
            "ptimer_ticking": result.clock_replay.ptimer_ticking,
        },
        "devinit_replay": result.devinit_replay.as_ref().map(|r| serde_json::json!({
            "applied": r.applied,
            "failed": r.failed,
        })),
        "pgraph_replay": result.pgraph_replay.as_ref().map(|r| serde_json::json!({
            "applied": r.applied,
            "failed": r.failed,
        })),
        "firmware_snapshot": {
            "boot0": format!("{:#010x}", result.firmware_snapshot.boot0),
            "architecture": result.firmware_snapshot.architecture,
        },
        "log": result.log,
    }))
}

fn resolve_firmware_dir(bdf: &str) -> std::path::PathBuf {
    let _ = bdf;
    let candidates = [
        std::path::PathBuf::from("/lib/firmware/nvidia/gk110/gr"),
        std::path::PathBuf::from("/usr/share/coralreef/firmware/nvidia/gk110"),
    ];
    for c in &candidates {
        if c.join("fecs_inst.bin").exists() {
            return c.clone();
        }
    }
    candidates[0].clone()
}

fn load_firmware_blobs(
    dir: &std::path::Path,
) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>), String> {
    let load = |name: &str| -> Result<Vec<u8>, String> {
        let path = dir.join(name);
        std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))
    };
    Ok((
        load("fecs_inst.bin")?,
        load("fecs_data.bin")?,
        load("gpccs_inst.bin")?,
        load("gpccs_data.bin")?,
    ))
}
