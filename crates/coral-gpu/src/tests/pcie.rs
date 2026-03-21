// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! [`crate::PcieDeviceInfo`] formatting.

use coral_reef::{AmdArch, GpuTarget};

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
