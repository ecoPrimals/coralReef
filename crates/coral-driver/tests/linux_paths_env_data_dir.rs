// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(unsafe_code)]

//! `CORALREEF_DATA_DIR` for optional data paths.

use coral_driver::linux_paths;

#[test]
fn optional_data_dir_reads_coralreef_env() {
    // SAFETY: integration test binary; env mutation before reads.
    unsafe {
        std::env::remove_var("CORALREEF_DATA_DIR");
    }
    assert!(linux_paths::optional_data_dir().is_none());

    // SAFETY: integration test binary; env mutation before reads.
    unsafe {
        std::env::set_var("CORALREEF_DATA_DIR", "/coral-data");
    }
    assert_eq!(
        linux_paths::optional_data_dir().as_deref(),
        Some("/coral-data")
    );

    // SAFETY: integration test binary; restore env for other tests.
    unsafe {
        std::env::remove_var("CORALREEF_DATA_DIR");
    }
}
