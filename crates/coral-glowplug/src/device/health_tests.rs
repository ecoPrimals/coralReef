// SPDX-License-Identifier: AGPL-3.0-only

//! Focused tests for [`super::health`](super::health) helpers.

use std::time::Duration;

use crate::MockSysfs;
use crate::config::DeviceConfig;
use crate::ember::EmberClient;
use crate::error::DeviceError;
use crate::pci_ids::{AMD_VENDOR_ID, BRAINCHIP_VENDOR_ID, NVIDIA_VENDOR_ID};

use super::DeviceSlot;
use super::PowerState;

fn base_config(bdf: &str, boot: &str) -> DeviceConfig {
    DeviceConfig {
        bdf: bdf.into(),
        name: None,
        boot_personality: boot.into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    }
}

#[test]
fn read_register_without_vfio_returns_none() {
    let bdf = "0000:40:00.0";
    let mock = MockSysfs::default();
    let slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    assert!(slot.read_register(0x10_0000).is_none());
}

#[test]
fn dump_registers_empty_offsets_without_vfio_is_empty() {
    let bdf = "0000:41:00.0";
    let mock = MockSysfs::default();
    let slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    let m = slot.dump_registers(&[]);
    assert!(m.is_empty());
}

#[test]
fn last_snapshot_starts_empty() {
    let bdf = "0000:42:00.0";
    let mock = MockSysfs::default();
    let slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    assert!(slot.last_snapshot().is_empty());
}

#[test]
fn snapshot_registers_without_vfio_is_noop() {
    let bdf = "0000:43:00.0";
    let mock = MockSysfs::default();
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    slot.snapshot_registers();
    assert!(slot.last_snapshot().is_empty());
}

#[test]
fn snapshot_registers_skips_oracle_dump_without_vfio_even_if_configured() {
    let bdf = "0000:44:00.0";
    let dir = tempfile::tempdir().expect("tempdir");
    let dump_path = dir.path().join("oracle.txt");
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    let mut slot = DeviceSlot::with_sysfs(
        DeviceConfig {
            oracle_dump: Some(dump_path.to_string_lossy().into_owned()),
            shared: None,
            ..base_config(bdf, "vfio")
        },
        mock,
    );
    slot.snapshot_registers();
    assert!(
        !dump_path.exists(),
        "oracle dump is only written after BAR0 snapshot with VFIO"
    );
}

#[test]
fn check_health_without_vfio_clears_domain_counts() {
    let bdf = "0000:45:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.power_state.insert(bdf.to_string(), PowerState::D0);
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    slot.check_health();
    assert!(!slot.health.vram_alive);
    assert_eq!(slot.health.domains_alive, 0);
    assert_eq!(slot.health.domains_faulted, 0);
    assert_eq!(slot.health.power, PowerState::D0);
}

#[test]
fn check_health_refresh_power_and_link_width_from_mock() {
    let bdf = "0000:46:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.power_state.insert(bdf.to_string(), PowerState::D3Hot);
    mock.link_width.insert(bdf.to_string(), Some(8));
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    slot.check_health();
    assert_eq!(slot.health.power, PowerState::D3Hot);
    assert_eq!(slot.health.pci_link_width, Some(8));
}

#[test]
fn check_quiescence_override_true_short_circuits() {
    let bdf = "0000:47:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    slot.test_set_quiescence_override(Some(true));
    assert!(slot.wait_quiescence(Duration::from_millis(1)));
}

#[test]
fn check_quiescence_override_false_times_out() {
    let bdf = "0000:48:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    slot.test_set_quiescence_override(Some(false));
    assert!(!slot.wait_quiescence(Duration::from_millis(0)));
}

#[test]
fn resurrect_hbm2_refuses_when_nvidia_bound() {
    let bdf = "0000:49:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.current_driver
        .insert(bdf.to_string(), Some("nvidia".into()));
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    let err = slot.resurrect_hbm2().unwrap_err();
    assert!(matches!(err, DeviceError::DriverBind { .. }));
}

