// SPDX-License-Identifier: AGPL-3.0-only
//! GPU telemetry and quota management via NVML (libnvidia-ml).
//!
//! Uses the `nvml-wrapper` crate for direct NVML API access, avoiding
//! the overhead and fragility of spawning `nvidia-smi` subprocesses.

use std::sync::Arc;
use tokio::sync::Mutex;

use super::validate_bdf;

/// Query GPU compute info via nvidia-smi, releasing the device lock first.
pub(crate) async fn compute_info_async(
    params: &serde_json::Value,
    devices: &Mutex<Vec<coral_glowplug::device::DeviceSlot>>,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();

    let (chip, personality, role, protected) = {
        let devs = devices.lock().await;
        let slot = devs
            .iter()
            .find(|d| d.bdf.as_ref() == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::from(bdf.as_str()),
            })
            .map_err(RpcError::from)?;
        (
            slot.chip_name.clone(),
            slot.personality.to_string(),
            slot.config.role.clone(),
            slot.config.is_protected(),
        )
    };

    let bdf2 = bdf.clone();
    let info = tokio::task::spawn_blocking(move || query_nvml(&bdf2))
        .await
        .map_err(|e| RpcError::internal(format!("NVML query task panicked: {e}")))?;

    let render_node = coral_glowplug::sysfs::find_render_node(&bdf);
    Ok(serde_json::json!({
        "bdf": bdf,
        "chip": chip,
        "personality": personality,
        "role": role,
        "protected": protected,
        "render_node": render_node,
        "compute": info,
    }))
}

/// Query GPU quota info via nvidia-smi, releasing the device lock first.
pub(crate) async fn quota_info_async(
    params: &serde_json::Value,
    devices: &Mutex<Vec<coral_glowplug::device::DeviceSlot>>,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();

    let (role, protected, quota) = {
        let devs = devices.lock().await;
        let slot = devs
            .iter()
            .find(|d| d.bdf.as_ref() == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::from(bdf.as_str()),
            })
            .map_err(RpcError::from)?;
        (
            slot.config.role.clone(),
            slot.config.is_protected(),
            slot.config.shared.as_ref().cloned().unwrap_or_default(),
        )
    };

    let bdf2 = bdf.clone();
    let current = tokio::task::spawn_blocking(move || query_nvml(&bdf2))
        .await
        .map_err(|e| RpcError::internal(format!("NVML query task panicked: {e}")))?;

    Ok(serde_json::json!({
        "bdf": bdf,
        "role": role,
        "protected": protected,
        "quota": {
            "power_limit_w": quota.power_limit_w,
            "vram_budget_mib": quota.vram_budget_mib,
            "compute_mode": quota.compute_mode,
            "compute_priority": quota.compute_priority,
        },
        "current": current,
    }))
}

/// Set GPU quota and apply via nvidia-smi, releasing the device lock for the
/// blocking nvidia-smi call.
pub(crate) async fn set_quota_async(
    params: &serde_json::Value,
    devices: &Mutex<Vec<coral_glowplug::device::DeviceSlot>>,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();

    let quota = {
        let mut devs = devices.lock().await;
        let slot = devs
            .iter_mut()
            .find(|d| d.bdf.as_ref() == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::from(bdf.as_str()),
            })
            .map_err(RpcError::from)?;

        if !slot.config.is_shared() && !slot.config.is_display() {
            return Err(RpcError::device_error(
                "set_quota only applies to role=shared or role=display devices",
            ));
        }

        let mut quota = slot.config.shared.clone().unwrap_or_default();
        if let Some(pl) = params.get("power_limit_w").and_then(|v| v.as_u64()) {
            quota.power_limit_w = Some(pl as u32);
        }
        if let Some(vb) = params.get("vram_budget_mib").and_then(|v| v.as_u64()) {
            quota.vram_budget_mib = Some(vb as u32);
        }
        if let Some(cm) = params.get("compute_mode").and_then(|v| v.as_str()) {
            quota.compute_mode = cm.to_string();
        }
        if let Some(cp) = params.get("compute_priority").and_then(|v| v.as_u64()) {
            quota.compute_priority = cp as u32;
        }

        slot.config.shared = Some(quota.clone());
        quota
    };

    let bdf2 = bdf.clone();
    let quota2 = quota.clone();
    let results = tokio::task::spawn_blocking(move || apply_quota(&bdf2, &quota2))
        .await
        .map_err(|e| RpcError::internal(format!("nvidia-smi task panicked: {e}")))?;

    Ok(serde_json::json!({
        "bdf": bdf,
        "quota": {
            "power_limit_w": quota.power_limit_w,
            "vram_budget_mib": quota.vram_budget_mib,
            "compute_mode": quota.compute_mode,
            "compute_priority": quota.compute_priority,
        },
        "applied": results,
    }))
}

