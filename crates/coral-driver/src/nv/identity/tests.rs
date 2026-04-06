// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn gpu_identity_nvidia_sm_mapping() {
    let titan_v = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x1D81,
        sysfs_path: String::new(),
    };
    assert_eq!(titan_v.nvidia_sm(), Some(70));

    let non_nvidia = GpuIdentity {
        vendor_id: PCI_VENDOR_AMD,
        device_id: 0x73BF,
        sysfs_path: String::new(),
    };
    assert_eq!(non_nvidia.nvidia_sm(), None);
}

#[test]
fn gpu_identity_amd_arch_mapping() {
    let rdna2 = GpuIdentity {
        vendor_id: PCI_VENDOR_AMD,
        device_id: 0x73BF,
        sysfs_path: String::new(),
    };
    assert_eq!(rdna2.amd_arch(), Some("rdna2"));

    let non_amd = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x73BF,
        sysfs_path: String::new(),
    };
    assert_eq!(non_amd.amd_arch(), None);
}

#[test]
fn nvidia_sm_turing_sm75() {
    // Turing: RTX 2080 (1E82), RTX 2060 (1F03), GTX 1660 Ti (2182)
    let rtx_2080 = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x1E82,
        sysfs_path: String::new(),
    };
    assert_eq!(rtx_2080.nvidia_sm(), Some(75));

    let rtx_2060 = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x1F03,
        sysfs_path: String::new(),
    };
    assert_eq!(rtx_2060.nvidia_sm(), Some(75));

    let gtx_1660_ti = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x2182,
        sysfs_path: String::new(),
    };
    assert_eq!(gtx_1660_ti.nvidia_sm(), Some(75));
}

#[test]
fn nvidia_sm_ampere_ga100_vs_ga102() {
    // Ampere GA100 (A100): SM80
    let a100 = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x20B0,
        sysfs_path: String::new(),
    };
    assert_eq!(a100.nvidia_sm(), Some(80));

    // Ampere GA102 (RTX 3090/3080): SM86
    let rtx_3090 = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x2204,
        sysfs_path: String::new(),
    };
    assert_eq!(rtx_3090.nvidia_sm(), Some(86));

    let rtx_3080 = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x2206,
        sysfs_path: String::new(),
    };
    assert_eq!(rtx_3080.nvidia_sm(), Some(86));
}

#[test]
fn nvidia_sm_ada_lovelace_sm89() {
    // Ada Lovelace AD102 (RTX 4090)
    let ada_ad102 = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x2680,
        sysfs_path: String::new(),
    };
    assert_eq!(ada_ad102.nvidia_sm(), Some(89));

    // Ada Lovelace AD103 (RTX 4080)
    let ada_ad103 = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x2704,
        sysfs_path: String::new(),
    };
    assert_eq!(ada_ad103.nvidia_sm(), Some(89));

    // Ada Lovelace AD104 (RTX 4070) — PCI 0x2786
    let rtx_4070 = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x2786,
        sysfs_path: String::new(),
    };
    assert_eq!(rtx_4070.nvidia_sm(), Some(89));

    // Ada Lovelace AD106 (RTX 4060 Ti)
    let rtx_4060_ti = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x2803,
        sysfs_path: String::new(),
    };
    assert_eq!(rtx_4060_ti.nvidia_sm(), Some(89));

    // Ada Lovelace AD107 (RTX 4060)
    let rtx_4060 = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x2882,
        sysfs_path: String::new(),
    };
    assert_eq!(rtx_4060.nvidia_sm(), Some(89));
}

#[test]
fn nvidia_sm_unknown_device_id_returns_none() {
    let unknown = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x9999,
        sysfs_path: String::new(),
    };
    assert_eq!(unknown.nvidia_sm(), None);

    let fake_vendor = GpuIdentity {
        vendor_id: 0x1234, // intentionally fake vendor
        device_id: 0x1D81,
        sysfs_path: String::new(),
    };
    assert_eq!(fake_vendor.nvidia_sm(), None);
}

#[test]
fn probe_gpu_identity_nonexistent_path_returns_none() {
    // Path parses to node "renderD99999"; /sys/class/drm/renderD99999/device won't exist
    let result = probe_gpu_identity("/tmp/fake/renderD99999");
    assert!(result.is_none());
}

