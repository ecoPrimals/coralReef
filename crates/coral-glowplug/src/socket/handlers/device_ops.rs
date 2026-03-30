// SPDX-License-Identifier: AGPL-3.0-only
//! Synchronous device lifecycle handlers for glowplug JSON-RPC.
//!
//! These handlers run under the device lock and execute synchronously
//! (no `spawn_blocking`). They cover device listing, swap, health,
//! register access, lend/reclaim, and daemon status.

use std::sync::Arc;

use super::validate_bdf;
use super::{DeviceInfo, device_to_info};
use crate::socket::protocol::HealthInfo;

pub(crate) fn dispatch(
    method: &str,
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
    started_at: std::time::Instant,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    match method {
        "device.list" => {
            let infos: Vec<DeviceInfo> = devices.iter().map(device_to_info).collect();
            serde_json::to_value(infos).map_err(|e| RpcError::internal(e.to_string()))
        }
        "device.get" => {
            let bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(bdf)?;
            let slot = devices
                .iter()
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf),
                })
                .map_err(RpcError::from)?;
            serde_json::to_value(device_to_info(slot))
                .map_err(|e| RpcError::internal(e.to_string()))
        }
        "device.swap" => handle_swap(params, devices),
        "device.warm_handoff" => handle_warm_handoff(params, devices),
        "device.health" => handle_health(params, devices),
        "device.register_dump" => handle_register_dump(params, devices),
        "device.register_snapshot" => handle_register_snapshot(params, devices),
        "device.write_register" => handle_write_register(params, devices),
        "device.read_bar0_range" => handle_read_bar0_range(params, devices),
        "device.pramin_read" => handle_pramin_read(params, devices),
        "device.pramin_write" => handle_pramin_write(params, devices),
        "device.lend" => handle_lend(params, devices),
        "device.reclaim" => handle_reclaim(params, devices),
        "device.resurrect" => handle_resurrect(params, devices),
        "device.reset" => handle_reset(params, devices),
        "health.check" | "health.liveness" => Ok(serde_json::json!({
            "alive": true,
            "name": "coral-glowplug",
            "device_count": devices.len(),
            "healthy_count": devices.iter().filter(|d| d.health.vram_alive).count(),
        })),
        "daemon.status" => Ok(serde_json::json!({
            "uptime_secs": started_at.elapsed().as_secs(),
            "device_count": devices.len(),
            "healthy_count": devices.iter().filter(|d| d.health.vram_alive).count(),
        })),
        "device.compute_info" | "device.quota" => {
            Err(RpcError::internal("routed to async handler"))
        }
        "device.set_quota" => Err(RpcError::internal("routed to async handler")),
        "daemon.shutdown" => {
            tracing::info!("shutdown requested via JSON-RPC");
            Err(RpcError::device_error("shutdown"))
        }
        other => Err(RpcError::method_not_found(other)),
    }
}

fn handle_swap(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();
    let target = params
        .get("target")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'target' parameter"))?
        .to_owned();
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
            "device {bdf} is busy — cannot swap while a long-running operation is in progress"
        )));
    }
    slot.swap_traced(&target, enable_trace)
        .map_err(|e| RpcError::device_error(e.to_string()))?;

    let mut observer_insights = Vec::new();
    if let Some(ref obs) = slot.last_swap_observation {
        let registry = coral_glowplug::observer::ObserverRegistry::default_observers();
        observer_insights = registry.observe_swap(obs);

        if let Some(ref trace_path) = obs.trace_path
            && let Some(trace_insight) = registry.observe_trace(&obs.to_personality, trace_path)
        {
            observer_insights.push(trace_insight);
        }

        if !observer_insights.is_empty() {
            for insight in &observer_insights {
                tracing::info!(
                    bdf = %bdf,
                    personality = %insight.personality,
                    findings = insight.findings.len(),
                    "observer insight captured"
                );
            }
        }
    }

    let insights_json: Vec<serde_json::Value> = observer_insights
        .iter()
        .map(|i| serde_json::to_value(i).unwrap_or_default())
        .collect();

    Ok(serde_json::json!({
        "bdf": bdf,
        "personality": slot.personality.to_string(),
        "vram_alive": slot.health.vram_alive,
        "observation": slot.last_swap_observation.as_ref().map(|o| {
            serde_json::json!({
                "total_ms": o.timing.total_ms,
                "bind_ms": o.timing.bind_ms,
                "unbind_ms": o.timing.unbind_ms,
                "trace_path": o.trace_path,
            })
        }),
        "insights": insights_json,
    }))
}