/// Apply quota settings to a GPU via NVML.
fn apply_quota(bdf: &str, quota: &coral_glowplug::config::SharedQuota) -> serde_json::Value {
    let pci_bus_id = bdf.trim_start_matches("0000:");
    let mut results = serde_json::Map::new();

    let nvml = match nvml_wrapper::Nvml::init() {
        Ok(n) => n,
        Err(e) => {
            results.insert(
                "nvml_init".into(),
                serde_json::json!({"ok": false, "message": format!("NVML init failed: {e}")}),
            );
            return serde_json::Value::Object(results);
        }
    };

    let mut device = match nvml.device_by_pci_bus_id(pci_bus_id) {
        Ok(d) => d,
        Err(e) => {
            results.insert(
                "device".into(),
                serde_json::json!({"ok": false, "message": format!("NVML device lookup failed for {pci_bus_id}: {e}")}),
            );
            return serde_json::Value::Object(results);
        }
    };

    if let Some(pl) = quota.power_limit_w {
        let milliwatts = pl * 1000;
        let result = device.set_power_management_limit(milliwatts);
        let (ok, msg) = match result {
            Ok(()) => (true, format!("power limit set to {pl}W")),
            Err(e) => (false, format!("{e}")),
        };
        results.insert(
            "power_limit".into(),
            serde_json::json!({"ok": ok, "message": msg}),
        );
    }

    match quota.compute_mode.as_str() {
        "default" | "exclusive_process" | "prohibited" => {
            use nvml_wrapper::enum_wrappers::device::ComputeMode;
            let mode = match quota.compute_mode.as_str() {
                "exclusive_process" => ComputeMode::ExclusiveProcess,
                "prohibited" => ComputeMode::Prohibited,
                _ => ComputeMode::Default,
            };
            let result = device.set_compute_mode(mode);
            let (ok, msg) = match result {
                Ok(()) => (true, format!("compute mode set to {}", quota.compute_mode)),
                Err(e) => (false, format!("{e}")),
            };
            results.insert(
                "compute_mode".into(),
                serde_json::json!({"ok": ok, "message": msg}),
            );
        }
        _ => {
            results.insert(
                "compute_mode".into(),
                serde_json::json!({"ok": false, "message": "unknown mode"}),
            );
        }
    }

    serde_json::Value::Object(results)
}