#[test]
fn resurrect_hbm2_unknown_vendor_returns_driver_bind() {
    let bdf = "0000:4a:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.pci_ids
        .insert(bdf.to_string(), (BRAINCHIP_VENDOR_ID, 0xbca1));
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    let err = slot.resurrect_hbm2().unwrap_err();
    match err {
        DeviceError::DriverBind { driver, .. } => assert_eq!(driver, "unknown"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn resurrect_hbm2_amd_vendor_requires_ember() {
    let _guard = EmberClient::disable_for_test();
    let bdf = "0000:4b:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.pci_ids
        .insert(bdf.to_string(), (AMD_VENDOR_ID, 0x66a0));
    mock.current_driver
        .insert(bdf.to_string(), Some("vfio-pci".into()));
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    let err = slot.resurrect_hbm2().unwrap_err();
    match err {
        DeviceError::DriverBind { reason, .. } => {
            assert!(
                reason.contains("ember") || reason.contains("amdgpu"),
                "{reason}"
            );
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn resurrect_hbm2_nvidia_vendor_non_nvidia_driver_still_needs_ember() {
    let _guard = EmberClient::disable_for_test();
    let bdf = "0000:4c:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.pci_ids
        .insert(bdf.to_string(), (NVIDIA_VENDOR_ID, 0x1d81));
    mock.current_driver
        .insert(bdf.to_string(), Some("vfio-pci".into()));
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    let err = slot.resurrect_hbm2().unwrap_err();
    match err {
        DeviceError::DriverBind { reason, .. } => assert!(reason.contains("ember"), "{reason}"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn write_register_pmc_enable_blocked_without_allow_dangerous() {
    let bdf = "0000:55:00.0";
    let mock = MockSysfs::default();
    let slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    let err = slot
        .write_register(0x200, 1, false)
        .expect_err("PMC_ENABLE write without allow_dangerous");
    match err {
        DeviceError::VfioOpen { reason, .. } => {
            assert!(
                reason.contains("PMC_ENABLE") || reason.contains("0x200"),
                "{reason}"
            );
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn write_register_non_pmc_requires_vfio_holder() {
    let bdf = "0000:56:00.0";
    let mock = MockSysfs::default();
    let slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    let err = slot
        .write_register(0x100, 1, false)
        .expect_err("no VFIO holder");
    match err {
        DeviceError::VfioOpen { reason, .. } => {
            assert!(
                reason.contains("VFIO") || reason.contains("no VFIO"),
                "{reason}"
            );
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn read_bar0_range_without_vfio_returns_empty() {
    let bdf = "0000:57:00.0";
    let mock = MockSysfs::default();
    let slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    assert!(slot.read_bar0_range(0, 100).is_empty());
}

#[test]
fn read_bar0_range_caps_count_at_4096() {
    let bdf = "0000:58:00.0";
    let mock = MockSysfs::default();
    let slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    assert!(slot.read_bar0_range(0, 10_000).is_empty());
}

#[test]
fn pramin_read_without_vfio_returns_error() {
    let bdf = "0000:59:00.0";
    let mock = MockSysfs::default();
    let slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    let err = slot.pramin_read(0, 4).expect_err("PRAMIN requires VFIO");
    match err {
        DeviceError::VfioOpen { reason, .. } => {
            assert!(
                reason.contains("PRAMIN") || reason.contains("VFIO"),
                "{reason}"
            );
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn pramin_write_without_vfio_returns_error() {
    let bdf = "0000:5a:00.0";
    let mock = MockSysfs::default();
    let slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    let err = slot
        .pramin_write(0, &[1, 2])
        .expect_err("PRAMIN requires VFIO");
    match err {
        DeviceError::VfioOpen { reason, .. } => {
            assert!(
                reason.contains("PRAMIN") || reason.contains("VFIO"),
                "{reason}"
            );
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn oracle_capture_via_vfio_without_holder_returns_err() {
    let bdf = "0000:5b:00.0";
    let mock = MockSysfs::default();
    let slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    let err = slot
        .oracle_capture_via_vfio(4)
        .expect_err("oracle capture requires VFIO");
    let msg = err.to_string();
    assert!(
        msg.contains("VFIO") || msg.contains("vfio"),
        "unexpected message: {msg}"
    );
}
