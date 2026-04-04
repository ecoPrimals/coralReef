// SPDX-License-Identifier: AGPL-3.0-only
//! GPU telemetry and quota management via nvidia-smi.
//!
//! These handlers isolate the nvidia-smi vendor tool dependency so it can
//! be replaced with a pure-Rust NVML implementation when available.

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
    let bdf: Arc<str> = Arc::from(validate_bdf(raw_bdf)?);

    let (chip, personality, role, protected) = {
        let devs = devices.lock().await;
        let slot = devs
            .iter()
            .find(|d| d.bdf == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::clone(&bdf),
            })
            .map_err(RpcError::from)?;
        (
            slot.chip_name.clone(),
            slot.personality.to_string(),
            slot.config.role.clone(),
            slot.config.is_protected(),
        )
    };

    let bdf_task = Arc::clone(&bdf);
    let info = tokio::task::spawn_blocking(move || query_nvidia_smi(&bdf_task))
        .await
        .map_err(|e| RpcError::internal(format!("nvidia-smi task panicked: {e}")))?;

    let render_node = coral_glowplug::sysfs::find_render_node(bdf.as_ref());
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
    let bdf: Arc<str> = Arc::from(validate_bdf(raw_bdf)?);

    let (role, protected, quota) = {
        let devs = devices.lock().await;
        let slot = devs
            .iter()
            .find(|d| d.bdf == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::clone(&bdf),
            })
            .map_err(RpcError::from)?;
        (
            slot.config.role.clone(),
            slot.config.is_protected(),
            slot.config.shared.as_ref().cloned().unwrap_or_default(),
        )
    };

    let bdf_task = Arc::clone(&bdf);
    let current = tokio::task::spawn_blocking(move || query_nvidia_smi(&bdf_task))
        .await
        .map_err(|e| RpcError::internal(format!("nvidia-smi task panicked: {e}")))?;

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
    let bdf: Arc<str> = Arc::from(validate_bdf(raw_bdf)?);

    let quota = {
        let mut devs = devices.lock().await;
        let slot = devs
            .iter_mut()
            .find(|d| d.bdf == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::clone(&bdf),
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

    let bdf_task = Arc::clone(&bdf);
    let quota2 = quota.clone();
    let results = tokio::task::spawn_blocking(move || apply_quota(bdf_task.as_ref(), &quota2))
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

/// Apply quota settings to a GPU via nvidia-smi.
fn apply_quota(bdf: &str, quota: &coral_glowplug::config::SharedQuota) -> serde_json::Value {
    let pci_bus_id = bdf.trim_start_matches("0000:");
    let mut results = serde_json::Map::new();

    if let Some(pl) = quota.power_limit_w {
        let out = std::process::Command::new("nvidia-smi")
            .args(["-i", pci_bus_id, &format!("--power-limit={pl}")])
            .output();
        let ok = out.as_ref().is_ok_and(|o| o.status.success());
        let msg = out
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|e| e.to_string());
        results.insert(
            "power_limit".into(),
            serde_json::json!({"ok": ok, "message": msg}),
        );
    }

    match quota.compute_mode.as_str() {
        "default" | "exclusive_process" | "prohibited" => {
            let mode_id = match quota.compute_mode.as_str() {
                "default" => "0",
                "exclusive_process" => "3",
                "prohibited" => "2",
                _ => "0",
            };
            let out = std::process::Command::new("nvidia-smi")
                .args(["-i", pci_bus_id, &format!("--compute-mode={mode_id}")])
                .output();
            let ok = out.as_ref().is_ok_and(|o| o.status.success());
            let msg = out
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|e| e.to_string());
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

/// Query nvidia-smi for GPU compute info.
///
/// Returns a JSON object with memory, clocks, power, temp.
/// Returns null fields if nvidia-smi is unavailable or the BDF doesn't match.
fn query_nvidia_smi(bdf: &str) -> serde_json::Value {
    let pci_bus_id = bdf.trim_start_matches("0000:");
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=gpu_name,memory.total,memory.free,memory.used,temperature.gpu,power.draw,power.limit,clocks.current.sm,clocks.current.memory,compute_cap,pcie.link.width.current",
            "--format=csv,noheader,nounits",
            &format!("--id={pci_bus_id}"),
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let fields: Vec<&str> = text.trim().splitn(11, ", ").collect();
            if fields.len() >= 11 {
                serde_json::json!({
                    "gpu_name": fields[0],
                    "memory_total_mib": fields[1].trim().parse::<f64>().unwrap_or(0.0),
                    "memory_free_mib": fields[2].trim().parse::<f64>().unwrap_or(0.0),
                    "memory_used_mib": fields[3].trim().parse::<f64>().unwrap_or(0.0),
                    "temperature_c": fields[4].trim().parse::<u32>().unwrap_or(0),
                    "power_draw_w": fields[5].trim().parse::<f64>().unwrap_or(0.0),
                    "power_limit_w": fields[6].trim().parse::<f64>().unwrap_or(0.0),
                    "clock_sm_mhz": fields[7].trim().parse::<u32>().unwrap_or(0),
                    "clock_mem_mhz": fields[8].trim().parse::<u32>().unwrap_or(0),
                    "compute_cap": fields[9].trim(),
                    "pcie_width": fields[10].trim().parse::<u32>().unwrap_or(0),
                })
            } else {
                serde_json::json!({"error": "unexpected nvidia-smi output format"})
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            serde_json::json!({"error": format!("nvidia-smi failed: {}", stderr.trim())})
        }
        Err(e) => serde_json::json!({"error": format!("nvidia-smi not available: {e}")}),
    }
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
    fn query_nvidia_smi_returns_json_object() {
        let v = query_nvidia_smi("0000:ff:00.0");
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
