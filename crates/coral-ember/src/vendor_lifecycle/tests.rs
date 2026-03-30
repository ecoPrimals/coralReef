// SPDX-License-Identifier: AGPL-3.0-only

use super::{
    AmdRdnaLifecycle, AmdVega20Lifecycle, BrainChipLifecycle, GenericLifecycle, IntelXeLifecycle,
    NvidiaKeplerLifecycle, NvidiaLifecycle, NvidiaOracleLifecycle, RebindStrategy, ResetMethod,
    VendorLifecycle, detect_lifecycle, detect_lifecycle_for_target, is_amd_vega20,
    is_nvidia_kepler, lifecycle_from_pci_ids,
};

#[test]
fn vega20_ids_recognized() {
    assert!(is_amd_vega20(0x66a0)); // MI50
    assert!(is_amd_vega20(0x66a1)); // MI60
    assert!(is_amd_vega20(0x66af)); // Radeon VII
    assert!(!is_amd_vega20(0x7340)); // Navi 14
}

#[test]
fn amd_vega20_uses_pm_reset_for_native() {
    let lc = AmdVega20Lifecycle { device_id: 0x66af };
    assert_eq!(lc.rebind_strategy("amdgpu"), RebindStrategy::PmResetAndBind);
    assert_eq!(lc.rebind_strategy("vfio-pci"), RebindStrategy::SimpleBind);
}

#[test]
fn kepler_ids_recognized() {
    assert!(is_nvidia_kepler(0x102d)); // GK210 K80
    assert!(is_nvidia_kepler(0x100c)); // GK110 GTX Titan
    assert!(is_nvidia_kepler(0x1024)); // GK110B K40m
    assert!(!is_nvidia_kepler(0x1d81)); // GV100 Titan V
    assert!(!is_nvidia_kepler(0x2204)); // GA102 RTX 3090
}

#[test]
fn kepler_uses_rescan_fallback_for_native() {
    let lc = NvidiaKeplerLifecycle { device_id: 0x102d };
    assert_eq!(
        lc.rebind_strategy("nouveau"),
        RebindStrategy::SimpleWithRescanFallback
    );
    assert_eq!(lc.rebind_strategy("vfio-pci"), RebindStrategy::SimpleBind);
}

#[test]
fn kepler_only_has_remove_rescan_reset() {
    let lc = NvidiaKeplerLifecycle { device_id: 0x102d };
    let methods = lc.available_reset_methods();
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0], ResetMethod::RemoveRescan);
}

#[test]
fn kepler_nouveau_gets_long_settle() {
    let lc = NvidiaKeplerLifecycle { device_id: 0x102d };
    assert_eq!(lc.settle_secs("nouveau"), 20);
    assert_eq!(lc.settle_secs("vfio-pci"), 5);
}

#[test]
fn kepler_description_mentions_kepler() {
    let lc = NvidiaKeplerLifecycle { device_id: 0x102d };
    assert!(lc.description().contains("Kepler"));
}

#[test]
fn kepler_prepare_for_unbind_succeeds_on_missing_sysfs() {
    let lc = NvidiaKeplerLifecycle { device_id: 0x102d };
    lc.prepare_for_unbind("9999:99:99.9", "vfio-pci")
        .expect("Kepler prepare should not fail on missing reset_method");
}

#[test]
fn kepler_stabilize_and_verify_best_effort() {
    let lc = NvidiaKeplerLifecycle { device_id: 0x102d };
    lc.stabilize_after_bind("9999:99:99.9", "nouveau");
    lc.verify_health("9999:99:99.9", "nouveau")
        .expect("health OK when power state unknown");
}

#[test]
fn nvidia_uses_simple_bind() {
    let lc = NvidiaLifecycle { device_id: 0x1d81 };
    assert_eq!(lc.rebind_strategy("nouveau"), RebindStrategy::SimpleBind);
    assert_eq!(lc.rebind_strategy("nvidia"), RebindStrategy::SimpleBind);
    assert_eq!(lc.rebind_strategy("vfio-pci"), RebindStrategy::SimpleBind);
}

