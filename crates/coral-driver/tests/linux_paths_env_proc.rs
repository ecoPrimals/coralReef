// SPDX-License-Identifier: AGPL-3.0-only

//! Proc root override (separate test binary).

use coral_driver::linux_paths;

#[test]
fn proc_root_override_paths() {
    // SAFETY: integration test; env mutation is single-threaded before `linux_paths` init.
    unsafe {
        std::env::set_var("CORALREEF_PROC_ROOT", "/fake/proc");
    }
    assert_eq!(linux_paths::proc_root(), "/fake/proc");
    assert_eq!(linux_paths::proc_cmdline(), "/fake/proc/cmdline");
    assert_eq!(linux_paths::proc_pid_fd_dir(99), "/fake/proc/99/fd");
    assert_eq!(linux_paths::proc_self_fd(3), "/fake/proc/self/fd/3");
}
