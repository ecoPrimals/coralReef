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

pub(crate) use super::resurrect::handle_resurrect;
pub(crate) use super::cold_boot::handle_cold_boot;
pub(crate) use super::warm_handoff::handle_warm_handoff;

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
        "device.cold_boot" => handle_cold_boot(params, devices),
        "ember.deploy" => handle_ember_deploy(params),
        "device.health" => handle_health(params, devices),
        "device.register_dump" => super::register_ops::handle_register_dump(params, devices),
        "device.register_snapshot" => {
            super::register_ops::handle_register_snapshot(params, devices)
        }
        "device.write_register" => super::register_ops::handle_write_register(params, devices),
        "device.read_bar0_range" => super::register_ops::handle_read_bar0_range(params, devices),
        "device.pramin_read" => super::register_ops::handle_pramin_read(params, devices),
        "device.pramin_write" => super::register_ops::handle_pramin_write(params, devices),
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
        "health.readiness" => {
            let healthy = devices.iter().filter(|d| d.health.vram_alive).count();
            let total = devices.len();
            Ok(serde_json::json!({
                "ready": total > 0 && healthy == total,
                "name": "coral-glowplug",
                "device_count": total,
                "healthy_count": healthy,
                "uptime_secs": started_at.elapsed().as_secs(),
            }))
        }
        "capabilities.list" | "capability.list" | "primal.capabilities" => Ok(serde_json::json!({
            "name": "coral-glowplug",
            "version": env!("CARGO_PKG_VERSION"),
            "capabilities": [
                "device.list",
                "device.get",
                "device.swap",
                "device.warm_handoff",
                "device.cold_boot",
                "device.health",
                "device.dispatch",
                "device.dispatch_sovereign",
                "device.oracle_capture",
                "device.register_dump",
                "device.register_snapshot",
                "device.write_register",
                "device.read_bar0_range",
                "device.pramin_read",
                "device.pramin_write",
                "device.lend",
                "device.reclaim",
                "device.resurrect",
                "device.reset",
                "device.compute_info",
                "device.quota",
                "device.set_quota",
                "mailbox.create",
                "mailbox.post",
                "mailbox.poll",
                "mailbox.complete",
                "mailbox.drain",
                "mailbox.stats",
                "ring.create",
                "ring.submit",
                "ring.consume",
                "ring.fence",
                "ring.peek",
                "ring.stats",
                "health.check",
                "health.liveness",
                "health.readiness",
                "capabilities.list",
                "daemon.status",
                "daemon.shutdown",
            ],
            "sovereign": ["device.dispatch_sovereign", "device.cold_boot"],
            "transitional": ["device.dispatch"],
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
        .unwrap_or("auto");
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
        "flr" | "pmc" | "sbr" | "bridge-sbr" | "remove-rescan" | "auto" => {
            let ember = coral_glowplug::ember::EmberClient::connect().ok_or_else(|| {
                RpcError::device_error(
                    "ember not available — all resets route through ember (FD holder)"
                        .to_string(),
                )
            })?;
            ember
                .device_reset(&bdf, method)
                .map_err(|e| RpcError::device_error(format!("ember reset ({method}): {e}")))?;
            tracing::info!(bdf = %bdf, method, "device reset completed via ember");
            Ok(serde_json::json!({
                "bdf": bdf,
                "reset": true,
                "method": method,
            }))
        }
        other => Err(RpcError::invalid_params(format!(
            "unknown reset method '{other}' (use: auto, flr, pmc, sbr, bridge-sbr, remove-rescan)"
        ))),
    }
}

/// Forward `ember.deploy` RPC to ember. Glowplug acts as a proxy since
/// coralctl connects to glowplug's socket (ember's socket is sandboxed).
fn handle_ember_deploy(
    params: &serde_json::Value,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let ember = coral_glowplug::ember::EmberClient::connect().ok_or_else(|| {
        RpcError::device_error("ember not available — deploy requires ember")
    })?;

    let restart = params
        .get("restart")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    ember
        .deploy("/run/coralreef/staging", restart)
        .map_err(|e| RpcError::device_error(format!("ember deploy: {e}")))
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