/// Query GPU compute info via NVML.
///
/// Returns a JSON object with memory, clocks, power, temp.
/// Falls back to error JSON if NVML is unavailable (e.g., nvidia driver not loaded).
fn query_nvml(bdf: &str) -> serde_json::Value {
    let pci_bus_id = bdf.trim_start_matches("0000:");

    let nvml = match nvml_wrapper::Nvml::init() {
        Ok(n) => n,
        Err(e) => return serde_json::json!({"error": format!("NVML not available: {e}")}),
    };

    let device = match nvml.device_by_pci_bus_id(pci_bus_id) {
        Ok(d) => d,
        Err(e) => {
            return serde_json::json!({"error": format!("device not found in NVML for {pci_bus_id}: {e}")});
        }
    };

    let gpu_name = device.name().unwrap_or_else(|_| "unknown".into());
    let mem = device
        .memory_info()
        .map(|m| {
            (
                m.total / (1024 * 1024),
                m.free / (1024 * 1024),
                m.used / (1024 * 1024),
            )
        })
        .unwrap_or((0, 0, 0));
    let temp = device
        .temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)
        .unwrap_or(0);
    let power_draw = device.power_usage().unwrap_or(0) as f64 / 1000.0;
    let power_limit = device
        .enforced_power_limit()
        .or_else(|_| device.power_management_limit())
        .unwrap_or(0) as f64
        / 1000.0;
    let clock_sm = device
        .clock_info(nvml_wrapper::enum_wrappers::device::Clock::Graphics)
        .unwrap_or(0);
    let clock_mem = device
        .clock_info(nvml_wrapper::enum_wrappers::device::Clock::Memory)
        .unwrap_or(0);
    let cc = device
        .cuda_compute_capability()
        .map(|c| (c.major, c.minor))
        .unwrap_or((0, 0));
    let (major, minor) = cc;
    let pcie_width = device.current_pcie_link_width().unwrap_or(0);

    serde_json::json!({
        "gpu_name": gpu_name,
        "memory_total_mib": mem.0 as f64,
        "memory_free_mib": mem.1 as f64,
        "memory_used_mib": mem.2 as f64,
        "temperature_c": temp,
        "power_draw_w": power_draw,
        "power_limit_w": power_limit,
        "clock_sm_mhz": clock_sm,
        "clock_mem_mhz": clock_mem,
        "compute_cap": format!("{major}.{minor}"),
        "pcie_width": pcie_width,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::socket::handlers::test_device_config;
    use coral_glowplug::config::SharedQuota;
    use coral_glowplug::device::DeviceSlot;

    #[test]
    fn apply_quota_unknown_compute_mode_records_failure() {
        let quota = SharedQuota {
            power_limit_w: None,
            vram_budget_mib: None,
            compute_mode: "bogus_mode".into(),
            compute_priority: 0,
        };
        let v = apply_quota("0000:01:00.0", &quota);
        let cm = v
            .as_object()
            .expect("apply_quota returns JSON object")
            .get("compute_mode")
            .expect("compute_mode key");
        assert_eq!(cm["ok"], false);
        assert_eq!(cm["message"], "unknown mode");
    }

    #[test]
    fn apply_quota_power_limit_and_default_compute_mode_produce_structured_results() {
        let quota = SharedQuota {
            power_limit_w: Some(250),
            vram_budget_mib: None,
            compute_mode: "default".into(),
            compute_priority: 0,
        };
        let v = apply_quota("0000:01:00.0", &quota);
        let obj = v.as_object().expect("apply_quota returns JSON object");
        let pl = obj
            .get("power_limit")
            .expect("power_limit branch")
            .as_object()
            .expect("power_limit object");
        assert!(pl.contains_key("ok"));
        assert!(pl.contains_key("message"));
        let cm = obj
            .get("compute_mode")
            .expect("compute_mode branch")
            .as_object()
            .expect("compute_mode object");
        assert!(cm.contains_key("ok"));
        assert!(cm.contains_key("message"));
    }

    #[test]
    fn apply_quota_exclusive_process_compute_mode_invokes_nvidia_smi_branch() {
        let quota = SharedQuota {
            power_limit_w: None,
            vram_budget_mib: None,
            compute_mode: "exclusive_process".into(),
            compute_priority: 0,
        };
        let v = apply_quota("0000:01:00.0", &quota);
        let obj = v.as_object().expect("apply_quota returns JSON object");
        let cm = obj
            .get("compute_mode")
            .expect("compute_mode")
            .as_object()
            .expect("compute_mode object");
        assert!(cm.contains_key("ok"));
        assert!(cm.contains_key("message"));
    }

    #[test]
    fn apply_quota_prohibited_compute_mode_invokes_nvidia_smi_branch() {
        let quota = SharedQuota {
            power_limit_w: None,
            vram_budget_mib: None,
            compute_mode: "prohibited".into(),
            compute_priority: 0,
        };
        let v = apply_quota("0000:01:00.0", &quota);
        let obj = v.as_object().expect("apply_quota returns JSON object");
        let cm = obj
            .get("compute_mode")
            .expect("compute_mode")
            .as_object()
            .expect("compute_mode object");
        assert!(cm.contains_key("ok"));
        assert!(cm.contains_key("message"));
    }

    #[test]
    fn query_nvml_returns_json_object() {
        let v = query_nvml("0000:ff:00.0");
        assert!(
            v.get("error").is_some() || v.get("gpu_name").is_some(),
            "expected error or metrics: {v}"
        );
    }

    #[tokio::test]
    async fn compute_info_async_missing_bdf() {
        let devices = Mutex::new(vec![]);
        let err = compute_info_async(&serde_json::json!({}), &devices)
            .await
            .expect_err("missing bdf");
        assert_eq!(i32::from(err.code), -32602);
    }

    #[tokio::test]
    async fn compute_info_async_invalid_bdf() {
        let devices = Mutex::new(vec![]);
        let err = compute_info_async(&serde_json::json!({"bdf": "x/y/z"}), &devices)
            .await
            .expect_err("invalid bdf");
        assert_eq!(i32::from(err.code), -32602);
    }

    #[tokio::test]
    async fn compute_info_async_device_not_managed() {
        let devices = Mutex::new(vec![]);
        let err = compute_info_async(&serde_json::json!({"bdf": "0000:01:00.0"}), &devices)
            .await
            .expect_err("not managed");
        assert_eq!(i32::from(err.code), -32000);
    }

    #[tokio::test]
    async fn compute_info_async_returns_structured_json_with_slot() {
        let devices = Mutex::new(vec![DeviceSlot::new(test_device_config("0000:99:00.0"))]);
        let val = compute_info_async(&serde_json::json!({"bdf": "0000:99:00.0"}), &devices)
            .await
            .expect("compute_info");
        assert_eq!(val["bdf"], "0000:99:00.0");
        assert!(val.get("chip").is_some());
        assert!(val.get("personality").is_some());
        assert!(val.get("role").is_some());
        assert!(val.get("protected").is_some());
        assert!(val.get("render_node").is_some());
        assert!(val.get("compute").is_some());
    }

    #[tokio::test]
    async fn quota_info_async_missing_bdf() {
        let devices = Mutex::new(vec![]);
        let err = quota_info_async(&serde_json::json!({}), &devices)
            .await
            .expect_err("missing bdf");
        assert_eq!(i32::from(err.code), -32602);
    }

    #[tokio::test]
    async fn quota_info_async_not_managed() {
        let devices = Mutex::new(vec![]);
        let err = quota_info_async(&serde_json::json!({"bdf": "0000:02:00.0"}), &devices)
            .await
            .expect_err("not managed");
        assert_eq!(i32::from(err.code), -32000);
    }

    #[tokio::test]
    async fn quota_info_async_returns_quota_and_current() {
        let devices = Mutex::new(vec![DeviceSlot::new(test_device_config("0000:99:00.0"))]);
        let val = quota_info_async(&serde_json::json!({"bdf": "0000:99:00.0"}), &devices)
            .await
            .expect("quota_info");
        assert_eq!(val["bdf"], "0000:99:00.0");
        assert!(val.get("quota").is_some());
        assert!(val.get("current").is_some());
        assert!(val.get("protected").is_some());
    }

    #[tokio::test]
    async fn set_quota_async_missing_bdf() {
        let devices = Mutex::new(vec![]);
        let err = set_quota_async(&serde_json::json!({}), &devices)
            .await
            .expect_err("missing bdf");
        assert_eq!(i32::from(err.code), -32602);
    }

    #[tokio::test]
    async fn set_quota_async_rejects_non_shared_non_display_role() {
        let mut cfg = test_device_config("0000:99:00.0");
        cfg.role = Some("compute".into());
        let devices = Mutex::new(vec![DeviceSlot::new(cfg)]);
        let err = set_quota_async(
            &serde_json::json!({"bdf": "0000:99:00.0", "power_limit_w": 200}),
            &devices,
        )
        .await
        .expect_err("wrong role");
        assert_eq!(i32::from(err.code), -32000);
        assert!(
            err.message.contains("shared") || err.message.contains("display"),
            "{}",
            err.message
        );
    }

    #[tokio::test]
    async fn set_quota_async_applies_for_shared_role() {
        let mut cfg = test_device_config("0000:99:00.0");
        cfg.role = Some("shared".into());
        cfg.shared = Some(SharedQuota {
            power_limit_w: None,
            vram_budget_mib: None,
            compute_mode: "default".into(),
            compute_priority: 0,
        });
        let devices = Mutex::new(vec![DeviceSlot::new(cfg)]);
        let val = set_quota_async(
            &serde_json::json!({
                "bdf": "0000:99:00.0",
                "power_limit_w": 100,
                "vram_budget_mib": 1024,
                "compute_mode": "default",
                "compute_priority": 0,
            }),
            &devices,
        )
        .await
        .expect("set_quota shared");
        assert_eq!(val["bdf"], "0000:99:00.0");
        assert!(val.get("applied").is_some());
        let q = val["quota"].as_object().expect("quota object");
        assert_eq!(q["power_limit_w"], 100);
        assert_eq!(q["vram_budget_mib"], 1024);
    }
}
