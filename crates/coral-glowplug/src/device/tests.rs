// SPDX-License-Identifier: AGPL-3.0-only

use crate::MockSysfs;
use crate::config::DeviceConfig;
use crate::ember::EmberClient;
use crate::error::DeviceError;
use crate::personality::Personality;
use std::sync::Arc;

use super::*;

#[test]
fn test_is_faulted_read_pci_dead() {
    assert!(is_faulted_read(0xDEAD_DEAD));
}

#[test]
fn test_is_faulted_read_all_ones() {
    assert!(is_faulted_read(0xFFFF_FFFF));
}

#[test]
fn test_is_faulted_read_badf() {
    assert!(is_faulted_read((PCI_FAULT_BADF as u32) << 16));
}

#[test]
fn test_is_faulted_read_bad0() {
    assert!(is_faulted_read((PCI_FAULT_BAD0 as u32) << 16));
}

#[test]
fn test_is_faulted_read_bad1() {
    assert!(is_faulted_read((PCI_FAULT_BAD1 as u32) << 16));
}

#[test]
fn test_is_faulted_read_valid() {
    assert!(!is_faulted_read(0x0000_0000));
    assert!(!is_faulted_read(0x1234_5678));
    assert!(!is_faulted_read(0x0001_0000));
}

#[test]
fn test_pci_constants() {
    assert_eq!(PCI_READ_DEAD, 0xDEAD_DEAD);
    assert_eq!(PCI_READ_ALL_ONES, 0xFFFF_FFFF);
    assert_eq!(PCI_FAULT_BADF, 0xBADF);
    assert_eq!(PCI_FAULT_BAD0, 0xBAD0);
    assert_eq!(PCI_FAULT_BAD1, 0xBAD1);
}

#[test]
fn test_power_state_display() {
    assert_eq!(PowerState::D0.to_string(), "D0");
    assert_eq!(PowerState::D3Hot.to_string(), "D3hot");
    assert_eq!(PowerState::D3Cold.to_string(), "D3cold");
    assert_eq!(PowerState::Unknown.to_string(), "unknown");
}

#[test]
fn test_device_health_defaults_in_slot() {
    let config = DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let slot = DeviceSlot::new(config);
    assert!(!slot.health.vram_alive);
    assert_eq!(slot.health.boot0, 0);
    assert_eq!(slot.health.pmc_enable, 0);
    assert_eq!(slot.health.power, PowerState::Unknown);
    assert!(slot.health.pci_link_width.is_none());
    assert_eq!(slot.health.domains_alive, 0);
    assert_eq!(slot.health.domains_faulted, 0);
}

#[test]
fn test_device_slot_new_with_mock_config() {
    let config = DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: Some("Test GPU".into()),
        boot_personality: "nouveau".into(),
        power_policy: "power_save".into(),
        health_policy: "passive".into(),
        role: Some("compute".into()),
        oracle_dump: Some("/tmp/dump.txt".into()),
        shared: None,
    };
    let slot = DeviceSlot::new(config.clone());
    assert_eq!(slot.bdf.as_ref(), "0000:99:00.0");
    assert_eq!(slot.config.name.as_deref(), Some("Test GPU"));
    assert_eq!(slot.config.boot_personality, "nouveau");
    assert_eq!(slot.config.power_policy, "power_save");
    assert_eq!(slot.config.role.as_deref(), Some("compute"));
    assert_eq!(slot.config.oracle_dump.as_deref(), Some("/tmp/dump.txt"));
    assert_eq!(slot.personality, Personality::Unbound);
    assert!(!slot.has_vfio());
}

#[test]
fn test_device_slot_has_vfio_initially_false() {
    let config = DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let slot = DeviceSlot::new(config);
    assert!(!slot.has_vfio());
}

