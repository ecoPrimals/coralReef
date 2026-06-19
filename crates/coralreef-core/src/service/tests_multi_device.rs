// SPDX-License-Identifier: AGPL-3.0-or-later
//! Multi-device compilation tests — cross-vendor, mixed success/failure, edge cases.

use super::*;
use std::sync::Arc;
use types::{DeviceTarget, MultiDeviceCompileRequest};

#[test]
fn test_multi_device_compile_basic() {
    let req = MultiDeviceCompileRequest {
        wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
        targets: vec![
            DeviceTarget {
                card_index: 0,
                arch: "sm_70".to_owned(),
                pcie_group: None,
            },
            DeviceTarget {
                card_index: 1,
                arch: "sm_89".to_owned(),
                pcie_group: Some(0),
            },
        ],
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
    };
    let resp = handle_compile_wgsl_multi(req).expect("multi-device should succeed");
    assert_eq!(resp.total_count, 2);
    assert_eq!(resp.success_count, 2);
    assert_eq!(resp.results.len(), 2);
    assert_eq!(resp.results[0].card_index, 0);
    assert_eq!(resp.results[0].arch, "sm_70");
    assert!(resp.results[0].binary.is_some());
    assert!(resp.results[0].size > 0);
    assert!(resp.results[0].error.is_none());
    assert_eq!(resp.results[1].card_index, 1);
    assert_eq!(resp.results[1].arch, "sm_89");
    assert!(resp.results[1].binary.is_some());
}

#[test]
fn test_multi_device_compile_mixed_success_failure() {
    let req = MultiDeviceCompileRequest {
        wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
        targets: vec![
            DeviceTarget {
                card_index: 0,
                arch: "sm_70".to_owned(),
                pcie_group: None,
            },
            DeviceTarget {
                card_index: 1,
                arch: "sm_99".to_owned(),
                pcie_group: None,
            },
        ],
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
    };
    let resp = handle_compile_wgsl_multi(req).expect("partial failure is not a top-level error");
    assert_eq!(resp.total_count, 2);
    assert_eq!(resp.success_count, 1);
    assert!(resp.results[0].binary.is_some());
    assert!(resp.results[1].binary.is_none());
    assert!(resp.results[1].error.is_some());
}

#[test]
fn test_multi_device_compile_empty_source() {
    let req = MultiDeviceCompileRequest {
        wgsl_source: Arc::from(""),
        targets: vec![DeviceTarget {
            card_index: 0,
            arch: "sm_70".to_owned(),
            pcie_group: None,
        }],
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
    };
    assert!(handle_compile_wgsl_multi(req).is_err());
}

#[test]
fn test_multi_device_compile_no_targets() {
    let req = MultiDeviceCompileRequest {
        wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
        targets: vec![],
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
    };
    assert!(handle_compile_wgsl_multi(req).is_err());
}

#[test]
fn test_multi_device_compile_cross_vendor() {
    let req = MultiDeviceCompileRequest {
        wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
        targets: vec![
            DeviceTarget {
                card_index: 0,
                arch: "sm_80".to_owned(),
                pcie_group: Some(0),
            },
            DeviceTarget {
                card_index: 1,
                arch: "rdna2".to_owned(),
                pcie_group: Some(1),
            },
        ],
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: Some("fused".to_owned()),
    };
    let resp = handle_compile_wgsl_multi(req).expect("cross-vendor should succeed");
    assert_eq!(resp.success_count, 2);
    assert_eq!(resp.results[0].arch, "sm_80");
    assert_eq!(resp.results[1].arch, "rdna2");
}

#[test]
fn test_multi_device_request_serde_roundtrip() {
    let req = MultiDeviceCompileRequest {
        wgsl_source: Arc::from("fn main() {}"),
        targets: vec![DeviceTarget {
            card_index: 0,
            arch: "sm_70".to_owned(),
            pcie_group: Some(1),
        }],
        opt_level: 3,
        fp64_software: true,
        fp64_strategy: Some("software".to_owned()),
        fma_policy: Some("separate".to_owned()),
    };
    let json = serde_json::to_string(&req).unwrap();
    let roundtrip: MultiDeviceCompileRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.wgsl_source.as_ref(), req.wgsl_source.as_ref());
    assert_eq!(roundtrip.targets.len(), 1);
    assert_eq!(roundtrip.targets[0].arch, "sm_70");
    assert_eq!(roundtrip.targets[0].pcie_group, Some(1));
    assert_eq!(roundtrip.fma_policy.as_deref(), Some("separate"));
}
