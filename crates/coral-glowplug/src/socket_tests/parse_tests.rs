// SPDX-License-Identifier: AGPL-3.0-only

use super::super::JsonRpcRequest;
use super::super::protocol::{DeviceInfo, HealthInfo};

#[test]
fn test_jsonrpc_request_parse_valid() {
    let line = r#"{"jsonrpc":"2.0","method":"health.check","params":{},"id":1}"#;
    let result: Result<JsonRpcRequest, _> = serde_json::from_str(line);
    let req = match result {
        Ok(r) => r,
        Err(e) => panic!("expected valid JSON-RPC request: {e}"),
    };
    assert_eq!(req.jsonrpc, "2.0");
    assert_eq!(req.method, "health.check");
    assert!(req.params.is_object());
    assert_eq!(req.id, serde_json::json!(1));
}

#[test]
fn test_jsonrpc_request_parse_with_params() {
    let line =
        r#"{"jsonrpc":"2.0","method":"device.get","params":{"bdf":"0000:01:00.0"},"id":"req-1"}"#;
    let result: Result<JsonRpcRequest, _> = serde_json::from_str(line);
    let req = match result {
        Ok(r) => r,
        Err(e) => panic!("expected valid JSON-RPC request: {e}"),
    };
    assert_eq!(req.method, "device.get");
    let bdf = req.params.get("bdf").and_then(serde_json::Value::as_str);
    assert_eq!(bdf, Some("0000:01:00.0"));
}

#[test]
fn test_jsonrpc_request_parse_default_params() {
    let line = r#"{"jsonrpc":"2.0","method":"daemon.status","id":null}"#;
    let result: Result<JsonRpcRequest, _> = serde_json::from_str(line);
    let req = match result {
        Ok(r) => r,
        Err(e) => panic!("expected valid JSON-RPC request: {e}"),
    };
    assert_eq!(req.method, "daemon.status");
    assert!(req.params.is_null() || req.params.is_object());
}

#[test]
fn test_jsonrpc_request_parse_invalid() {
    let line = r#"{"jsonrpc":"2.0","method":123}"#;
    let result: Result<JsonRpcRequest, _> = serde_json::from_str(line);
    assert!(result.is_err());
}

#[test]
fn test_jsonrpc_request_parse_missing_method_field() {
    let line = r#"{"jsonrpc":"2.0","params":{},"id":1}"#;
    let result: Result<JsonRpcRequest, _> = serde_json::from_str(line);
    assert!(result.is_err());
}

#[test]
fn test_jsonrpc_request_parse_missing_jsonrpc_field() {
    let line = r#"{"method":"health.check","params":{},"id":1}"#;
    let result: Result<JsonRpcRequest, _> = serde_json::from_str(line);
    assert!(result.is_err());
}

#[test]
fn test_jsonrpc_request_parse_invalid_json() {
    let line = r#"{"jsonrpc":"2.0","method":"#;
    let result: Result<JsonRpcRequest, _> = serde_json::from_str(line);
    assert!(result.is_err());
}

#[test]
fn test_device_info_serialization_roundtrip() {
    let info = DeviceInfo {
        bdf: "0000:01:00.0".into(),
        name: Some("Compute GPU".into()),
        chip: "GV100 (Titan V)".into(),
        vendor_id: 0x10de,
        device_id: 0x1d81,
        personality: "vfio (group 5)".into(),
        role: Some("compute".into()),
        power: "D0".into(),
        vram_alive: true,
        domains_alive: 8,
        domains_faulted: 0,
        has_vfio_fd: true,
        pci_link_width: Some(16),
        protected: false,
    };
    let json = match serde_json::to_string(&info) {
        Ok(j) => j,
        Err(e) => panic!("serialize DeviceInfo: {e}"),
    };
    let parsed: DeviceInfo = match serde_json::from_str(&json) {
        Ok(p) => p,
        Err(e) => panic!("deserialize DeviceInfo: {e}"),
    };
    assert_eq!(parsed.bdf, info.bdf);
    assert_eq!(parsed.name, info.name);
    assert_eq!(parsed.chip, info.chip);
    assert_eq!(parsed.vendor_id, info.vendor_id);
    assert_eq!(parsed.device_id, info.device_id);
    assert_eq!(parsed.personality, info.personality);
    assert_eq!(parsed.power, info.power);
    assert_eq!(parsed.vram_alive, info.vram_alive);
    assert_eq!(parsed.domains_alive, info.domains_alive);
    assert_eq!(parsed.domains_faulted, info.domains_faulted);
    assert_eq!(parsed.has_vfio_fd, info.has_vfio_fd);
    assert_eq!(parsed.pci_link_width, info.pci_link_width);
}

