// SPDX-License-Identifier: AGPL-3.0-only
//! [`coral_driver::nv::identity::probe_gpu_identity`] against a fake sysfs tree (single test per
//! binary so `CORALREEF_SYSFS_ROOT` applies before [`coral_driver::linux_paths::sysfs_root`] locks).

use std::fs;

use coral_driver::nv::identity::{PCI_VENDOR_NVIDIA, probe_gpu_identity};

#[test]
fn probe_gpu_identity_fake_sysfs_success_and_parse_error() {
    let root = tempfile::tempdir().expect("tempdir");
    // SAFETY: this test binary only; set env before first `sysfs_root` read.
    unsafe {
        std::env::set_var(
            "CORALREEF_SYSFS_ROOT",
            root.path().to_str().expect("utf8 path"),
        );
    }

    let ok_node = "renderD204";
    let ok_device = root.path().join("class/drm").join(ok_node).join("device");
    fs::create_dir_all(&ok_device).expect("mkdir");
    fs::write(ok_device.join("vendor"), "0x10de\n").expect("vendor");
    fs::write(ok_device.join("device"), "0x1d81\n").expect("device");

    let gpu = probe_gpu_identity(&format!("/dev/dri/{ok_node}"))
        .expect("identity should parse from fake sysfs");

    assert_eq!(gpu.vendor_id, PCI_VENDOR_NVIDIA);
    assert_eq!(gpu.device_id, 0x1D81);
    assert!(
        gpu.sysfs_path
            .ends_with(&format!("/class/drm/{ok_node}/device"))
    );

    let bad_node = "renderD205";
    let bad_device = root.path().join("class/drm").join(bad_node).join("device");
    fs::create_dir_all(&bad_device).expect("mkdir");
    fs::write(bad_device.join("vendor"), "not-hex\n").expect("vendor");
    fs::write(bad_device.join("device"), "0x1d81\n").expect("device");

    assert!(probe_gpu_identity(&format!("/dev/dri/{bad_node}")).is_none());
}
