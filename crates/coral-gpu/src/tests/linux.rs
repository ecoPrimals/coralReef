// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! Linux-only driver helpers, PCIe topology, and opt-in hardware integration tests.

use crate::GpuContext;

#[cfg(target_os = "linux")]
mod linux_tests {
    use crate::GpuContext;
    use crate::driver::{self, sm_to_nvarch};
    use crate::pcie::{PcieDeviceInfo, assign_switch_groups};
    use coral_reef::{AmdArch, GpuTarget, NvArch};

    #[test]
    fn sm_to_nvarch_maps_known_sm_versions() {
        assert_eq!(sm_to_nvarch(75), NvArch::Sm75);
        assert_eq!(sm_to_nvarch(80), NvArch::Sm80);
        assert_eq!(sm_to_nvarch(86), NvArch::Sm86);
        assert_eq!(sm_to_nvarch(89), NvArch::Sm89);
        assert_eq!(sm_to_nvarch(99), NvArch::Sm70, "unknown SM maps to Sm70");
    }

    #[test]
    fn sm_to_nvarch_round_trips_nvarch_sm() {
        for sm in [70_u32, 75, 80, 86, 89] {
            let arch = sm_to_nvarch(sm);
            assert_eq!(arch.sm(), sm, "roundtrip for SM {sm}");
        }
        assert_eq!(sm_to_nvarch(71).sm(), 70);
    }

    #[test]
    fn auto_with_empty_preference_errors_without_matching_driver() {
        let pref = crate::preference::DriverPreference::from_str_list("");
        match GpuContext::auto_with_preference(&pref) {
            Err(crate::GpuError::NoDevice(msg)) => {
                assert!(
                    msg.contains("no GPU devices found")
                        || msg.contains("no preferred driver found"),
                    "unexpected message: {msg}"
                );
            }
            Ok(_) => panic!("expected error when preference list is empty"),
            Err(e) => panic!("expected NoDevice, got {e:?}"),
        }
    }

    #[test]
    fn default_nv_sm_constants_documented() {
        assert_eq!(driver::DEFAULT_NV_SM, 86);
        assert_eq!(driver::DEFAULT_NV_SM_NOUVEAU, 70);
    }

    #[test]
    fn assign_switch_groups_groups_by_pci_prefix() {
        let mut devices = vec![
            PcieDeviceInfo {
                render_node: "/dev/dri/renderD128".into(),
                pcie_address: Some("0000:03:00.0".into()),
                switch_group: None,
                target: GpuTarget::Amd(AmdArch::Rdna2),
            },
            PcieDeviceInfo {
                render_node: "/dev/dri/renderD129".into(),
                pcie_address: Some("0000:03:00.1".into()),
                switch_group: None,
                target: GpuTarget::Amd(AmdArch::Rdna2),
            },
            PcieDeviceInfo {
                render_node: "/dev/dri/renderD130".into(),
                pcie_address: Some("0000:09:00.0".into()),
                switch_group: None,
                target: GpuTarget::Amd(AmdArch::Rdna2),
            },
        ];
        assign_switch_groups(&mut devices);
        assert_eq!(devices[0].switch_group, devices[1].switch_group);
        assert_ne!(devices[0].switch_group, devices[2].switch_group);
    }

    #[test]
    fn assign_switch_groups_leaves_none_without_address() {
        let mut devices = vec![PcieDeviceInfo {
            render_node: "/dev/dri/renderD128".into(),
            pcie_address: None,
            switch_group: None,
            target: GpuTarget::Amd(AmdArch::Rdna2),
        }];
        assign_switch_groups(&mut devices);
        assert_eq!(devices[0].switch_group, None);
    }

    #[test]
    fn assign_switch_groups_single_address_gets_group_zero() {
        let mut devices = vec![PcieDeviceInfo {
            render_node: "/dev/dri/renderD130".into(),
            pcie_address: Some("0000:65:00.0".into()),
            switch_group: None,
            target: GpuTarget::Nvidia(NvArch::Sm86),
        }];
        assign_switch_groups(&mut devices);
        assert_eq!(devices[0].switch_group, Some(0));
    }

    #[test]
    fn assign_switch_groups_four_ports_same_bus_prefix_share_group() {
        let mut devices = vec![
            PcieDeviceInfo {
                render_node: "/dev/dri/renderD128".into(),
                pcie_address: Some("0000:1a:00.0".into()),
                switch_group: None,
                target: GpuTarget::Amd(AmdArch::Rdna2),
            },
            PcieDeviceInfo {
                render_node: "/dev/dri/renderD129".into(),
                pcie_address: Some("0000:1a:00.1".into()),
                switch_group: None,
                target: GpuTarget::Amd(AmdArch::Rdna2),
            },
            PcieDeviceInfo {
                render_node: "/dev/dri/renderD130".into(),
                pcie_address: Some("0000:1a:00.2".into()),
                switch_group: None,
                target: GpuTarget::Amd(AmdArch::Rdna2),
            },
            PcieDeviceInfo {
                render_node: "/dev/dri/renderD131".into(),
                pcie_address: Some("0000:1a:00.3".into()),
                switch_group: None,
                target: GpuTarget::Amd(AmdArch::Rdna2),
            },
        ];
        assign_switch_groups(&mut devices);
        let g = devices[0].switch_group;
        assert!(g.is_some());
        assert_eq!(devices[1].switch_group, g);
        assert_eq!(devices[2].switch_group, g);
        assert_eq!(devices[3].switch_group, g);
    }

