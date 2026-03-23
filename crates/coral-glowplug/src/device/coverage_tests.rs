// SPDX-License-Identifier: AGPL-3.0-only

//! Additional [`MockSysfs`] coverage for activation, health, and swap paths.

use crate::MockSysfs;
use crate::config::DeviceConfig;
use crate::error::DeviceError;
use crate::personality::Personality;
use crate::sysfs_ops::SysfsOps;
use std::os::fd::OwnedFd;
use std::sync::atomic::Ordering;

use super::DeviceSlot;
use super::PowerState;

use crate::ember::EmberClient;

fn base_config(bdf: &str, boot: &str) -> DeviceConfig {
    DeviceConfig {
        bdf: bdf.into(),
        name: None,
        boot_personality: boot.into(),
        power_policy: "always_on".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    }
}

#[test]
fn activate_rejects_unknown_boot_personality_early() {
    let bdf = "0000:10:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    let mut slot = DeviceSlot::with_sysfs(
        DeviceConfig {
            boot_personality: "definitely-unknown-driver".into(),
            shared: None,
            ..base_config(bdf, "definitely-unknown-driver")
        },
        mock,
    );
    let err = slot.activate().unwrap_err();
    assert!(matches!(err, DeviceError::UnknownPersonality { .. }));
}

#[test]
fn activate_xe_already_bound_errors_at_final_bind_match() {
    let bdf = "0000:31:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.current_driver
        .insert(bdf.to_string(), Some("xe".into()));
    let mut slot = DeviceSlot::with_sysfs(
        DeviceConfig {
            boot_personality: "xe".into(),
            shared: None,
            ..base_config(bdf, "xe")
        },
        mock,
    );
    let err = slot.activate().unwrap_err();
    assert!(matches!(err, DeviceError::UnknownPersonality { .. }));
}

#[test]
fn activate_supported_but_non_bindable_personality_errors() {
    let bdf = "0000:11:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.current_driver
        .insert(bdf.to_string(), Some("xe".into()));
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "xe"), mock);
    let err = slot.activate().unwrap_err();
    assert!(matches!(err, DeviceError::UnknownPersonality { .. }));
}

#[test]
fn activate_active_drm_blocks_rebind() {
    let bdf = "0000:12:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.current_driver
        .insert(bdf.to_string(), Some("nouveau".into()));
    mock.drm_consumers.insert(bdf.to_string(), true);
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    let err = slot.activate().unwrap_err();
    assert!(matches!(err, DeviceError::ActiveDrmConsumers { .. }));
}

#[test]
fn activate_nouveau_no_rebind_sets_personality_from_sysfs() {
    let bdf = "0000:13:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.current_driver
        .insert(bdf.to_string(), Some("nouveau".into()));
    mock.drm_card
        .insert(bdf.to_string(), Some("/dev/dri/card7".into()));
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "nouveau"), mock);
    slot.activate().expect("nouveau already bound");
    assert_eq!(
        slot.personality,
        Personality::Nouveau {
            drm_card: Some("/dev/dri/card7".into())
        }
    );
}

#[test]
fn activate_amdgpu_no_rebind_sets_personality() {
    let bdf = "0000:14:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.current_driver
        .insert(bdf.to_string(), Some("amdgpu".into()));
    mock.drm_card
        .insert(bdf.to_string(), Some("/dev/dri/card2".into()));
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "amdgpu"), mock);
    slot.activate().expect("amdgpu already bound");
    assert_eq!(
        slot.personality,
        Personality::Amdgpu {
            drm_card: Some("/dev/dri/card2".into())
        }
    );
}

#[test]
fn activate_nvidia_no_rebind_sets_personality() {
    let bdf = "0000:15:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.current_driver
        .insert(bdf.to_string(), Some("nvidia".into()));
    mock.drm_card.insert(bdf.to_string(), None);
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "nvidia"), mock);
    slot.activate().expect("nvidia already bound");
    assert_eq!(slot.personality, Personality::Nvidia { drm_card: None });
}

