// SPDX-License-Identifier: AGPL-3.0-only

use crate::config::DeviceConfig;
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
        role: None,
        oracle_dump: None,
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
        role: Some("compute".into()),
        oracle_dump: Some("/tmp/dump.txt".into()),
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
        role: None,
        oracle_dump: None,
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
    let config = DeviceConfig {
        bdf: "0000:ff:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        role: None,
        oracle_dump: None,
    };
    let mut slot = DeviceSlot::new(config);
    // activate on a nonexistent device won't have DRM consumers,
    // so it should proceed (and fail at the bind stage, not the guard)
    let result = slot.activate();
    // Either succeeds (unlikely) or fails at bind — but must NOT panic
    drop(result);
}

#[test]
fn test_release_nonexistent_bdf_does_not_panic() {
    let config = DeviceConfig {
        bdf: "0000:ff:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        role: None,
        oracle_dump: None,
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
    let config = DeviceConfig {
        bdf: "0000:ff:00.0".into(),
        name: None,
        boot_personality: "vfio".into(),
        power_policy: "always_on".into(),
        role: None,
        oracle_dump: None,
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
        role: None,
        oracle_dump: None,
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
        role: None,
        oracle_dump: None,
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
        role: None,
        oracle_dump: None,
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