#[test]
fn nvidia_nouveau_gets_longer_settle() {
    let lc = NvidiaLifecycle { device_id: 0x1d81 };
    assert_eq!(lc.settle_secs("nouveau"), 10);
    assert_eq!(lc.settle_secs("nvidia"), 5);
}

#[test]
fn intel_xe_simple_bind() {
    let lc = IntelXeLifecycle { device_id: 0x56a0 };
    assert_eq!(lc.rebind_strategy("xe"), RebindStrategy::SimpleBind);
    assert_eq!(lc.rebind_strategy("i915"), RebindStrategy::SimpleBind);
}

#[test]
fn generic_conservative_fallback_for_native() {
    let lc = GenericLifecycle {
        vendor_id: 0xdead,
        device_id: 0xbeef,
    };
    assert_eq!(
        lc.rebind_strategy("some-driver"),
        RebindStrategy::SimpleWithRescanFallback
    );
    assert_eq!(lc.rebind_strategy("vfio-pci"), RebindStrategy::SimpleBind);
}

#[test]
fn nvidia_description() {
    let lc = NvidiaLifecycle { device_id: 0x1D81 };
    assert!(lc.description().contains("NVIDIA"));
    assert!(
        lc.description().contains("reset_method"),
        "description should mention reset_method is disabled"
    );
}

#[test]
fn nvidia_prepare_for_unbind_clears_reset_method() {
    let lc = NvidiaLifecycle { device_id: 0x1d81 };
    let err = lc
        .prepare_for_unbind("not-a-bdf", "nouveau")
        .expect_err("should fail on fake BDF (sysfs path absent)");
    assert!(!err.is_empty());
}

#[test]
fn nvidia_stabilize_after_bind_clears_reset_method_best_effort() {
    let lc = NvidiaLifecycle { device_id: 0x1d81 };
    lc.stabilize_after_bind("9999:99:99.9", "vfio-pci");
}

#[test]
fn amd_vega20_description_and_settle() {
    let lc = AmdVega20Lifecycle { device_id: 0x66af };
    assert!(lc.description().contains("Vega 20"));
    assert_eq!(lc.settle_secs("vfio-pci"), 3);
    assert_eq!(lc.settle_secs("amdgpu"), 15);
    assert_eq!(lc.settle_secs("some-other"), 15);
}

#[test]
fn amd_rdna_lifecycle_basics() {
    let lc = AmdRdnaLifecycle { device_id: 0x73BF };
    assert!(lc.description().contains("RDNA"));
    assert_eq!(lc.rebind_strategy("vfio-pci"), RebindStrategy::SimpleBind);
    assert_eq!(lc.rebind_strategy("amdgpu"), RebindStrategy::PmResetAndBind);
    assert_eq!(lc.settle_secs("any"), 12);
}

#[test]
fn intel_xe_description_and_settle() {
    let lc = IntelXeLifecycle { device_id: 0x56a0 };
    assert!(lc.description().contains("Intel"));
    assert!(lc.description().contains("xe") || lc.description().contains("i915"));
    assert_eq!(lc.settle_secs("i915"), 2);
    assert_eq!(lc.settle_secs("xe"), 2);
    assert_eq!(lc.settle_secs("vfio-pci"), 2);
}

#[test]
fn brainchip_lifecycle_basics() {
    let lc = BrainChipLifecycle { device_id: 0x0001 };
    assert!(lc.description().contains("BrainChip"));
    assert_eq!(lc.rebind_strategy("akida"), RebindStrategy::SimpleBind);
    assert_eq!(lc.settle_secs("akida"), 3);
}

#[test]
fn generic_description_and_settle() {
    let lc = GenericLifecycle {
        vendor_id: 0xdead,
        device_id: 0xbeef,
    };
    assert!(lc.description().contains("Unknown vendor"));
    assert_eq!(lc.settle_secs("any"), 10);
    assert_eq!(lc.rebind_strategy("vfio"), RebindStrategy::SimpleBind);
}