#[test]
fn firmware_check_returns_entries() {
    let entries = check_nouveau_firmware("gv100");
    assert!(!entries.is_empty());
    for (path, _exists) in &entries {
        assert!(path.contains("gv100"));
    }
}

#[test]
fn firmware_inventory_nonexistent_chip() {
    let inv = firmware_inventory("fake_chip_999");
    assert_eq!(inv.chip, "fake_chip_999");
    assert!(!inv.acr.is_present());
    assert!(!inv.gr.is_present());
    assert!(!inv.pmu.is_present());
    assert!(!inv.gsp.is_present());
    assert!(!inv.compute_viable());
}

#[test]
fn firmware_inventory_compute_viable_logic() {
    let mut inv = FirmwareInventory {
        chip: "test".into(),
        acr: FwStatus::Present,
        gr: FwStatus::Present,
        sec2: FwStatus::Present,
        nvdec: FwStatus::Present,
        pmu: FwStatus::Missing,
        gsp: FwStatus::Missing,
    };
    assert!(!inv.compute_viable(), "no PMU or GSP → not viable");
    assert!(!inv.compute_blockers().is_empty());

    inv.pmu = FwStatus::Present;
    assert!(inv.compute_viable(), "PMU present → viable");
    assert!(inv.compute_blockers().is_empty());

    inv.pmu = FwStatus::Missing;
    inv.gsp = FwStatus::Present;
    assert!(inv.compute_viable(), "GSP present → viable (Ampere+ path)");

    inv.gr = FwStatus::Missing;
    assert!(
        !inv.compute_viable(),
        "GR missing → not viable even with GSP"
    );
}

#[test]
fn fw_status_is_present() {
    assert!(FwStatus::Present.is_present());
    assert!(!FwStatus::Missing.is_present());
}

#[test]
fn boot0_gv100_titan_v() {
    assert_eq!(boot0_to_sm(0x1400_00a1), Some(70));
}

#[test]
fn boot0_ga102_rtx3090() {
    assert_eq!(boot0_to_sm(0x1720_00a1), Some(86));
}

#[test]
fn boot0_ad102_rtx4090() {
    assert_eq!(boot0_to_sm(0x1920_00a1), Some(89));
}

#[test]
fn boot0_ga100() {
    assert_eq!(boot0_to_sm(0x1700_00a1), Some(80));
}

#[test]
fn boot0_tu102_turing() {
    assert_eq!(boot0_to_sm(0x1640_00a1), Some(75));
}

#[test]
fn boot0_gh100_hopper() {
    assert_eq!(boot0_to_sm(0x1800_00a1), Some(90));
}

#[test]
fn boot0_gb202_blackwell_consumer() {
    assert_eq!(boot0_to_sm(0x1B20_00A1), Some(120)); // RTX 5090
}

#[test]
fn boot0_gb100_blackwell_datacenter() {
    assert_eq!(boot0_to_sm(0x1A00_00A1), Some(100)); // B100
}

#[test]
fn boot0_unknown_chipset() {
    assert_eq!(boot0_to_sm(0x0000_0000), None);
    assert_eq!(boot0_to_sm(0xFFFF_FFFF), None);
}

#[test]
fn chipset_variant_ada_granularity() {
    assert_eq!(chipset_variant(0x1920_00a1), "ad102");
    assert_eq!(chipset_variant(0x1930_00a1), "ad103");
    assert_eq!(chipset_variant(0x1940_00a1), "ad104");
    assert_eq!(chipset_variant(0x1960_00a1), "ad106");
    assert_eq!(chipset_variant(0x1970_00a1), "ad107");
}

#[test]
fn chipset_variant_blackwell() {
    assert_eq!(chipset_variant(0x1B20_00A1), "gb202");
    assert_eq!(chipset_variant(0x1B30_00A1), "gb203");
    assert_eq!(chipset_variant(0x1B50_00A1), "gb205");
    assert_eq!(chipset_variant(0x1A00_00A1), "gb100");
}

#[test]
fn chipset_variant_hopper() {
    assert_eq!(chipset_variant(0x1800_00a1), "gh100");
}