#[test]
fn activate_akida_pcie_no_rebind_sets_akida() {
    let bdf = "0000:16:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.current_driver
        .insert(bdf.to_string(), Some("akida-pcie".into()));
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "akida-pcie"), mock);
    slot.activate().expect("akida already bound");
    assert_eq!(slot.personality, Personality::Akida);
}

#[test]
fn activate_vfio_skip_rebind_still_requires_ember_for_bind_vfio() {
    let _guard = EmberClient::disable_for_test();
    let bdf = "0000:17:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.current_driver
        .insert(bdf.to_string(), Some("vfio-pci".into()));
    mock.iommu_group.insert(bdf.to_string(), 99);
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    let err = slot.activate().unwrap_err();
    assert!(
        matches!(err, DeviceError::VfioOpen { .. }),
        "expected VFIO bind to fail without a real device/ember; got {err:?}"
    );
}

#[test]
fn activate_needs_rebind_without_ember_errors_driver_bind() {
    let _guard = EmberClient::disable_for_test();
    let bdf = "0000:18:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.current_driver
        .insert(bdf.to_string(), Some("nouveau".into()));
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    let err = slot.activate().unwrap_err();
    match err {
        DeviceError::DriverBind { reason, .. } => {
            assert!(reason.contains("ember"), "unexpected: {reason}");
        }
        DeviceError::VfioOpen { .. } => {}
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn refresh_power_state_d3hot_link_width_changes() {
    let bdf = "0000:24:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.power_state.insert(bdf.to_string(), PowerState::D3Hot);
    mock.link_width.insert(bdf.to_string(), Some(4));
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    slot.check_health();
    assert_eq!(slot.health.power, PowerState::D3Hot);
    assert_eq!(slot.health.pci_link_width, Some(4));
    assert!(!slot.health.vram_alive);
}

#[test]
fn refresh_power_state_d3cold_and_link_width() {
    let bdf = "0000:19:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.power_state.insert(bdf.to_string(), PowerState::D3Cold);
    mock.link_width.insert(bdf.to_string(), Some(1));
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    slot.refresh_power_state();
    assert_eq!(slot.health.power, PowerState::D3Cold);
    assert_eq!(slot.health.pci_link_width, Some(1));
}

#[test]
fn sysfs_mock_bind_iommu_increments_counter() {
    let mock = MockSysfs::default();
    mock.bind_iommu_group_to_vfio("0000:01:00.0", 3);
    assert_eq!(mock.bind_iommu_calls.load(Ordering::Relaxed), 1);
}

#[test]
fn swap_ember_unavailable_all_targets_fail() {
    let _guard = EmberClient::disable_for_test();
    let bdf = "0000:20:00.0";
    let targets = [
        "nouveau",
        "amdgpu",
        "xe",
        "i915",
        "akida-pcie",
        "akida",
        "unbound",
        "vfio",
    ];
    for target in targets {
        let mut mock = MockSysfs::default();
        mock.seed_bdf(bdf);
        mock.current_driver
            .insert(bdf.to_string(), Some("vfio-pci".into()));
        let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
        slot.personality = Personality::Vfio { group_id: 1 };
        let err = slot.swap(target).unwrap_err();
        match err {
            DeviceError::DriverBind { reason, .. } => {
                assert!(reason.contains("ember"), "target {target}: {reason}");
            }
            other => panic!("target {target}: unexpected {other:?}"),
        }
    }
}

#[test]
fn swap_unknown_personality_returns_error() {
    let _guard = EmberClient::disable_for_test();
    let bdf = "0000:21:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    let err = slot.swap("not-in-registry-xyz").unwrap_err();
    assert!(
        matches!(
            err,
            DeviceError::UnknownPersonality { .. } | DeviceError::DriverBind { .. }
        ),
        "swap consults ember before local registry; expected unknown personality or ember bind error, got {err:?}"
    );
}

#[test]
fn swap_refuses_nvidia_when_sysfs_reports_nvidia() {
    let _guard = EmberClient::disable_for_test();
    let bdf = "0000:22:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.current_driver
        .insert(bdf.to_string(), Some("nvidia".into()));
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    let err = slot.swap("nouveau").unwrap_err();
    assert!(matches!(err, DeviceError::DriverBind { .. }));
}

#[test]
fn activate_from_ember_invalid_fds_does_not_apply_vfio_personality() {
    let bdf = "0000:23:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.iommu_group.insert(bdf.to_string(), 4242);
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    let container: OwnedFd = std::fs::File::open("/dev/null")
        .expect("open /dev/null")
        .into();
    let group: OwnedFd = std::fs::File::open("/dev/null")
        .expect("open /dev/null")
        .into();
    let device_fd: OwnedFd = std::fs::File::open("/dev/null")
        .expect("open /dev/null")
        .into();
    let fds = coral_driver::vfio::ReceivedVfioFds::Legacy {
        container,
        group,
        device: device_fd,
    };
    let result = slot.activate_from_ember(fds);
    assert!(
        result.is_err(),
        "/dev/null fds are not a valid VFIO device triple"
    );
    assert_ne!(
        slot.personality,
        Personality::Vfio { group_id: 4242 },
        "personality must not update on failed activation"
    );
}

#[test]
fn swap_vfio_pci_alias_requires_ember_like_vfio() {
    let _guard = EmberClient::disable_for_test();
    let bdf = "0000:25:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.current_driver
        .insert(bdf.to_string(), Some("vfio-pci".into()));
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    slot.personality = Personality::Vfio { group_id: 2 };
    let err = slot.swap("vfio-pci").unwrap_err();
    match err {
        DeviceError::DriverBind { reason, .. } => assert!(reason.contains("ember"), "{reason}"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn reclaim_rejects_non_vfio_personality() {
    let bdf = "0000:26:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "nouveau"), mock);
    slot.personality = Personality::Nouveau { drm_card: None };
    let err = slot.reclaim().unwrap_err();
    assert!(matches!(err, DeviceError::DriverBind { .. }));
}

#[test]
fn reclaim_vfio_without_holder_fails_without_ember() {
    let _guard = EmberClient::disable_for_test();
    let bdf = "0000:28:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    slot.personality = Personality::Vfio { group_id: 3 };
    slot.test_set_vfio_override(Some(false));
    let err = slot.reclaim().unwrap_err();
    assert!(
        matches!(err, DeviceError::VfioOpen { .. }),
        "expected ember missing / fds error, got {err:?}"
    );
}

#[test]
fn lend_rejects_non_vfio_personality() {
    let bdf = "0000:29:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "amdgpu"), mock);
    slot.personality = Personality::Amdgpu { drm_card: None };
    let err = slot.lend().unwrap_err();
    assert!(matches!(err, DeviceError::DriverBind { .. }));
}

#[test]
fn lend_vfio_personality_without_open_fd_errors() {
    let bdf = "0000:2b:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    let mut slot = DeviceSlot::with_sysfs(base_config(bdf, "vfio"), mock);
    slot.personality = Personality::Vfio { group_id: 11 };
    let err = slot.lend().unwrap_err();
    assert!(matches!(err, DeviceError::DriverBind { .. }));
}

#[test]
fn activate_intel_xe_supported_but_bind_match_errors_like_xe() {
    let bdf = "0000:2a:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.current_driver
        .insert(bdf.to_string(), Some("xe".into()));
    let mut slot = DeviceSlot::with_sysfs(
        DeviceConfig {
            boot_personality: "xe".into(),
            shared: None,
            ..base_config(bdf, "xe")
        },
        mock,
    );
    let err = slot.activate().unwrap_err();
    assert!(matches!(err, DeviceError::UnknownPersonality { .. }));
}