    #[test]
    fn probe_pcie_topology_smoke() {
        let list = crate::probe_pcie_topology();
        for d in &list {
            assert!(d.render_node.starts_with("/dev/dri/renderD"));
        }
    }

    #[cfg(feature = "vfio")]
    mod vfio_driver_tests {
        use crate::driver::{sm_to_compute_class, vfio_sm_from_device_id};
        use coral_driver::nv::pushbuf::class::{
            AMPERE_COMPUTE_A, TURING_COMPUTE_A, VOLTA_COMPUTE_A,
        };

        #[test]
        fn sm_to_compute_class_matches_identity_table() {
            use coral_driver::nv::identity::sm_to_compute_class as identity_class;

            for sm in [35_u32, 50, 60, 70, 74, 75, 79, 80, 86, 89, 90, 100, 120] {
                assert_eq!(sm_to_compute_class(sm), identity_class(sm), "SM {sm}");
            }
            assert_eq!(sm_to_compute_class(70), VOLTA_COMPUTE_A);
            assert_eq!(sm_to_compute_class(75), TURING_COMPUTE_A);
            assert_eq!(sm_to_compute_class(80), AMPERE_COMPUTE_A);
        }

        #[test]
        fn vfio_sm_from_device_id_maps_pci_ids() {
            assert_eq!(vfio_sm_from_device_id(Some(0x1D81)), 70);
            assert_eq!(vfio_sm_from_device_id(Some(0x1E00)), 75);
            assert_eq!(vfio_sm_from_device_id(Some(0x1E8F)), 75);
            assert_eq!(vfio_sm_from_device_id(Some(0x2200)), 80);
            assert_eq!(vfio_sm_from_device_id(Some(0x2203)), 80);
            assert_eq!(vfio_sm_from_device_id(Some(0x2207)), 80);
            assert_eq!(vfio_sm_from_device_id(Some(0x22FF)), 80);
            assert_eq!(vfio_sm_from_device_id(Some(0x2204)), 86);
            assert_eq!(vfio_sm_from_device_id(Some(0x2206)), 86);
            assert_eq!(vfio_sm_from_device_id(Some(0x2300)), 86);
            assert_eq!(vfio_sm_from_device_id(Some(0x23FF)), 86);
            assert_eq!(vfio_sm_from_device_id(Some(0x2400)), 89);
            assert_eq!(vfio_sm_from_device_id(Some(0x26FF)), 89);
        }

        #[test]
        fn vfio_sm_from_device_id_unknown_falls_back_to_default_nv_sm() {
            assert_eq!(vfio_sm_from_device_id(None), crate::driver::DEFAULT_NV_SM);
            assert_eq!(
                vfio_sm_from_device_id(Some(0xFFFF)),
                crate::driver::DEFAULT_NV_SM
            );
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires DRM render nodes and driver stack on the host"]
fn gpu_context_auto_integration() {
    let _ = GpuContext::auto();
}

#[cfg(all(target_os = "linux", feature = "vfio"))]
#[test]
#[ignore = "requires sysfs VFIO NVIDIA binding"]
fn discover_vfio_nvidia_bdf_integration() {
    let _ = crate::driver::discover_vfio_nvidia_bdf();
}

#[cfg(target_os = "linux")]
mod descriptor_and_open_driver_tests {
    use crate::GpuContext;
    use crate::error::GpuError;
    use crate::preference;

    #[test]
    fn from_descriptor_rejects_unknown_vendor() {
        let err = GpuContext::from_descriptor("acme", None, None)
            .err()
            .unwrap();
        let GpuError::NoDevice(msg) = err else {
            panic!("expected NoDevice, got {err:?}");
        };
        assert!(
            msg.contains("unsupported vendor/driver"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn from_descriptor_rejects_amd_with_wrong_driver_name() {
        let err =
            GpuContext::from_descriptor("amd", Some("rdna2"), Some(preference::DRIVER_NVIDIA_DRM))
                .err()
                .unwrap();
        let GpuError::NoDevice(msg) = err else {
            panic!("expected NoDevice, got {err:?}");
        };
        assert!(msg.contains("unsupported vendor/driver"), "{msg}");
    }

    #[test]
    fn open_driver_rejects_unknown_backend() {
        let err = GpuContext::open_driver("not-a-real-driver").err().unwrap();
        let GpuError::NoDevice(msg) = err else {
            panic!("expected NoDevice, got {err:?}");
        };
        assert!(msg.contains("unsupported driver"), "{msg}");
    }

    #[cfg(feature = "vfio")]
    #[test]
    fn from_descriptor_vfio_requires_bdf_as_render_node() {
        let err =
            GpuContext::from_descriptor("nvidia", Some("sm86"), Some(preference::DRIVER_VFIO))
                .err()
                .unwrap();
        let GpuError::NoDevice(msg) = err else {
            panic!("expected NoDevice, got {err:?}");
        };
        assert!(msg.contains("VFIO requires a BDF"), "{msg}");
    }
}
