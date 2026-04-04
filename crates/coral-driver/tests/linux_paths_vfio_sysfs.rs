// SPDX-License-Identifier: AGPL-3.0-only
//! [`coral_driver::linux_paths::sysfs_vfio_cdev_name`] under a fake sysfs root (single test per
//! binary so `CORALREEF_SYSFS_ROOT` wins before [`coral_driver::linux_paths::sysfs_root`] locks).

use std::fs;

use coral_driver::linux_paths::{sysfs_root, sysfs_vfio_cdev_name};

#[test]
fn sysfs_vfio_cdev_name_variants_under_fake_root() {
    let root = tempfile::tempdir().expect("tempdir");
    // SAFETY: this test binary only; set env before first `sysfs_root` read.
    unsafe {
        std::env::set_var(
            "CORALREEF_SYSFS_ROOT",
            root.path().to_str().expect("utf8 path"),
        );
    }

    assert_eq!(
        sysfs_root(),
        root.path().to_str().expect("utf8"),
        "test must run before any other code in this process called sysfs_root()"
    );

    let ok_bdf = "0000:03:00.0";
    let vfio_ok = root
        .path()
        .join("bus/pci/devices")
        .join(ok_bdf)
        .join("vfio-dev");
    fs::create_dir_all(&vfio_ok).expect("mkdir");
    fs::write(vfio_ok.join("vfio12"), []).expect("touch");

    let empty_bdf = "0000:04:00.0";
    let vfio_empty = root
        .path()
        .join("bus/pci/devices")
        .join(empty_bdf)
        .join("vfio-dev");
    fs::create_dir_all(&vfio_empty).expect("mkdir empty vfio-dev");

    fs::create_dir_all(root.path().join("bus/pci/devices/0000:99:00.0")).expect("bdf no vfio");

    assert_eq!(sysfs_vfio_cdev_name(ok_bdf).as_deref(), Some("vfio12"));
    assert!(sysfs_vfio_cdev_name("0000:99:00.0").is_none());
    assert!(sysfs_vfio_cdev_name(empty_bdf).is_none());
}
