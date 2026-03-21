// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! [`crate::PcieDeviceInfo`] formatting.

use coral_reef::{AmdArch, GpuTarget, NvArch};

#[test]
fn pcie_device_info_debug_format() {
    let info = crate::PcieDeviceInfo {
        render_node: "/dev/dri/renderD128".into(),
        pcie_address: Some("0000:01:00.0".into()),
        switch_group: Some(0),
        target: GpuTarget::Amd(AmdArch::Rdna2),
    };
    let s = format!("{info:?}");
    assert!(s.contains("PcieDeviceInfo"));
    assert!(s.contains("renderD128"));
}

#[test]
fn pcie_device_info_field_access() {
    let info = crate::PcieDeviceInfo {
        render_node: "/dev/dri/renderD129".into(),
        pcie_address: Some("0000:65:00.0".into()),
        switch_group: None,
        target: GpuTarget::Nvidia(NvArch::Sm80),
    };
    assert_eq!(info.render_node, "/dev/dri/renderD129");
    assert_eq!(info.pcie_address.as_deref(), Some("0000:65:00.0"));
    assert!(info.switch_group.is_none());
    assert!(matches!(info.target, GpuTarget::Nvidia(NvArch::Sm80)));
}

#[cfg(target_os = "linux")]
#[test]
fn assign_switch_groups_empty_and_single_device() {
    use crate::pcie::assign_switch_groups;

    let mut empty: Vec<crate::PcieDeviceInfo> = Vec::new();
    assign_switch_groups(&mut empty);

    let mut one = vec![crate::PcieDeviceInfo {
        render_node: "/dev/dri/renderD130".into(),
        pcie_address: Some("0000:03:00.0".into()),
        switch_group: None,
        target: GpuTarget::Amd(AmdArch::Rdna2),
    }];
    assign_switch_groups(&mut one);
    assert_eq!(one[0].switch_group, Some(0));
}
