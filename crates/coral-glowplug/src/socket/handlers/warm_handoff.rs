// SPDX-License-Identifier: AGPL-3.0-only
//! Warm handoff: swap to a kernel driver, settle, poll FECS, swap back to vfio.

use std::sync::Arc;

use super::validate_bdf;

pub(crate) fn handle_warm_handoff(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();
    let driver = params
        .get("driver")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("nouveau");
    let settle_ms = params
        .get("settle_ms")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(2000);
    let poll_fecs = params
        .get("poll_fecs")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let poll_timeout_ms = params
        .get("poll_timeout_ms")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(30_000);
    let keepalive = params
        .get("keepalive")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let enable_trace = params
        .get("trace")
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
            "device {bdf} is busy — cannot perform warm handoff"
        )));
    }

    let ember = coral_glowplug::ember::EmberClient::connect().ok_or_else(|| {
        RpcError::device_error("ember not available — warm handoff requires ember")
    })?;

    let handoff_start = std::time::Instant::now();

    // Step 1: Disable livepatch if targeting nouveau (unfreeze teardown paths)
    if driver == "nouveau" {
        tracing::info!(bdf = %bdf, "warm_handoff: disabling livepatch");
        if let Err(e) = ember.livepatch_disable() {
            tracing::warn!(bdf = %bdf, error = %e, "warm_handoff: livepatch disable failed (non-fatal)");
        }
    }

    // Step 2: Capture pre-swap FECS state
    let pre_fecs = ember.fecs_state(&bdf).ok();

    // Step 3: Swap to target driver via ember
    tracing::info!(bdf = %bdf, driver, trace = enable_trace, "warm_handoff: swapping to driver");
    slot.swap_traced(driver, enable_trace)
        .map_err(|e| RpcError::device_error(format!("swap to {driver}: {e}")))?;

    // Step 4: Settle
    std::thread::sleep(std::time::Duration::from_millis(settle_ms));

    // Step 5: Enable livepatch (freeze teardown paths)
    if driver == "nouveau" {
        tracing::info!(bdf = %bdf, "warm_handoff: enabling livepatch");
        if let Err(e) = ember.livepatch_enable() {
            tracing::warn!(bdf = %bdf, error = %e, "warm_handoff: livepatch enable failed");
        }
    }

    // Step 6: Poll FECS if requested
    let mut fecs_ever_running = false;
    let mut poll_count = 0u32;
    let mut last_fecs_during_poll = None;
    if poll_fecs {
        let poll_start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(poll_timeout_ms);
        while poll_start.elapsed() < timeout {
            poll_count += 1;
            if let Ok(state) = ember.fecs_state(&bdf) {
                let pri_fault = state
                    .get("pri_fault")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let halted = state
                    .get("halted")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let stopped = state
                    .get("stopped")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                last_fecs_during_poll = Some(state);
                if !pri_fault && !halted && !stopped {
                    fecs_ever_running = true;
                    tracing::info!(
                        bdf = %bdf,
                        poll_count,
                        elapsed_ms = poll_start.elapsed().as_millis(),
                        "warm_handoff: FECS detected running"
                    );
                    if !keepalive {
                        break;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if !fecs_ever_running {
            tracing::warn!(
                bdf = %bdf,
                poll_count,
                timeout_ms = poll_timeout_ms,
                "warm_handoff: FECS never seen running during poll window"
            );
        }
    }

    // Step 7: Swap back to vfio-pci
    tracing::info!(bdf = %bdf, "warm_handoff: swapping back to vfio-pci");
    slot.swap_traced("vfio", false)
        .map_err(|e| RpcError::device_error(format!("swap back to vfio: {e}")))?;

    // Step 8: Capture post-swap FECS state
    let post_fecs = ember.fecs_state(&bdf).ok();

    let total_ms = handoff_start.elapsed().as_millis() as u64;
    tracing::info!(
        bdf = %bdf,
        total_ms,
        fecs_ever_running,
        poll_count,
        "warm_handoff complete"
    );

    Ok(serde_json::json!({
        "bdf": bdf,
        "driver": driver,
        "total_ms": total_ms,
        "settle_ms": settle_ms,
        "poll_fecs": poll_fecs,
        "poll_count": poll_count,
        "fecs_ever_running": fecs_ever_running,
        "pre_fecs": pre_fecs,
        "post_fecs": post_fecs,
        "last_fecs_during_poll": last_fecs_during_poll,
        "personality": slot.personality.to_string(),
        "vram_alive": slot.health.vram_alive,
    }))
}
