// SPDX-License-Identifier: AGPL-3.0-only
//! Thread-isolated, timeout-protected VFIO device open/close.
//!
//! Prevents D-state hangs from blocking the main ember event loop by
//! moving VFIO open/close into a sacrificial thread with a hard timeout.

use std::time::Duration;

/// Maximum time to wait for a VFIO device open before declaring it stuck.
pub const GUARDED_OPEN_TIMEOUT: Duration = Duration::from_secs(30);

/// Close a VFIO device in a sacrificial thread so a D-state hang
/// during close doesn't block the caller.
pub fn guarded_vfio_close(device: coral_driver::vfio::VfioDevice, bdf: &str) {
    let bdf_owned = bdf.to_string();
    std::thread::Builder::new()
        .name(format!("ember-close-{bdf_owned}"))
        .spawn(move || {
            tracing::debug!(bdf = %bdf_owned, "guarded close: dropping VFIO device");
            drop(device);
            tracing::debug!(bdf = %bdf_owned, "guarded close: done");
        })
        .ok();
}
