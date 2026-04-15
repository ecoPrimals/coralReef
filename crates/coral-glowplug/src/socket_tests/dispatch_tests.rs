// SPDX-License-Identifier: AGPL-3.0-or-later

use super::super::*;

#[test]
fn test_dispatch_device_list_empty() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch("device.list", &serde_json::json!({}), &mut devices, started);
    let val = result.expect("device.list should succeed");
    let arr = val.as_array().expect("should be array");
    assert!(arr.is_empty());
}

#[test]
fn test_dispatch_device_list_with_devices() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: Some("Test GPU".into()),
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: Some("compute".into()),
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch("device.list", &serde_json::json!({}), &mut devices, started);
    let val = result.expect("device.list should succeed");
    let arr = val.as_array().expect("should be array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["bdf"], "0000:99:00.0");
    assert_eq!(arr[0]["name"], "Test GPU");
}

#[test]
fn test_dispatch_device_get_found() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: Some("Test".into()),
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.get",
        &serde_json::json!({"bdf": "0000:99:00.0"}),
        &mut devices,
        started,
    );
    let val = result.expect("device.get should succeed");
    assert_eq!(val["bdf"], "0000:99:00.0");
}

#[test]
fn test_dispatch_device_get_missing_bdf() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch("device.get", &serde_json::json!({}), &mut devices, started);
    let err = result.expect_err("device.get without bdf should fail");
    assert_eq!(i32::from(err.code), -32602);
    assert!(err.message.contains("bdf"));
}

#[test]
fn test_dispatch_device_get_not_managed() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.get",
        &serde_json::json!({"bdf": "0000:01:00.0"}),
        &mut devices,
        started,
    );
    let err = result.expect_err("device.get for unmanaged device should fail");
    assert_eq!(i32::from(err.code), -32000);
    assert!(err.message.contains("not managed"));
}

#[test]
fn test_dispatch_health_check() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "health.check",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let val = result.expect("health.check should succeed");
    assert_eq!(val["name"], "coral-glowplug");
    assert_eq!(val["device_count"], 0);
    assert!(val["healthy"].is_boolean());
    assert!(val["status"].is_string());
}

#[test]
fn test_dispatch_health_liveness() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "health.liveness",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let val = result.expect("health.liveness should succeed");
    assert_eq!(val["alive"], true);
}

#[test]
fn test_dispatch_health_readiness() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "health.readiness",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let val = result.expect("health.readiness should succeed");
    assert_eq!(val["name"], "coral-glowplug");
    assert!(val["ready"].is_boolean());
}

#[test]
fn test_dispatch_daemon_status() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "daemon.status",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let val = result.expect("daemon.status should succeed");
    assert!(val["uptime_secs"].as_u64().is_some());
    assert_eq!(val["device_count"], 0);
}

#[test]
fn test_dispatch_daemon_shutdown_returns_error() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "daemon.shutdown",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let err = result.expect_err("daemon.shutdown should return Err for shutdown signal");
    assert_eq!(i32::from(err.code), -32000);
    assert_eq!(err.message, "shutdown");
}

#[test]
fn test_dispatch_unknown_method() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "nonexistent.method",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let err = result.expect_err("unknown method should fail");
    assert_eq!(i32::from(err.code), -32601);
    assert!(err.message.contains("method not found"));
}

#[test]
fn test_dispatch_device_swap_missing_params() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch("device.swap", &serde_json::json!({}), &mut devices, started);
    let err = result.expect_err("device.swap without params should fail");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn test_dispatch_device_health() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.health",
        &serde_json::json!({"bdf": "0000:99:00.0"}),
        &mut devices,
        started,
    );
    let val = result.expect("device.health should succeed");
    assert_eq!(val["bdf"], "0000:99:00.0");
    assert!(val.get("vram_alive").is_some());
    assert!(val.get("domains_alive").is_some());
}

#[test]
fn test_make_response_success() {
    let resp = make_response(serde_json::json!(1), Ok(serde_json::json!({"ok": true})));
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["result"]["ok"], true);
    assert_eq!(parsed["id"], 1);
}

#[test]
fn test_make_response_error() {
    let resp = make_response(
        serde_json::json!("req-1"),
        Err(coral_glowplug::error::RpcError::invalid_params(
            "missing parameter",
        )),
    );
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert!(parsed["error"].is_object());
    assert_eq!(parsed["error"]["code"], -32602);
    assert_eq!(parsed["error"]["message"], "missing parameter");
}