fn handle_warm_handoff(
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
                let halted = state
                    .get("halted")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let stopped = state
                    .get("stopped")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                last_fecs_during_poll = Some(state);
                if !halted && !stopped {
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

fn handle_health(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(bdf)?;
    let slot = devices
        .iter_mut()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf),
        })
        .map_err(RpcError::from)?;
    slot.check_health();
    serde_json::to_value(HealthInfo {
        bdf: bdf.to_owned(),
        boot0: slot.health.boot0,
        pmc_enable: slot.health.pmc_enable,
        vram_alive: slot.health.vram_alive,
        power: slot.health.power.to_string(),
        domains_alive: slot.health.domains_alive,
        domains_faulted: slot.health.domains_faulted,
        fecs_cpuctl: slot.health.firmware.fecs_cpuctl,
        fecs_stopped: slot.health.firmware.fecs_stopped,
        fecs_halted: slot.health.firmware.fecs_halted,
        fecs_sctl: slot.health.firmware.fecs_sctl,
        gpccs_cpuctl: slot.health.firmware.gpccs_cpuctl,
    })
    .map_err(|e| RpcError::internal(e.to_string()))
}

fn handle_register_dump(
    params: &serde_json::Value,
    devices: &[coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(bdf)?;
    let slot = devices
        .iter()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf),
        })
        .map_err(RpcError::from)?;
    if !slot.has_vfio() {
        return Err(RpcError::device_error(format!(
            "device {bdf} has no VFIO fd — register reads require VFIO personality"
        )));
    }
    let custom_offsets: Vec<usize> = params
        .get("offsets")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_u64().map(|n| n as usize))
                .collect()
        })
        .unwrap_or_default();
    let regs = slot.dump_registers(&custom_offsets);
    let entries: Vec<serde_json::Value> = regs
        .iter()
        .map(|(off, val)| serde_json::json!({"offset": format!("{off:#010x}"), "value": format!("{val:#010x}"), "raw_offset": off, "raw_value": val}))
        .collect();
    Ok(serde_json::json!({"bdf": bdf, "register_count": entries.len(), "registers": entries}))
}

fn handle_register_snapshot(
    params: &serde_json::Value,
    devices: &[coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(bdf)?;
    let slot = devices
        .iter()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf),
        })
        .map_err(RpcError::from)?;
    let snap = slot.last_snapshot();
    let entries: Vec<serde_json::Value> = snap
        .iter()
        .map(|(off, val)| serde_json::json!({"offset": format!("{off:#010x}"), "value": format!("{val:#010x}"), "raw_offset": off, "raw_value": val}))
        .collect();
    Ok(serde_json::json!({"bdf": bdf, "register_count": entries.len(), "registers": entries}))
}

fn handle_write_register(
    params: &serde_json::Value,
    devices: &[coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(bdf)?;
    let offset = params
        .get("offset")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::invalid_params("missing 'offset' parameter"))?
        as usize;
    let value = params
        .get("value")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::invalid_params("missing 'value' parameter"))?
        as u32;
    let allow_dangerous = params
        .get("allow_dangerous")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let slot = devices
        .iter()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf),
        })
        .map_err(RpcError::from)?;
    slot.write_register(offset, value, allow_dangerous)
        .map_err(|e| RpcError::device_error(e.to_string()))?;
    Ok(serde_json::json!({
        "bdf": bdf,
        "offset": format!("{offset:#010x}"),
        "value": format!("{value:#010x}"),
    }))
}

