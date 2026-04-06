// SPDX-License-Identifier: AGPL-3.0-or-later

//! Default `/sys` and `/proc` roots. Single `#[test]` avoids `OnceLock` races when
//! parallel test threads set or read `CORALREEF_*` env vars.

use coral_driver::linux_paths;

#[test]
fn default_paths_and_helpers() {
    // SAFETY: integration test; env mutation is single-threaded before `linux_paths` init.
    unsafe {
        std::env::remove_var("CORALREEF_SYSFS_ROOT");
        std::env::remove_var("CORALREEF_PROC_ROOT");
    }

    assert_eq!(linux_paths::sysfs_root(), "/sys");
    assert_eq!(linux_paths::proc_root(), "/proc");

    assert_eq!(
        linux_paths::sysfs_join(&["bus", "pci", "devices"]),
        "/sys/bus/pci/devices"
    );
    assert_eq!(linux_paths::sysfs_join(&["//bus//", "pci"]), "/sys/bus/pci");

    let bdf = "0000:03:00.0";
    assert_eq!(linux_paths::sysfs_pci_devices(), "/sys/bus/pci/devices");
    assert_eq!(
        linux_paths::sysfs_pci_device_path(bdf),
        "/sys/bus/pci/devices/0000:03:00.0"
    );
    assert_eq!(
        linux_paths::sysfs_pci_device_file(bdf, "config"),
        "/sys/bus/pci/devices/0000:03:00.0/config"
    );
    assert_eq!(
        linux_paths::sysfs_pci_device_file(bdf, "/power/control"),
        "/sys/bus/pci/devices/0000:03:00.0/power/control"
    );
    assert_eq!(
        linux_paths::sysfs_pci_device_file(bdf, ""),
        "/sys/bus/pci/devices/0000:03:00.0"
    );
    assert_eq!(
        linux_paths::sysfs_pci_driver_bind("vfio-pci"),
        "/sys/bus/pci/drivers/vfio-pci/bind"
    );
    assert_eq!(
        linux_paths::sysfs_pci_driver_unbind("nouveau"),
        "/sys/bus/pci/drivers/nouveau/unbind"
    );
    assert_eq!(linux_paths::sysfs_pci_bus_rescan(), "/sys/bus/pci/rescan");
    assert_eq!(
        linux_paths::sysfs_module_path("nvidia"),
        "/sys/module/nvidia"
    );
    assert_eq!(
        linux_paths::sysfs_class_drm_device("renderD128"),
        "/sys/class/drm/renderD128/device"
    );
    assert_eq!(
        linux_paths::sysfs_kernel_iommu_group_devices(42),
        "/sys/kernel/iommu_groups/42/devices"
    );

    assert_eq!(linux_paths::proc_pid_fd_dir(1234), "/proc/1234/fd");
    assert_eq!(linux_paths::proc_self_fd(7), "/proc/self/fd/7");
    assert_eq!(linux_paths::proc_cmdline(), "/proc/cmdline");
}
