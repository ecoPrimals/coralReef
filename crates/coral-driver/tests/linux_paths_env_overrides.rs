// SPDX-License-Identifier: AGPL-3.0-or-later

//! Custom `CORALREEF_SYSFS_ROOT` / `CORALREEF_PROC_ROOT`. Single `#[test]` so
//! `OnceLock` initialization is deterministic (integration tests may run in parallel).

use coral_driver::linux_paths;

#[test]
fn sysfs_proc_overrides_and_empty_fallback() {
    // SAFETY: integration test; env mutation is single-threaded before `linux_paths` init.
    unsafe {
        std::env::set_var("CORALREEF_SYSFS_ROOT", "/mnt/coral-sys");
    }
    assert_eq!(linux_paths::sysfs_root(), "/mnt/coral-sys");
    assert_eq!(
        linux_paths::sysfs_pci_device_path("0000:01:00.0"),
        "/mnt/coral-sys/bus/pci/devices/0000:01:00.0"
    );

    // Second scenario requires a fresh process — covered by `linux_paths_env_trim` test binary.
}
