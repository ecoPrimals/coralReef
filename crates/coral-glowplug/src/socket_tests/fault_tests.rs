// SPDX-License-Identifier: AGPL-3.0-or-later

use super::super::*;
use super::{send_line, spawn_test_server};

#[test]
fn test_validate_bdf_valid() {
    assert!(validate_bdf("0000:01:00.0").is_ok());
    assert!(validate_bdf("0000:4a:00.0").is_ok());
}

#[test]
fn test_validate_bdf_rejects_path_traversal() {
    assert!(validate_bdf("../../../etc/shadow").is_err());
    assert!(validate_bdf("0000:01:00.0/../../root").is_err());
}

#[test]
fn test_validate_bdf_rejects_null_bytes() {
    assert!(validate_bdf("0000:01:00.0\0rm -rf /").is_err());
}

#[test]
fn test_validate_bdf_rejects_empty() {
    assert!(validate_bdf("").is_err());
}

#[test]
fn test_validate_bdf_rejects_overlong() {
    let long = "0".repeat(100);
    assert!(validate_bdf(&long).is_err());
}

#[test]
fn test_validate_bdf_rejects_shell_injection() {
    assert!(validate_bdf("0000:01:00.0; rm -rf /").is_err());
    assert!(validate_bdf("$(cat /etc/passwd)").is_err());
    assert!(validate_bdf("`whoami`").is_err());
}

#[test]
fn test_validate_bdf_rejects_unicode() {
    assert!(validate_bdf("0000:01:00.0\u{200B}").is_err());
    assert!(validate_bdf("０000:01:00.0").is_err());
}

#[tokio::test]
async fn test_dispatch_bdf_traversal_rejected() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.get",
        &serde_json::json!({"bdf": "../../../etc/shadow"}),
        &mut devices,
        started,
    );
    let err = result.expect_err("traversal should be rejected");
    assert_eq!(i32::from(err.code), -32602);
    assert!(err.message.contains("invalid BDF"));
}

#[tokio::test]
async fn test_dispatch_bdf_null_byte_rejected() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.health",
        &serde_json::json!({"bdf": "0000:01:00.0\u{0000}"}),
        &mut devices,
        started,
    );
    let err = result.expect_err("null byte should be rejected");
    assert_eq!(i32::from(err.code), -32602);
}

#[tokio::test]
async fn test_fuzz_empty_object() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let resp = send_line(&addr, "{}").await;
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
    assert!(v["error"].is_object(), "empty object should be an error");
    handle.abort();
}

#[tokio::test]
async fn test_fuzz_null_payload() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let resp = send_line(&addr, "null").await;
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
    assert_eq!(v["error"]["code"], -32700);
    handle.abort();
}

#[tokio::test]
async fn test_fuzz_array_payload() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let resp = send_line(&addr, "[1,2,3]").await;
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
    assert_eq!(v["error"]["code"], -32700);
    handle.abort();
}

#[tokio::test]
async fn test_fuzz_deeply_nested_json() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let nested = "{".repeat(100) + &"}".repeat(100);
    let resp = send_line(&addr, &nested).await;
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
    assert!(v["error"].is_object());
    handle.abort();
}

#[tokio::test]
async fn test_fuzz_huge_string_value() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let big_method = "x".repeat(10_000);
    let payload = format!(r#"{{"jsonrpc":"2.0","method":"{big_method}","params":{{}},"id":1}}"#);
    let resp = send_line(&addr, &payload).await;
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
    assert!(v["error"].is_object());
    assert_eq!(v["error"]["code"], -32601);
    handle.abort();
}

#[tokio::test]
async fn test_fuzz_numeric_method() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let resp = send_line(
        &addr,
        r#"{"jsonrpc":"2.0","method":12345,"params":{},"id":1}"#,
    )
    .await;
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
    assert_eq!(v["error"]["code"], -32700);
    handle.abort();
}

#[tokio::test]
async fn test_fuzz_negative_id() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let resp = send_line(
        &addr,
        r#"{"jsonrpc":"2.0","method":"health.check","params":{},"id":-999}"#,
    )
    .await;
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
    assert_eq!(v["id"], -999);
    assert!(v["result"]["alive"].as_bool().unwrap_or(false));
    handle.abort();
}

#[tokio::test]
async fn test_fault_invalid_method_does_not_crash() {
    let (addr, handle, _tx) = spawn_test_server().await;
    let methods = [
        "device.list.extra",
        "",
        ".",
        "device.",
        ".list",
        "DEVICE.LIST",
        "device.swap",
        "💀",
    ];
    for method in &methods {
        let resp = send_line(
            &addr,
            &format!(r#"{{"jsonrpc":"2.0","method":"{method}","params":{{}},"id":1}}"#),
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&resp).expect("valid json");
        assert!(
            v["result"].is_object() || v["error"].is_object(),
            "method {method:?} should return valid JSON-RPC"
        );
    }
    handle.abort();
}

#[tokio::test]
async fn test_fault_swap_with_traversal_target() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.swap",
        &serde_json::json!({
            "bdf": "0000:01:00.0",
            "target": "../../../bin/sh"
        }),
        &mut devices,
        started,
    );
    assert!(result.is_err());
}

#[tokio::test]
async fn test_fault_device_get_with_empty_bdf() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.get",
        &serde_json::json!({"bdf": ""}),
        &mut devices,
        started,
    );
    let err = result.expect_err("empty bdf should fail");
    assert_eq!(i32::from(err.code), -32602);
}

#[tokio::test]
async fn test_fault_params_wrong_type() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "device.get",
        &serde_json::json!({"bdf": 12345}),
        &mut devices,
        started,
    );
    let err = result.expect_err("numeric bdf should fail");
    assert_eq!(i32::from(err.code), -32602);
}

#[tokio::test]
async fn test_fault_extra_params_ignored() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let result = dispatch(
        "health.check",
        &serde_json::json!({"extra_field": "should_be_ignored", "another": 42}),
        &mut devices,
        started,
    );
    assert!(result.is_ok());
}

#[test]
fn test_pen_repeated_shutdown_does_not_panic() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    for _ in 0..10 {
        let result = dispatch(
            "daemon.shutdown",
            &serde_json::json!({}),
            &mut devices,
            started,
        );
        assert!(result.is_err());
    }
}

#[test]
fn test_pen_method_not_found_does_not_leak_internals() {
    let mut devices: Vec<coral_glowplug::device::DeviceSlot> = Vec::new();
    let started = std::time::Instant::now();
    let probes = [
        "__proto__",
        "constructor",
        "toString",
        "system.listMethods",
        "rpc.discover",
        "admin.shutdown",
    ];
    for method in &probes {
        let result = dispatch(method, &serde_json::json!({}), &mut devices, started);
        let err = result.expect_err("should be method not found");
        assert_eq!(i32::from(err.code), -32601);
        assert!(
            !err.message.contains("panic"),
            "error should not reveal internal state"
        );
    }
}
