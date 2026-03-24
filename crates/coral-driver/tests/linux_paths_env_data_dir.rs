// SPDX-License-Identifier: AGPL-3.0-only

//! `CORALREEF_DATA_DIR` / `HOTSPRING_DATA_DIR` for optional data paths.

use coral_driver::linux_paths;

#[test]
fn optional_data_dir_prefers_coralreef_then_legacy() {
    // SAFETY: integration test binary; env mutation before reads.
    unsafe {
        std::env::remove_var("CORALREEF_DATA_DIR");
        std::env::remove_var("HOTSPRING_DATA_DIR");
    }
    assert!(linux_paths::optional_data_dir().is_none());

    unsafe {
        std::env::set_var("HOTSPRING_DATA_DIR", "/legacy-data");
    }
    assert_eq!(
        linux_paths::optional_data_dir().as_deref(),
        Some("/legacy-data")
    );

    unsafe {
        std::env::set_var("CORALREEF_DATA_DIR", "/coral-data");
    }
    assert_eq!(
        linux_paths::optional_data_dir().as_deref(),
        Some("/coral-data")
    );
}