#[test]
fn sm_to_compute_class_mappings() {
    assert_eq!(sm_to_compute_class(35), 0xA1C0);
    assert_eq!(sm_to_compute_class(37), 0xA1C0);
    assert_eq!(sm_to_compute_class(50), 0xB0C0);
    assert_eq!(sm_to_compute_class(60), 0xC0C0);
    assert_eq!(sm_to_compute_class(70), 0xC3C0);
    assert_eq!(sm_to_compute_class(75), 0xC5C0);
    assert_eq!(sm_to_compute_class(80), 0xC6C0);
    assert_eq!(sm_to_compute_class(86), 0xC6C0);
    assert_eq!(sm_to_compute_class(89), 0xC6C0);
    assert_eq!(sm_to_compute_class(90), 0xC7C0);
    assert_eq!(sm_to_compute_class(100), 0xC8C0);
    assert_eq!(sm_to_compute_class(120), 0xC8C0);
}

#[test]
fn nvidia_sm_kepler_k80() {
    let k80 = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x102D,
        sysfs_path: String::new(),
    };
    assert_eq!(k80.nvidia_sm(), Some(37));
}

#[test]
fn boot0_kepler_gk110() {
    assert_eq!(boot0_to_sm(0x0F00_00A1), Some(35));
    assert_eq!(boot0_to_sm(0x0F10_00A1), Some(35));
}

#[test]
fn chipset_variant_kepler() {
    assert_eq!(chipset_variant(0x0F00_00A1), "gk110");
    assert_eq!(chipset_variant(0x0F10_00A1), "gk110b");
}

#[test]
fn chip_name_sm30_default_fallback() {
    assert_eq!(chip_name(30), "gv100");
}

#[test]
fn chip_name_sm35_kepler() {
    assert_eq!(chip_name(35), "gk210");
    assert_eq!(chip_name(37), "gk210");
}

#[test]
fn chip_name_sm50_maxwell() {
    assert_eq!(chip_name(50), "gm200");
    assert_eq!(chip_name(51), "gm200");
    assert_eq!(chip_name(52), "gm200");
}

#[test]
fn chip_name_sm60_pascal() {
    assert_eq!(chip_name(60), "gp100");
    assert_eq!(chip_name(61), "gp100");
    assert_eq!(chip_name(62), "gp100");
}

#[test]
fn chip_name_sm70_volta() {
    assert_eq!(chip_name(70), "gv100");
}

#[test]
fn chip_name_sm75_turing() {
    assert_eq!(chip_name(75), "tu102");
}

#[test]
fn chip_name_sm80_ampere_ga100() {
    assert_eq!(chip_name(80), "ga100");
}

#[test]
fn chip_name_sm86_ampere_ga102() {
    assert_eq!(chip_name(86), "ga102");
    assert_eq!(chip_name(87), "ga102");
}

#[test]
fn chip_name_sm89_ada_lovelace() {
    assert_eq!(chip_name(89), "ad102");
}

#[test]
fn chip_name_sm90_hopper() {
    assert_eq!(chip_name(90), "gh100");
}

#[test]
fn chip_name_sm100_blackwell_datacenter() {
    assert_eq!(chip_name(100), "gb100");
}

#[test]
fn chip_name_sm120_blackwell_consumer() {
    assert_eq!(chip_name(120), "gb202");
}

#[test]
fn chip_name_unknown_sm_returns_gv100() {
    assert_eq!(chip_name(0), "gv100");
    assert_eq!(chip_name(99), "gv100");
    assert_eq!(chip_name(u32::MAX), "gv100");
}

#[test]
fn boot0_to_sm_maxwell_gm200_range() {
    assert_eq!(boot0_to_sm(0x1200_00A1), Some(50));
}

#[test]
fn boot0_to_sm_pascal_gp100_range() {
    assert_eq!(boot0_to_sm(0x1300_00A1), Some(60));
}

#[test]
fn boot0_to_sm_turing_tu104_tu106() {
    assert_eq!(boot0_to_sm(0x1660_00A1), Some(75));
    assert_eq!(boot0_to_sm(0x1670_00A1), Some(75));
}

#[test]
fn boot0_to_sm_ampere_ga107_top_of_range() {
    assert_eq!(boot0_to_sm(0x1770_00A1), Some(86));
}

#[test]
fn boot0_to_sm_blackwell_gb102_variant() {
    assert_eq!(boot0_to_sm(0x1A20_00A1), Some(100));
}