#[test]
fn rebind_strategy_debug_format() {
    assert!(format!("{:?}", RebindStrategy::SimpleBind).contains("SimpleBind"));
    assert!(format!("{:?}", RebindStrategy::PmResetAndBind).contains("PmResetAndBind"));
    assert!(format!("{:?}", RebindStrategy::PciRescan).contains("PciRescan"));
    assert!(
        format!("{:?}", RebindStrategy::SimpleWithRescanFallback)
            .contains("SimpleWithRescanFallback")
    );
}

#[test]
fn rebind_strategy_clone_eq() {
    let a = RebindStrategy::SimpleBind;
    let b = a;
    assert_eq!(a, b);
}

#[test]
fn detect_lifecycle_unknown_vendor_is_generic() {
    let lc = detect_lifecycle("9999:99:99.9");
    assert!(lc.description().contains("Unknown"));
}

#[test]
fn generic_prepare_vfio_pci_clears_reset_method_path_best_effort() {
    let lc = GenericLifecycle {
        vendor_id: 0xdead,
        device_id: 0xbeef,
    };
    lc.prepare_for_unbind("9999:99:99.9", "vfio-pci").unwrap();
}

#[test]
fn generic_prepare_non_vfio_skips_reset_override() {
    let lc = GenericLifecycle {
        vendor_id: 0xdead,
        device_id: 0xbeef,
    };
    lc.prepare_for_unbind("9999:99:99.9", "amdgpu").unwrap();
}

#[test]
fn nvidia_verify_health_ok_when_sysfs_missing_or_not_d3cold() {
    let lc = NvidiaLifecycle { device_id: 0x1d81 };
    lc.verify_health("9999:99:99.9", "nouveau").unwrap();
}

#[test]
fn intel_amd_rdna_brainchip_verify_health_ok_without_d3cold_sysfs() {
    let intel = IntelXeLifecycle { device_id: 0x56a0 };
    let rdna = AmdRdnaLifecycle { device_id: 0x73bf };
    let brain = BrainChipLifecycle { device_id: 1 };
    intel
        .verify_health("9999:99:99.9", "vfio-pci")
        .expect("VFIO has no DRM node");
    rdna.verify_health("9999:99:99.9", "amdgpu").unwrap();
    brain.verify_health("9999:99:99.9", "akida-pcie").unwrap();
}

#[test]
fn intel_verify_health_requires_drm_sysfs_for_native_drivers() {
    let intel = IntelXeLifecycle { device_id: 0x56a0 };
    let err = intel
        .verify_health("9999:99:99.9", "xe")
        .expect_err("missing drm/card on fake BDF");
    assert!(err.contains("DRM") || err.contains("drm"), "{err}");
}

#[test]
fn intel_uses_trait_default_reset_methods() {
    let lc = IntelXeLifecycle { device_id: 0x56a0 };
    let methods = lc.available_reset_methods();
    assert_eq!(methods.len(), 2);
    assert_eq!(methods[0], ResetMethod::VfioFlr);
    assert_eq!(methods[1], ResetMethod::SysfsSbr);
}

#[test]
fn generic_verify_health_ok_when_power_unknown() {
    let lc = GenericLifecycle {
        vendor_id: 0xdead,
        device_id: 0xbeef,
    };
    lc.verify_health("9999:99:99.9", "vfio-pci").unwrap();
}

#[test]
fn amd_vega20_stabilize_amdgpu_writes_autosuspend_paths_best_effort() {
    let lc = AmdVega20Lifecycle { device_id: 0x66af };
    lc.stabilize_after_bind("9999:99:99.9", "amdgpu");
}

#[test]
fn amd_vega20_stabilize_non_amdgpu_skips_autosuspend() {
    let lc = AmdVega20Lifecycle { device_id: 0x66af };
    lc.stabilize_after_bind("9999:99:99.9", "vfio-pci");
}

#[test]
fn nvidia_settle_secs_branches() {
    let lc = NvidiaLifecycle { device_id: 1 };
    assert_eq!(lc.settle_secs("vfio-pci"), 5);
    assert_eq!(lc.settle_secs("other"), 5);
}

