// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(unsafe_code)]
//! `CORALREEF_GROUP_FILE` override for [`coral_glowplug::group_unix`].

#[cfg(unix)]
use std::sync::Mutex;

#[cfg(unix)]
static ENV_LOCK: Mutex<()> = Mutex::new(());

#[cfg(unix)]
#[test]
fn gid_for_group_name_respects_coralreef_group_file_env() {
    let _guard = ENV_LOCK.lock().expect("env test lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("group");
    std::fs::write(&path, "coralreef_itest_grp:x:424242:\n").expect("write group file");
    let path_str = path.to_str().expect("utf8 path");
    let prev = std::env::var("CORALREEF_GROUP_FILE").ok();
    // SAFETY: serialized by `ENV_LOCK`; no concurrent env access in this process.
    unsafe {
        std::env::set_var("CORALREEF_GROUP_FILE", path_str);
    }
    let gid = coral_glowplug::group_unix::gid_for_group_name("coralreef_itest_grp");
    // SAFETY: same as `set_var` above.
    unsafe {
        match &prev {
            Some(v) => std::env::set_var("CORALREEF_GROUP_FILE", v),
            None => std::env::remove_var("CORALREEF_GROUP_FILE"),
        }
    }
    assert_eq!(gid, Some(424_242));
}
