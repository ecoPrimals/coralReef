// SPDX-License-Identifier: AGPL-3.0-only

//! Trailing-slash trimming for sysfs root (separate test binary).

use coral_driver::linux_paths;

#[test]
fn sysfs_root_trims_trailing_slashes() {
    // SAFETY: integration test; env mutation is single-threaded before `linux_paths` init.
    unsafe {
        std::env::set_var("CORALREEF_SYSFS_ROOT", "/tmp/sys///");
    }
    assert_eq!(linux_paths::sysfs_root(), "/tmp/sys");
    assert_eq!(
        linux_paths::sysfs_pci_bus_rescan(),
        "/tmp/sys/bus/pci/rescan"
    );
}