#[test]
fn generic_rebind_vfio_alias() {
    let lc = GenericLifecycle {
        vendor_id: 1,
        device_id: 2,
    };
    assert_eq!(lc.rebind_strategy("vfio"), RebindStrategy::SimpleBind);
}

#[test]
fn lifecycle_from_pci_ids_matches_each_vendor_arm() {
    let nvidia = lifecycle_from_pci_ids(0x10de, 0x1d81);
    assert!(nvidia.description().contains("NVIDIA"));
    assert!(
        !nvidia.description().contains("Kepler"),
        "Volta should not be Kepler"
    );

    let kepler = lifecycle_from_pci_ids(0x10de, 0x102d);
    assert!(kepler.description().contains("Kepler"));

    let vega = lifecycle_from_pci_ids(0x1002, 0x66a0);
    assert!(vega.description().contains("Vega 20"));

    let rdna = lifecycle_from_pci_ids(0x1002, 0x73bf);
    assert!(rdna.description().contains("RDNA"));

    let intel = lifecycle_from_pci_ids(0x8086, 0x56a0);
    assert!(intel.description().contains("Intel"));

    let akida = lifecycle_from_pci_ids(0x1e7c, 0xbca1);
    assert!(akida.description().contains("BrainChip"));

    let generic = lifecycle_from_pci_ids(0xabcd, 0x1234);
    assert!(generic.description().contains("Unknown"));
}

#[test]
fn is_amd_vega20_excludes_adjacent_device_ids() {
    assert!(!is_amd_vega20(0x669f));
    assert!(!is_amd_vega20(0x66b0));
}

#[test]
fn amd_vega20_prepare_for_unbind_errors_on_garbage_bdf() {
    let lc = AmdVega20Lifecycle { device_id: 0x66a0 };
    let err = lc
        .prepare_for_unbind("not-a-bdf", "vfio-pci")
        .expect_err("reset_method sysfs");
    assert!(!err.is_empty());
}

#[test]
fn nvidia_oracle_lifecycle_description_and_strategy() {
    let lc = NvidiaOracleLifecycle {
        device_id: 0x1d81,
        module_name: "nvidia_oracle_535".to_string(),
    };
    assert!(lc.description().contains("Oracle"));
    assert_eq!(lc.rebind_strategy("amdgpu"), RebindStrategy::SimpleBind);
    assert_eq!(lc.settle_secs("nouveau"), 10);
    assert_eq!(lc.settle_secs("nvidia_oracle_535"), 8);
}

#[test]
fn nvidia_oracle_prepare_and_verify_best_effort_on_missing_sysfs() {
    let lc = NvidiaOracleLifecycle {
        device_id: 0x1d81,
        module_name: "nvidia_oracle".to_string(),
    };
    let err = lc
        .prepare_for_unbind("9999:99:99.9", "vfio-pci")
        .expect_err("reset_method write on absent device");
    assert!(!err.is_empty());
    lc.verify_health("9999:99:99.9", "nvidia_oracle")
        .expect("health when power state unknown");
}

#[test]
fn detect_lifecycle_for_target_nvidia_oracle_prefix_uses_oracle_lifecycle() {
    let lc = detect_lifecycle_for_target("9999:99:99.9", "nvidia_oracle");
    assert!(lc.description().contains("Oracle"));
    let lc_suffixed = detect_lifecycle_for_target("9999:99:99.9", "nvidia_oracle_535");
    assert!(lc_suffixed.description().contains("Oracle"));
}

#[test]
fn detect_lifecycle_for_target_plain_driver_delegates_to_detect_lifecycle() {
    let oracle = detect_lifecycle_for_target("9999:99:99.9", "nvidia_oracle");
    let plain = detect_lifecycle_for_target("9999:99:99.9", "nouveau");
    assert!(oracle.description().contains("Oracle"));
    assert!(
        plain.description().contains("Unknown"),
        "non-oracle target should use PCI-detected lifecycle"
    );
}