#[test]
fn test_dispatch_device_lend_missing_bdf() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch("device.lend", &serde_json::json!({}), &mut devices, started);
    let err = result.expect_err("device.lend without bdf should fail");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn test_dispatch_device_lend_not_managed() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.lend",
        &serde_json::json!({"bdf": "0000:01:00.0"}),
        &mut devices,
        started,
    );
    let err = result.expect_err("device.lend for unmanaged device should fail");
    assert_eq!(i32::from(err.code), -32000);
}

#[test]
fn test_dispatch_device_lend_not_vfio() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.lend",
        &serde_json::json!({"bdf": "0000:99:00.0"}),
        &mut devices,
        started,
    );
    let err = result.expect_err("device.lend on unbound personality should fail");
    assert_eq!(i32::from(err.code), -32000);
}

#[test]
fn test_dispatch_device_reclaim_missing_bdf() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.reclaim",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let err = result.expect_err("device.reclaim without bdf should fail");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn test_dispatch_device_reclaim_not_managed() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.reclaim",
        &serde_json::json!({"bdf": "0000:01:00.0"}),
        &mut devices,
        started,
    );
    let err = result.expect_err("device.reclaim for unmanaged device should fail");
    assert_eq!(i32::from(err.code), -32000);
}

#[test]
fn test_dispatch_device_health_missing_bdf() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.health",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let err = result.expect_err("device.health without bdf should fail");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn test_dispatch_device_register_dump_missing_bdf() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.register_dump",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let err = result.expect_err("register_dump without bdf should fail");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn test_dispatch_device_register_dump_no_vfio() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.register_dump",
        &serde_json::json!({"bdf": "0000:99:00.0"}),
        &mut devices,
        started,
    );
    let err = result.expect_err("register_dump without VFIO should fail");
    assert_eq!(i32::from(err.code), -32000);
    assert!(err.message.contains("VFIO"));
}

#[test]
fn test_dispatch_device_register_snapshot_missing_bdf() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.register_snapshot",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let err = result.expect_err("register_snapshot without bdf should fail");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn test_dispatch_device_register_snapshot_empty() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.register_snapshot",
        &serde_json::json!({"bdf": "0000:99:00.0"}),
        &mut devices,
        started,
    );
    let val = result.expect("register_snapshot should succeed");
    assert_eq!(val["bdf"], "0000:99:00.0");
    assert_eq!(val["register_count"], 0);
    let regs = val["registers"].as_array().expect("registers array");
    assert!(regs.is_empty());
}

#[test]
fn test_dispatch_device_resurrect_missing_bdf() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.resurrect",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let err = result.expect_err("resurrect without bdf should fail");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn test_dispatch_device_resurrect_unknown_vendor() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.resurrect",
        &serde_json::json!({"bdf": "0000:99:00.0"}),
        &mut devices,
        started,
    );
    let err = result.expect_err("resurrect without HBM2 driver mapping should fail");
    assert_eq!(i32::from(err.code), -32000);
    assert!(
        err.message.contains("HBM2") || err.message.contains("vendor"),
        "unexpected message: {}",
        err.message
    );
}

#[test]
fn test_dispatch_device_swap_ember_unavailable() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:98:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.swap",
        &serde_json::json!({"bdf": "0000:98:00.0", "target": "nouveau"}),
        &mut devices,
        started,
    );
    match result {
        Ok(val) => {
            assert_eq!(val["bdf"], "0000:98:00.0");
        }
        Err(e) => {
            assert_eq!(i32::from(e.code), -32000);
        }
    }
}

#[test]
fn test_dispatch_device_get_invalid_bdf_rejected() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.get",
        &serde_json::json!({"bdf": "../etc/passwd"}),
        &mut devices,
        started,
    );
    let err = result.expect_err("invalid bdf");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn test_dispatch_device_swap_missing_target() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.swap",
        &serde_json::json!({"bdf": "0000:99:00.0"}),
        &mut devices,
        started,
    );
    let err = result.expect_err("device.swap without target should fail");
    assert_eq!(i32::from(err.code), -32602);
    assert!(err.message.contains("target"));
}

#[test]
fn test_dispatch_health_check_counts_devices() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "health.check",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let val = result.expect("health.check should succeed");
    assert_eq!(val["device_count"], 1);
    assert_eq!(val["healthy_count"], 0);
}