#[test]
fn boot0_to_sm_blackwell_gb206_gb207() {
    assert_eq!(boot0_to_sm(0x1B60_00A1), Some(120));
    assert_eq!(boot0_to_sm(0x1B70_00A1), Some(120));
}

#[test]
fn chipset_variant_unknown_chip_id() {
    assert_eq!(chipset_variant(0x9990_00A1), "unknown");
}

#[test]
fn chipset_variant_tu116_and_ga103() {
    assert_eq!(chipset_variant(0x1680_00A1), "tu116");
    assert_eq!(chipset_variant(0x1730_00A1), "ga103");
}

#[test]
fn sm_to_compute_class_volta_upper_bound_inclusive() {
    assert_eq!(sm_to_compute_class(74), 0xC3C0);
}

#[test]
fn sm_to_compute_class_ampere_non_multiple_of_ten() {
    assert_eq!(sm_to_compute_class(81), 0xC6C0);
    assert_eq!(sm_to_compute_class(88), 0xC6C0);
}

#[test]
fn sm_to_compute_class_blackwell_above_120() {
    assert_eq!(sm_to_compute_class(121), 0xC8C0);
}

#[test]
fn nvidia_sm_hopper_device_in_range() {
    let h100 = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x2321,
        sysfs_path: String::new(),
    };
    assert_eq!(h100.nvidia_sm(), Some(90));
    let h200 = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x233A,
        sysfs_path: String::new(),
    };
    assert_eq!(h200.nvidia_sm(), Some(90));
}

#[test]
fn nvidia_sm_blackwell_consumer_range() {
    let rtx = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x2900,
        sysfs_path: String::new(),
    };
    assert_eq!(rtx.nvidia_sm(), Some(120));
    let hi = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x2999,
        sysfs_path: String::new(),
    };
    assert_eq!(hi.nvidia_sm(), Some(120));
}

#[test]
fn nvidia_sm_blackwell_outside_range_is_none() {
    let oob = GpuIdentity {
        vendor_id: PCI_VENDOR_NVIDIA,
        device_id: 0x29A0,
        sysfs_path: String::new(),
    };
    assert_eq!(oob.nvidia_sm(), None);
}

#[test]
fn amd_arch_gfx9_vega_range() {
    let v = GpuIdentity {
        vendor_id: PCI_VENDOR_AMD,
        device_id: 0x687F,
        sysfs_path: String::new(),
    };
    assert_eq!(v.amd_arch(), Some("gfx9"));
}

#[test]
fn amd_arch_rdna1_navi10() {
    let navi = GpuIdentity {
        vendor_id: PCI_VENDOR_AMD,
        device_id: 0x7310,
        sysfs_path: String::new(),
    };
    assert_eq!(navi.amd_arch(), Some("rdna1"));
}

#[test]
fn amd_arch_unknown_device_returns_none() {
    let u = GpuIdentity {
        vendor_id: PCI_VENDOR_AMD,
        device_id: 0x0001,
        sysfs_path: String::new(),
    };
    assert_eq!(u.amd_arch(), None);
}

#[test]
fn firmware_inventory_compute_blockers_gr_missing_lists_gr_only() {
    let inv = FirmwareInventory {
        chip: "x".into(),
        acr: FwStatus::Present,
        gr: FwStatus::Missing,
        sec2: FwStatus::Present,
        nvdec: FwStatus::Present,
        pmu: FwStatus::Present,
        gsp: FwStatus::Present,
    };
    let b = inv.compute_blockers();
    assert_eq!(b.len(), 1);
    assert!(b[0].contains("GR"));
}

#[test]
fn firmware_inventory_compute_blockers_pmu_gsp_missing_lists_init_firmware() {
    let inv = FirmwareInventory {
        chip: "x".into(),
        acr: FwStatus::Present,
        gr: FwStatus::Present,
        sec2: FwStatus::Present,
        nvdec: FwStatus::Present,
        pmu: FwStatus::Missing,
        gsp: FwStatus::Missing,
    };
    let b = inv.compute_blockers();
    assert_eq!(b.len(), 1);
    assert!(b[0].contains("PMU"));
}

#[test]
fn pci_vendor_intel_constant() {
    assert_eq!(PCI_VENDOR_INTEL, 0x8086);
}
