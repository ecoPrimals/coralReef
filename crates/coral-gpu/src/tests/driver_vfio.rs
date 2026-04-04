// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! VFIO sysfs-free helpers (`vfio_sm_from_device_id`, `sm_to_compute_class`) when `vfio` is enabled.

#[cfg(all(target_os = "linux", feature = "vfio"))]
mod vfio_mapping {
    use coral_driver::nv::pushbuf::class::{AMPERE_COMPUTE_A, TURING_COMPUTE_A, VOLTA_COMPUTE_A};

    use crate::driver::{sm_to_compute_class, vfio_sm_from_device_id};

    #[test]
    fn sm_to_compute_class_volta_turing_ampere() {
        assert_eq!(sm_to_compute_class(70), VOLTA_COMPUTE_A);
        assert_eq!(sm_to_compute_class(74), VOLTA_COMPUTE_A);
        assert_eq!(sm_to_compute_class(75), TURING_COMPUTE_A);
        assert_eq!(sm_to_compute_class(79), TURING_COMPUTE_A);
        assert_eq!(sm_to_compute_class(80), AMPERE_COMPUTE_A);
        // SM 100+ uses the identity-table fallback (`identity::sm_to_compute_class`), not Ampere.
        assert_eq!(sm_to_compute_class(120), 0xC8C0);
    }

    #[test]
    fn vfio_sm_from_device_id_maps_known_pci_ids() {
        assert_eq!(vfio_sm_from_device_id(Some(0x1003)), 35);
        assert_eq!(vfio_sm_from_device_id(Some(0x1024)), 35);
        assert_eq!(vfio_sm_from_device_id(Some(0x1D81)), 70);
        assert_eq!(vfio_sm_from_device_id(Some(0x1E00)), 75);
        assert_eq!(vfio_sm_from_device_id(Some(0x1E8F)), 75);
        assert_eq!(vfio_sm_from_device_id(Some(0x2200)), 80);
        assert_eq!(vfio_sm_from_device_id(Some(0x2203)), 80);
        assert_eq!(vfio_sm_from_device_id(Some(0x2207)), 80);
        assert_eq!(vfio_sm_from_device_id(Some(0x2204)), 86);
        assert_eq!(vfio_sm_from_device_id(Some(0x2206)), 86);
        assert_eq!(vfio_sm_from_device_id(Some(0x2300)), 86);
        assert_eq!(vfio_sm_from_device_id(Some(0x23FF)), 86);
        assert_eq!(vfio_sm_from_device_id(Some(0x2400)), 89);
        assert_eq!(vfio_sm_from_device_id(Some(0x2900)), 120);
    }

    #[test]
    fn vfio_sm_from_device_id_unknown_matches_none_fallback() {
        let fallback = vfio_sm_from_device_id(None);
        assert_eq!(vfio_sm_from_device_id(Some(0xFFFF)), fallback);
    }
}