#[test]
fn test_health_info_serialization_roundtrip() {
    let info = HealthInfo {
        bdf: "0000:02:00.0".into(),
        boot0: 0x1234_5678,
        pmc_enable: 0x9abc_def0,
        vram_alive: true,
        power: "D3hot".into(),
        domains_alive: 7,
        domains_faulted: 1,
        fecs_cpuctl: 0,
        fecs_stopped: false,
        fecs_halted: false,
        fecs_sctl: 0,
        gpccs_cpuctl: 0,
    };
    let json = match serde_json::to_string(&info) {
        Ok(j) => j,
        Err(e) => panic!("serialize HealthInfo: {e}"),
    };
    let parsed: HealthInfo = match serde_json::from_str(&json) {
        Ok(p) => p,
        Err(e) => panic!("deserialize HealthInfo: {e}"),
    };
    assert_eq!(parsed.bdf, info.bdf);
    assert_eq!(parsed.boot0, info.boot0);
    assert_eq!(parsed.pmc_enable, info.pmc_enable);
    assert_eq!(parsed.vram_alive, info.vram_alive);
    assert_eq!(parsed.power, info.power);
    assert_eq!(parsed.domains_alive, info.domains_alive);
    assert_eq!(parsed.domains_faulted, info.domains_faulted);
}

#[test]
fn health_info_firmware_fields_roundtrip() {
    let info = HealthInfo {
        bdf: "0000:03:00.0".into(),
        boot0: 0x1400_00A1,
        pmc_enable: 0x0000_1100,
        vram_alive: true,
        power: "D0".into(),
        domains_alive: 5,
        domains_faulted: 0,
        fecs_cpuctl: 0x30,
        fecs_stopped: false,
        fecs_halted: false,
        fecs_sctl: 0x2000,
        gpccs_cpuctl: 0x30,
    };
    let json = serde_json::to_string(&info).expect("serialize");
    let parsed: HealthInfo = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.fecs_cpuctl, 0x30);
    assert!(!parsed.fecs_stopped);
    assert!(!parsed.fecs_halted);
    assert_eq!(parsed.fecs_sctl, 0x2000);
    assert_eq!(parsed.gpccs_cpuctl, 0x30);
}

#[test]
fn health_info_deserializes_fecs_hreset_alias_to_fecs_halted() {
    let json = r#"{
        "bdf": "0000:03:00.0",
        "boot0": 0,
        "pmc_enable": 0,
        "vram_alive": true,
        "power": "D0",
        "domains_alive": 0,
        "domains_faulted": 0,
        "fecs_hreset": true
    }"#;
    let parsed: HealthInfo = serde_json::from_str(json).expect("deserialize");
    assert!(parsed.fecs_halted);
    assert!(!parsed.fecs_stopped);
}

#[test]
fn health_info_backward_compat_without_firmware_fields() {
    let json = r#"{
        "bdf": "0000:03:00.0",
        "boot0": 335544481,
        "pmc_enable": 4352,
        "vram_alive": true,
        "power": "D0",
        "domains_alive": 5,
        "domains_faulted": 0
    }"#;
    let parsed: HealthInfo = serde_json::from_str(json).expect("deserialize legacy HealthInfo");
    assert_eq!(parsed.fecs_cpuctl, 0);
    assert!(!parsed.fecs_stopped);
    assert!(!parsed.fecs_halted);
    assert_eq!(parsed.fecs_sctl, 0);
    assert_eq!(parsed.gpccs_cpuctl, 0);
    assert_eq!(parsed.bdf, "0000:03:00.0");
}

#[test]
fn test_health_check_response_format() {
    let response = serde_json::json!({
        "alive": true,
        "name": "coral-glowplug",
        "device_count": 0,
        "healthy_count": 0
    });
    assert_eq!(response["alive"], true);
    assert_eq!(response["name"], "coral-glowplug");
    assert_eq!(response["device_count"], 0);
    assert_eq!(response["healthy_count"], 0);
}
