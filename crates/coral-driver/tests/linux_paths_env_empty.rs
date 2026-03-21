// SPDX-License-Identifier: AGPL-3.0-only

//! Empty env strings fall back to defaults (separate test binary).

use coral_driver::linux_paths;

#[test]
fn empty_sysfs_proc_env_uses_defaults() {
    // SAFETY: integration test; env mutation is single-threaded before `linux_paths` init.
    unsafe {
        std::env::set_var("CORALREEF_SYSFS_ROOT", "");
        std::env::set_var("CORALREEF_PROC_ROOT", "");
    }
    assert_eq!(linux_paths::sysfs_root(), "/sys");
    assert_eq!(linux_paths::proc_root(), "/proc");
}
