// SPDX-License-Identifier: AGPL-3.0-only
//! Integration tests for [`coral_ember::detect_lifecycle`] and [`coral_ember::VendorLifecycle`].

use coral_ember::{RebindStrategy, detect_lifecycle};

#[test]
fn detect_unknown_bdf_yields_generic_lifecycle() {
    let lc = detect_lifecycle("9999:99:99.9");
    assert!(lc.description().contains("Unknown"));
}

#[test]
fn generic_rebind_prefers_rescan_fallback_for_arbitrary_native_driver() {
    let lc = detect_lifecycle("9999:99:99.9");
    assert_eq!(
        lc.rebind_strategy("some-driver"),
        RebindStrategy::SimpleWithRescanFallback
    );
    assert_eq!(lc.rebind_strategy("vfio-pci"), RebindStrategy::SimpleBind);
}

#[test]
fn generic_prepare_vfio_pci_is_non_fatal() {
    let lc = detect_lifecycle("9999:99:99.9");
    lc.prepare_for_unbind("9999:99:99.9", "vfio-pci")
        .expect("prepare");
}

#[test]
fn generic_verify_health_ok_without_sysfs_power() {
    let lc = detect_lifecycle("9999:99:99.9");
    lc.verify_health("9999:99:99.9", "vfio-pci")
        .expect("health");
}

#[test]
fn lifecycle_settle_secs_are_positive() {
    let lc = detect_lifecycle("9999:99:99.9");
    assert!(lc.settle_secs("amdgpu") >= 1);
    assert!(lc.settle_secs("vfio-pci") >= 1);
}

#[test]
fn rebind_strategy_variants_are_distinct() {
    assert_ne!(
        RebindStrategy::SimpleBind,
        RebindStrategy::SimpleWithRescanFallback
    );
    assert_ne!(RebindStrategy::PciRescan, RebindStrategy::PmResetAndBind);
}
