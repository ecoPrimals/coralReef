// SPDX-License-Identifier: AGPL-3.0-only

//! Unit tests for transport-agnostic JSON helpers in [`super::handlers_device`].

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::error::EmberIpcError;
use crate::hold::HeldDevice;

use super::handlers_device::{
    list_value, parse_swap_params, status_value, vfio_fds_payload, vfio_fds_result_value,
};

#[test]
fn vfio_fds_result_value_legacy_shape() {
    let v = vfio_fds_result_value(
        "0000:01:00.0",
        3,
        coral_driver::vfio::VfioBackendKind::Legacy,
    );
    assert_eq!(v["bdf"], json!("0000:01:00.0"));
    assert_eq!(v["num_fds"], json!(3));
    assert_eq!(v["backend"], json!("legacy"));
    assert!(v.get("ioas_id").is_none());
}

#[test]
fn vfio_fds_result_value_iommufd_shape() {
    let v = vfio_fds_result_value(
        "0000:02:00.0",
        2,
        coral_driver::vfio::VfioBackendKind::Iommufd { ioas_id: 9 },
    );
    assert_eq!(v["bdf"], json!("0000:02:00.0"));
    assert_eq!(v["num_fds"], json!(2));
    assert_eq!(v["backend"], json!("iommufd"));
    assert_eq!(v["ioas_id"], json!(9));
}

#[test]
fn vfio_fds_payload_matches_result_value_for_open_device() {
    let bdf = "0000:01:00.0";
    match coral_driver::vfio::VfioDevice::open(bdf) {
        Ok(dev) => {
            let held = HeldDevice::new_unmonitored(bdf.into(), dev);
            let n = held.device.sendable_fds().len();
            let kind = held.device.backend_kind();
            let got = vfio_fds_payload(&held, bdf);
            let expected = vfio_fds_result_value(bdf, n, kind);
            assert_eq!(got, expected);
        }
        Err(_) => {
            // No VFIO GPU in this test environment — pure JSON shape is covered above.
        }
    }
}

#[test]
fn parse_swap_params_accepts_valid_and_default_trace() {
    let p = parse_swap_params(&json!({
        "bdf": "0000:01:00.0",
        "target": "vfio-pci",
    }))
    .unwrap();
    assert_eq!(p.bdf, "0000:01:00.0");
    assert_eq!(p.target, "vfio-pci");
    assert!(!p.trace);

    let p2 = parse_swap_params(&json!({
        "bdf": "0000:01:00.0",
        "target": "nouveau",
        "trace": true,
    }))
    .unwrap();
    assert!(p2.trace);
}

#[test]
fn parse_swap_params_missing_or_wrong_type() {
    assert!(matches!(
        parse_swap_params(&json!({"target": "vfio-pci"})),
        Err(EmberIpcError::InvalidRequest(_))
    ));
    assert!(matches!(
        parse_swap_params(&json!({"bdf": "0000:01:00.0"})),
        Err(EmberIpcError::InvalidRequest(_))
    ));
    assert!(matches!(
        parse_swap_params(&json!({"bdf": 1, "target": "vfio-pci"})),
        Err(EmberIpcError::InvalidRequest(_))
    ));
    assert!(matches!(
        parse_swap_params(&json!({"bdf": "0000:01:00.0", "target": 2})),
        Err(EmberIpcError::InvalidRequest(_))
    ));
}

#[test]
fn list_value_empty_devices_array() {
    let held: Arc<RwLock<HashMap<String, HeldDevice>>> = Arc::new(RwLock::new(HashMap::new()));
    let v = list_value(&held).unwrap();
    assert_eq!(v, json!({ "devices": [] }));
}

#[test]
fn status_value_includes_devices_and_uptime() {
    let held: Arc<RwLock<HashMap<String, HeldDevice>>> = Arc::new(RwLock::new(HashMap::new()));
    let started = Instant::now() - Duration::from_secs(3);
    let v = status_value(&held, started).unwrap();
    assert_eq!(v["devices"], json!([]));
    let up = v["uptime_secs"].as_u64().unwrap();
    assert!((3..=10).contains(&up));
}