#[test]
fn test_device_health_struct() {
    let health = DeviceHealth {
        vram_alive: true,
        boot0: 0x1234_5678,
        pmc_enable: 0x9abc_def0,
        power: PowerState::D0,
        pci_link_width: Some(16),
        domains_alive: 9,
        domains_faulted: 0,
    };
    assert!(health.vram_alive);
    assert_eq!(health.boot0, 0x1234_5678);
    assert_eq!(health.pmc_enable, 0x9abc_def0);
    assert_eq!(health.power, PowerState::D0);
    assert_eq!(health.pci_link_width, Some(16));
    assert_eq!(health.domains_alive, 9);
    assert_eq!(health.domains_faulted, 0);
}

#[test]
fn test_power_state_equality() {
    assert_eq!(PowerState::D0, PowerState::D0);
    assert_ne!(PowerState::D0, PowerState::D3Hot);
}

#[test]
fn test_activate_nonexistent_bdf_with_drm_check_does_not_panic() {
    let _guard = EmberClient::disable_for_test();
    let config = DeviceConfig {
        bdf: "0000:ff:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut slot = DeviceSlot::new(config);
    let result = slot.activate();
    drop(result);
}

#[test]
fn test_release_nonexistent_bdf_does_not_panic() {
    let config = DeviceConfig {
        bdf: "0000:ff:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut slot = DeviceSlot::new(config);
    let result = slot.release();
    assert!(
        result.is_ok(),
        "release on nonexistent should succeed (no DRM consumers)"
    );
    assert_eq!(slot.personality, Personality::Unbound);
}

#[test]
fn test_swap_nonexistent_bdf_does_not_panic() {
    let _guard = EmberClient::disable_for_test();
    let config = DeviceConfig {
        bdf: "0000:ff:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut slot = DeviceSlot::new(config);
    let result = slot.swap("nouveau");
    // Should not panic — guard passes (no DRM), bind may fail
    drop(result);
}

#[test]
fn test_lend_requires_vfio_personality() {
    let config = DeviceConfig {
        bdf: "0000:ff:00.0".into(),
        name: None,
        boot_personality: "nouveau".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut slot = DeviceSlot::new(config);
    slot.personality = Personality::Nouveau { drm_card: None };
    let result = slot.lend();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not VFIO"));
}

#[test]
fn test_lend_returns_error_when_no_fd() {
    let config = DeviceConfig {
        bdf: "0000:ff:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut slot = DeviceSlot::new(config);
    slot.personality = Personality::Vfio { group_id: 42 };
    // No vfio_device set — lend should fail
    let result = slot.lend();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("already lent"));
}

#[test]
fn test_reclaim_requires_vfio_personality() {
    let config = DeviceConfig {
        bdf: "0000:ff:00.0".into(),
        name: None,
        boot_personality: "nouveau".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut slot = DeviceSlot::new(config);
    slot.personality = Personality::Unbound;
    let result = slot.reclaim();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not VFIO"));
}

#[test]
fn test_active_drm_consumers_error_display() {
    let err = crate::error::DeviceError::ActiveDrmConsumers {
        bdf: Arc::from("0000:03:00.0"),
    };
    let msg = err.to_string();
    assert!(msg.contains("active DRM consumers"));
    assert!(msg.contains("0000:03:00.0"));
    assert!(msg.contains("crash the kernel"));
}

#[test]
fn read_register_returns_none_without_vfio() {
    let config = DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let slot = DeviceSlot::new(config);
    assert!(slot.read_register(0x00_0000).is_none());
}

#[test]
fn dump_registers_empty_without_vfio() {
    let config = DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let slot = DeviceSlot::new(config);
    assert!(slot.dump_registers(&[0x00_0000]).is_empty());
}

#[test]
fn last_snapshot_empty_until_snapshot() {
    let config = DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let slot = DeviceSlot::new(config);
    assert!(slot.last_snapshot().is_empty());
}

#[test]
fn snapshot_registers_no_vfio_is_noop() {
    let config = DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut slot = DeviceSlot::new(config);
    slot.snapshot_registers();
    assert!(slot.last_snapshot().is_empty());
}

#[test]
fn refresh_power_state_does_not_panic() {
    let config = DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut slot = DeviceSlot::new(config);
    slot.refresh_power_state();
    assert_eq!(slot.health.power, PowerState::Unknown);
}

#[test]
fn wait_quiescence_without_vfio_is_trivially_true() {
    let config = DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let slot = DeviceSlot::new(config);
    assert!(slot.wait_quiescence(std::time::Duration::from_millis(1)));
}

#[test]
fn check_health_without_vfio_clears_domain_counts() {
    let config = DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut slot = DeviceSlot::new(config);
    slot.check_health();
    assert!(!slot.health.vram_alive);
    assert_eq!(slot.health.domains_alive, 0);
    assert_eq!(slot.health.domains_faulted, 0);
}

#[test]
fn resurrect_hbm2_fails_without_ember_for_unknown_vendor() {
    let config = DeviceConfig {
        bdf: "0000:99:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut slot = DeviceSlot::new(config);
    // sysfs read_pci_ids returns 0,0 for fake BDF — not a known HBM2 vendor
    let err = slot
        .resurrect_hbm2()
        .expect_err("expected driver bind error");
    assert!(err.to_string().contains("HBM2") || err.to_string().contains("vendor"));
}

#[test]
fn mock_refresh_power_state_updates_health_from_sysfs_ops() {
    let bdf = "0000:01:00.0";
    let config = DeviceConfig {
        bdf: bdf.into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.power_state.insert(bdf.to_string(), PowerState::D0);
    mock.link_width.insert(bdf.to_string(), Some(16));
    let mut slot = DeviceSlot::with_sysfs(config, mock);
    slot.refresh_power_state();
    assert_eq!(slot.health.power, PowerState::D0);
    assert_eq!(slot.health.pci_link_width, Some(16));
}

#[test]
fn mock_check_health_refreshes_power_and_link_from_sysfs_ops() {
    let bdf = "0000:02:00.0";
    let config = DeviceConfig {
        bdf: bdf.into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.power_state.insert(bdf.to_string(), PowerState::D3Hot);
    mock.link_width.insert(bdf.to_string(), Some(8));
    let mut slot = DeviceSlot::with_sysfs(config, mock);
    slot.check_health();
    assert_eq!(slot.health.power, PowerState::D3Hot);
    assert_eq!(slot.health.pci_link_width, Some(8));
}

#[test]
fn mock_swap_refuses_when_nvidia_driver_reported() {
    let _guard = EmberClient::disable_for_test();
    let bdf = "0000:03:00.0";
    let config = DeviceConfig {
        bdf: bdf.into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.current_driver
        .insert(bdf.to_string(), Some("nvidia".into()));
    let mut slot = DeviceSlot::with_sysfs(config, mock);
    let err = slot.swap("nouveau").unwrap_err();
    assert!(matches!(err, DeviceError::DriverBind { .. }));
}

#[test]
fn mock_wait_quiescence_respects_test_override_quiescent() {
    let bdf = "0000:50:00.0";
    let config = DeviceConfig {
        bdf: bdf.into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    let mut slot = DeviceSlot::with_sysfs(config, mock);
    slot.test_set_quiescence_override(Some(true));
    assert!(slot.wait_quiescence(std::time::Duration::from_millis(1)));
}

#[test]
fn mock_snapshot_registers_no_vfio_leaves_empty_snapshot() {
    let bdf = "0000:51:00.0";
    let config = DeviceConfig {
        bdf: bdf.into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    let mut slot = DeviceSlot::with_sysfs(config, mock);
    slot.snapshot_registers();
    assert!(slot.last_snapshot().is_empty());
}

#[test]
fn resurrect_hbm2_amd_warm_driver_is_amdgpu_without_ember() {
    let _guard = EmberClient::disable_for_test();
    let bdf = "0000:52:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.pci_ids
        .insert(bdf.to_string(), (crate::pci_ids::AMD_VENDOR_ID, 0x66a0));
    let mut slot = DeviceSlot::with_sysfs(
        DeviceConfig {
            bdf: bdf.into(),
            name: None,
            boot_personality: "vfio".into(),
            power_policy: "always_on".into(),
            health_policy: "passive".into(),
            role: None,
            oracle_dump: None,
            shared: None,
        },
        mock,
    );
    let err = slot.resurrect_hbm2().expect_err("ember required");
    let s = err.to_string();
    assert!(
        s.contains("ember") || s.contains("amdgpu"),
        "unexpected message: {s}"
    );
}

#[test]
fn activate_unknown_boot_personality_returns_error() {
    let _guard = EmberClient::disable_for_test();
    let bdf = "0000:60:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    let mut slot = DeviceSlot::with_sysfs(
        DeviceConfig {
            bdf: bdf.into(),
            name: None,
            boot_personality: "not-a-listed-personality".into(),
            power_policy: "always_on".into(),
            health_policy: "passive".into(),
            role: None,
            oracle_dump: None,
            shared: None,
        },
        mock,
    );
    let err = slot.activate().expect_err("unknown personality");
    assert!(matches!(err, DeviceError::UnknownPersonality { .. }));
}

#[test]
fn activate_nvidia_oracle_registered_but_not_implemented_returns_unknown() {
    let _guard = EmberClient::disable_for_test();
    let bdf = "0000:61:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.current_driver
        .insert(bdf.to_string(), Some("nvidia_oracle".into()));
    let mut slot = DeviceSlot::with_sysfs(
        DeviceConfig {
            bdf: bdf.into(),
            name: None,
            boot_personality: "nvidia_oracle".into(),
            power_policy: "always_on".into(),
            health_policy: "passive".into(),
            role: None,
            oracle_dump: None,
            shared: None,
        },
        mock,
    );
    let err = slot.activate().expect_err("binding path not implemented");
    match err {
        DeviceError::UnknownPersonality { personality, .. } => {
            assert_eq!(personality, "nvidia_oracle");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn try_acquire_busy_guard_releases_flag_on_drop() {
    let bdf = "0000:62:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    let slot = DeviceSlot::with_sysfs(
        DeviceConfig {
            bdf: bdf.into(),
            name: None,
            boot_personality: "vfio".into(),
            power_policy: "always_on".into(),
            health_policy: "passive".into(),
            role: None,
            oracle_dump: None,
            shared: None,
        },
        mock,
    );
    assert!(!slot.is_busy());
    let guard = slot.try_acquire_busy().expect("first acquire");
    assert!(slot.is_busy());
    assert!(slot.try_acquire_busy().is_none());
    drop(guard);
    assert!(!slot.is_busy());
    assert!(slot.try_acquire_busy().is_some());
}

#[test]
fn reset_device_without_vfio_returns_error() {
    let bdf = "0000:63:00.0";
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    let slot = DeviceSlot::with_sysfs(
        DeviceConfig {
            bdf: bdf.into(),
            name: None,
            boot_personality: "vfio".into(),
            power_policy: "always_on".into(),
            health_policy: "passive".into(),
            role: None,
            oracle_dump: None,
            shared: None,
        },
        mock,
    );
    let err = slot.reset_device().expect_err("no VFIO device");
    assert!(matches!(err, DeviceError::VfioOpen { .. }));
}

#[test]
fn mock_release_errors_when_drm_consumers_reported() {
    let bdf = "0000:04:00.0";
    let config = DeviceConfig {
        bdf: bdf.into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        health_policy: "passive".into(),
        role: None,
        oracle_dump: None,
        shared: None,
    };
    let mut mock = MockSysfs::default();
    mock.seed_bdf(bdf);
    mock.drm_consumers.insert(bdf.to_string(), true);
    let mut slot = DeviceSlot::with_sysfs(config, mock);
    let err = slot.release().unwrap_err();
    assert!(matches!(err, DeviceError::ActiveDrmConsumers { .. }));
}
