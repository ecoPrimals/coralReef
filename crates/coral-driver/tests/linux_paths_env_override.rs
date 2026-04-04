// SPDX-License-Identifier: AGPL-3.0-only
//! `CORALREEF_SYSFS_ROOT` / `CORALREEF_PROC_ROOT` are captured once per process; keep a single
//! test in this binary so the override is observed before the first read.

use coral_driver::linux_paths::{
    proc_cmdline, proc_pid_fd_dir, proc_root, proc_self_fd, sysfs_class_drm_device, sysfs_join,
    sysfs_kernel_iommu_group_devices, sysfs_pci_device_file, sysfs_pci_devices, sysfs_root,
};

#[test]
fn coralreef_sysfs_proc_env_override_applies_before_first_read() {
    // SAFETY: integration test binary; env is set before any other test code reads these vars.
    unsafe {
        std::env::set_var("CORALREEF_SYSFS_ROOT", "/tmp/coral-test-sys");
        std::env::set_var("CORALREEF_PROC_ROOT", "/tmp/coral-test-proc");
    }

    assert_eq!(sysfs_root(), "/tmp/coral-test-sys");
    assert_eq!(proc_root(), "/tmp/coral-test-proc");

    let joined = sysfs_join(&["bus", "pci", "devices"]);
    assert_eq!(joined, "/tmp/coral-test-sys/bus/pci/devices");
    assert_eq!(sysfs_pci_devices(), joined);

    let drm = sysfs_class_drm_device("renderD128");
    assert!(drm.starts_with("/tmp/coral-test-sys/"));
    assert!(drm.ends_with("/class/drm/renderD128/device"));

    assert_eq!(
        sysfs_pci_device_file("0000:01:00.0", "vfio-dev"),
        "/tmp/coral-test-sys/bus/pci/devices/0000:01:00.0/vfio-dev"
    );

    assert_eq!(proc_pid_fd_dir(42), "/tmp/coral-test-proc/42/fd");
    assert_eq!(proc_self_fd(3), "/tmp/coral-test-proc/self/fd/3");
    assert_eq!(proc_cmdline(), "/tmp/coral-test-proc/cmdline");

    let iommu = sysfs_kernel_iommu_group_devices(7);
    assert_eq!(iommu, "/tmp/coral-test-sys/kernel/iommu_groups/7/devices");
}