fn handle_read_bar0_range(
    params: &serde_json::Value,
    devices: &[coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(bdf)?;
    let offset = params
        .get("offset")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::invalid_params("missing 'offset' parameter"))?
        as usize;
    let count = params
        .get("count")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::invalid_params("missing 'count' parameter"))?
        as usize;
    if count > 4096 {
        return Err(RpcError::invalid_params("count exceeds 4096 maximum"));
    }
    let slot = devices
        .iter()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf),
        })
        .map_err(RpcError::from)?;
    let values = slot.read_bar0_range(offset, count);
    Ok(serde_json::json!({
        "bdf": bdf,
        "offset": format!("{offset:#010x}"),
        "count": values.len(),
        "values": values,
    }))
}

fn handle_pramin_read(
    params: &serde_json::Value,
    devices: &[coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(bdf)?;
    let vram_offset = params
        .get("vram_offset")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::invalid_params("missing 'vram_offset' parameter"))?;
    let count = params
        .get("count")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::invalid_params("missing 'count' parameter"))?
        as usize;
    if count > 4096 {
        return Err(RpcError::invalid_params("count exceeds 4096 maximum"));
    }
    let slot = devices
        .iter()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf),
        })
        .map_err(RpcError::from)?;
    let values = slot
        .pramin_read(vram_offset, count)
        .map_err(|e| RpcError::device_error(e.to_string()))?;
    Ok(serde_json::json!({
        "bdf": bdf,
        "vram_offset": format!("{vram_offset:#010x}"),
        "count": values.len(),
        "values": values,
    }))
}

fn handle_pramin_write(
    params: &serde_json::Value,
    devices: &[coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(bdf)?;
    let vram_offset = params
        .get("vram_offset")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::invalid_params("missing 'vram_offset' parameter"))?;
    let values: Vec<u32> = params
        .get("values")
        .and_then(|v| v.as_array())
        .ok_or_else(|| RpcError::invalid_params("missing 'values' array parameter"))?
        .iter()
        .filter_map(|v| v.as_u64().map(|n| n as u32))
        .collect();
    if values.len() > 4096 {
        return Err(RpcError::invalid_params(
            "values array exceeds 4096 maximum",
        ));
    }
    let slot = devices
        .iter()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf),
        })
        .map_err(RpcError::from)?;
    slot.pramin_write(vram_offset, &values)
        .map_err(|e| RpcError::device_error(e.to_string()))?;
    Ok(serde_json::json!({
        "bdf": bdf,
        "vram_offset": format!("{vram_offset:#010x}"),
        "count": values.len(),
    }))
}

fn handle_lend(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();
    let slot = devices
        .iter_mut()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf.as_str()),
        })
        .map_err(RpcError::from)?;
    let group_id = slot
        .lend()
        .map_err(|e| RpcError::device_error(e.to_string()))?;
    Ok(serde_json::json!({
        "bdf": bdf,
        "group_id": group_id,
        "personality": slot.personality.to_string(),
    }))
}

fn handle_reclaim(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();
    let slot = devices
        .iter_mut()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf.as_str()),
        })
        .map_err(RpcError::from)?;
    if slot.is_busy() {
        return Err(RpcError::device_error(format!(
            "device {bdf} is busy — cannot reclaim while a long-running operation is in progress"
        )));
    }
    slot.reclaim()
        .map_err(|e| RpcError::device_error(e.to_string()))?;
    Ok(serde_json::json!({
        "bdf": bdf,
        "personality": slot.personality.to_string(),
        "vram_alive": slot.health.vram_alive,
        "has_vfio_fd": slot.has_vfio(),
    }))
}

fn handle_resurrect(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();
    let slot = devices
        .iter_mut()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf.as_str()),
        })
        .map_err(RpcError::from)?;
    if slot.is_busy() {
        return Err(RpcError::device_error(format!(
            "device {bdf} is busy — cannot resurrect while a long-running operation is in progress"
        )));
    }
    let alive = slot
        .resurrect_hbm2()
        .map_err(|e| RpcError::device_error(e.to_string()))?;
    Ok(serde_json::json!({
        "bdf": bdf,
        "vram_alive": alive,
        "domains_alive": slot.health.domains_alive,
    }))
}