#[test]
fn test_make_response_preserves_null_id() {
    let resp = make_response(serde_json::Value::Null, Ok(serde_json::json!({"ok": true})));
    let parsed: serde_json::Value =
        serde_json::from_str(&resp).expect("make_response should emit valid JSON");
    assert!(parsed["id"].is_null());
}

#[test]
fn test_make_response_device_error_round_trip() {
    let resp = make_response(
        serde_json::json!(42),
        Err(coral_glowplug::error::RpcError::device_error("boom")),
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&resp).expect("make_response should emit valid JSON");
    assert_eq!(parsed["error"]["code"], -32000);
    assert_eq!(parsed["error"]["message"], "boom");
    assert_eq!(parsed["id"], 42);
}

#[test]
fn test_dispatch_read_bar0_range_count_exceeds_max() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.read_bar0_range",
        &serde_json::json!({"bdf": "0000:99:00.0", "offset": 0, "count": 5000}),
        &mut devices,
        started,
    );
    let err = result.expect_err("count over 4096");
    assert_eq!(i32::from(err.code), -32602);
    assert!(err.message.contains("4096"));
}

#[test]
fn test_dispatch_read_bar0_range_missing_offset() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.read_bar0_range",
        &serde_json::json!({"bdf": "0000:99:00.0", "count": 4}),
        &mut devices,
        started,
    );
    let err = result.expect_err("missing offset");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn test_dispatch_pramin_read_count_exceeds_max() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.pramin_read",
        &serde_json::json!({"bdf": "0000:99:00.0", "vram_offset": 0, "count": 5000}),
        &mut devices,
        started,
    );
    let err = result.expect_err("count over 4096");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn test_dispatch_pramin_write_values_too_long() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let values: Vec<u64> = (0..4100).collect();
    let result = dispatch(
        "device.pramin_write",
        &serde_json::json!({"bdf": "0000:99:00.0", "vram_offset": 0, "values": values}),
        &mut devices,
        started,
    );
    let err = result.expect_err("values array too long");
    assert_eq!(i32::from(err.code), -32602);
    assert!(err.message.contains("4096"));
}

#[test]
fn test_dispatch_pramin_write_missing_values_array() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.pramin_write",
        &serde_json::json!({"bdf": "0000:99:00.0", "vram_offset": 0}),
        &mut devices,
        started,
    );
    let err = result.expect_err("missing values");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn test_dispatch_write_register_pmc_blocked_without_allow_dangerous() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.write_register",
        &serde_json::json!({
            "bdf": "0000:99:00.0",
            "offset": 0x200,
            "value": 1,
            "allow_dangerous": false
        }),
        &mut devices,
        started,
    );
    let err = result.expect_err("PMC write blocked");
    assert_eq!(i32::from(err.code), -32000);
    assert!(
        err.message.contains("PMC") || err.message.contains("0x200"),
        "{}",
        err.message
    );
}

#[test]
fn test_dispatch_async_routed_methods_return_internal() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    for method in ["device.compute_info", "device.quota", "device.set_quota"] {
        let result = dispatch(
            method,
            &serde_json::json!({"bdf": "0000:01:00.0"}),
            &mut devices,
            started,
        );
        let err = result.expect_err("sync handler should not implement async route");
        assert_eq!(i32::from(err.code), -32603);
        assert!(err.message.contains("async"), "{method}: {}", err.message);
    }
}

#[test]
fn test_dispatch_device_reset_missing_bdf() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.reset",
        &serde_json::json!({}),
        &mut devices,
        started,
    );
    let err = result.expect_err("missing bdf");
    assert_eq!(i32::from(err.code), -32602);
}

#[test]
fn test_dispatch_read_bar0_range_success_empty_without_vfio() {
    let config = coral_glowplug::config::DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut devices = vec![coral_glowplug::device::DeviceSlot::new(config)];
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.read_bar0_range",
        &serde_json::json!({"bdf": "0000:99:00.0", "offset": 0, "count": 4}),
        &mut devices,
        started,
    );
    let val = result.expect("read_bar0_range");
    assert_eq!(val["bdf"], "0000:99:00.0");
    assert_eq!(val["count"], 0);
    let arr = val["values"].as_array().expect("values array");
    assert!(arr.is_empty());
}