fn handle_reset(
    params: &serde_json::Value,
    devices: &[coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let method = params
        .get("method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("flr");
    let bdf = validate_bdf(raw_bdf)?.to_owned();
    let slot = devices
        .iter()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf.as_str()),
        })
        .map_err(RpcError::from)?;
    if slot.is_busy() {
        return Err(RpcError::device_error(format!(
            "device {bdf} is busy — cannot reset while a long-running operation is in progress"
        )));
    }

    match method {
        "flr" => {
            slot.reset_device()
                .map_err(|e| RpcError::device_error(e.to_string()))?;
            tracing::info!(bdf = %bdf, "PCIe FLR completed via VFIO_DEVICE_RESET");
            Ok(serde_json::json!({
                "bdf": bdf,
                "reset": true,
                "method": "flr",
            }))
        }
        "sbr" | "bridge-sbr" | "remove-rescan" | "auto" => {
            let ember = coral_glowplug::ember::EmberClient::connect().ok_or_else(|| {
                RpcError::device_error("ember not available — cannot perform reset".to_string())
            })?;
            ember
                .device_reset(&bdf, method)
                .map_err(|e| RpcError::device_error(format!("ember reset: {e}")))?;
            tracing::info!(bdf = %bdf, method, "PCI device reset completed via ember");
            Ok(serde_json::json!({
                "bdf": bdf,
                "reset": true,
                "method": method,
            }))
        }
        other => Err(RpcError::invalid_params(format!(
            "unknown reset method '{other}' (use: auto, flr, sbr, bridge-sbr, remove-rescan)"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::socket::handlers::test_device_config;
    use coral_glowplug::device::DeviceSlot;
    use std::time::Instant;

    #[test]
    fn dispatch_swap_rejects_when_device_busy() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let _guard = devices[0]
            .try_acquire_busy()
            .expect("slot should not start busy");
        let started = Instant::now();
        let err = dispatch(
            "device.swap",
            &serde_json::json!({"bdf": "0000:99:00.0", "target": "nouveau"}),
            &mut devices,
            started,
        )
        .expect_err("swap while busy");
        assert_eq!(i32::from(err.code), -32000);
        assert!(err.message.contains("busy"), "{}", err.message);
    }

    #[test]
    fn dispatch_reclaim_rejects_when_device_busy() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let _guard = devices[0]
            .try_acquire_busy()
            .expect("slot should not start busy");
        let started = Instant::now();
        let err = dispatch(
            "device.reclaim",
            &serde_json::json!({"bdf": "0000:99:00.0"}),
            &mut devices,
            started,
        )
        .expect_err("reclaim while busy");
        assert_eq!(i32::from(err.code), -32000);
        assert!(err.message.contains("busy"), "{}", err.message);
    }

    #[test]
    fn dispatch_resurrect_rejects_when_device_busy() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let _guard = devices[0]
            .try_acquire_busy()
            .expect("slot should not start busy");
        let started = Instant::now();
        let err = dispatch(
            "device.resurrect",
            &serde_json::json!({"bdf": "0000:99:00.0"}),
            &mut devices,
            started,
        )
        .expect_err("resurrect while busy");
        assert_eq!(i32::from(err.code), -32000);
        assert!(err.message.contains("busy"), "{}", err.message);
    }

    #[test]
    fn dispatch_reset_rejects_when_device_busy() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let _guard = devices[0]
            .try_acquire_busy()
            .expect("slot should not start busy");
        let started = Instant::now();
        let err = dispatch(
            "device.reset",
            &serde_json::json!({"bdf": "0000:99:00.0"}),
            &mut devices,
            started,
        )
        .expect_err("reset while busy");
        assert_eq!(i32::from(err.code), -32000);
        assert!(err.message.contains("busy"), "{}", err.message);
    }

    #[test]
    fn dispatch_write_register_missing_offset() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let started = Instant::now();
        let err = dispatch(
            "device.write_register",
            &serde_json::json!({"bdf": "0000:99:00.0", "value": 0}),
            &mut devices,
            started,
        )
        .expect_err("missing offset");
        assert_eq!(i32::from(err.code), -32602);
        assert!(err.message.contains("offset"), "{}", err.message);
    }

    #[test]
    fn dispatch_write_register_missing_value() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let started = Instant::now();
        let err = dispatch(
            "device.write_register",
            &serde_json::json!({"bdf": "0000:99:00.0", "offset": 0}),
            &mut devices,
            started,
        )
        .expect_err("missing value");
        assert_eq!(i32::from(err.code), -32602);
        assert!(err.message.contains("value"), "{}", err.message);
    }

    #[test]
    fn dispatch_read_bar0_range_missing_count() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let started = Instant::now();
        let err = dispatch(
            "device.read_bar0_range",
            &serde_json::json!({"bdf": "0000:99:00.0", "offset": 0}),
            &mut devices,
            started,
        )
        .expect_err("missing count");
        assert_eq!(i32::from(err.code), -32602);
        assert!(err.message.contains("count"), "{}", err.message);
    }

    #[test]
    fn dispatch_pramin_read_missing_vram_offset() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let started = Instant::now();
        let err = dispatch(
            "device.pramin_read",
            &serde_json::json!({"bdf": "0000:99:00.0", "count": 1}),
            &mut devices,
            started,
        )
        .expect_err("missing vram_offset");
        assert_eq!(i32::from(err.code), -32602);
        assert!(err.message.contains("vram_offset"), "{}", err.message);
    }

    #[test]
    fn dispatch_pramin_read_missing_count() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let started = Instant::now();
        let err = dispatch(
            "device.pramin_read",
            &serde_json::json!({"bdf": "0000:99:00.0", "vram_offset": 0}),
            &mut devices,
            started,
        )
        .expect_err("missing count");
        assert_eq!(i32::from(err.code), -32602);
        assert!(err.message.contains("count"), "{}", err.message);
    }

    #[test]
    fn dispatch_device_swap_invalid_bdf() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let started = Instant::now();
        let err = dispatch(
            "device.swap",
            &serde_json::json!({"bdf": "not-a-bdf", "target": "nouveau"}),
            &mut devices,
            started,
        )
        .expect_err("invalid bdf");
        assert_eq!(i32::from(err.code), -32602);
    }

    #[test]
    fn dispatch_warm_handoff_rejects_when_device_busy() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let _guard = devices[0]
            .try_acquire_busy()
            .expect("slot should not start busy");
        let started = Instant::now();
        let err = dispatch(
            "device.warm_handoff",
            &serde_json::json!({"bdf": "0000:99:00.0"}),
            &mut devices,
            started,
        )
        .expect_err("warm_handoff while busy");
        assert_eq!(i32::from(err.code), -32000);
        assert!(err.message.contains("busy"), "{}", err.message);
    }

    #[test]
    fn dispatch_warm_handoff_missing_bdf() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let started = Instant::now();
        let err = dispatch(
            "device.warm_handoff",
            &serde_json::json!({}),
            &mut devices,
            started,
        )
        .expect_err("missing bdf");
        assert_eq!(i32::from(err.code), -32602);
    }

    #[test]
    fn dispatch_warm_handoff_invalid_bdf() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let started = Instant::now();
        let err = dispatch(
            "device.warm_handoff",
            &serde_json::json!({"bdf": "not-valid"}),
            &mut devices,
            started,
        )
        .expect_err("invalid bdf");
        assert_eq!(i32::from(err.code), -32602);
    }

    #[test]
    fn dispatch_warm_handoff_unknown_device() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let started = Instant::now();
        let err = dispatch(
            "device.warm_handoff",
            &serde_json::json!({"bdf": "0000:ff:00.0"}),
            &mut devices,
            started,
        )
        .expect_err("device not managed");
        assert_eq!(i32::from(err.code), -32000);
    }
}
