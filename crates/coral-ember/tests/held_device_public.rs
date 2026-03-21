// SPDX-License-Identifier: AGPL-3.0-only
//! Integration-level checks for [`coral_ember::HeldDevice`] public surface.
//!
//! A real [`coral_driver::vfio::VfioDevice`] is required to construct a [`coral_ember::HeldDevice`];
//! hardware-gated tests in `ipc_dispatch.rs` exercise `ember.vfio_fds` / `ember.release` end-to-end.

#[test]
fn held_device_bdf_field_is_public_for_ipc_clients() {
    #[allow(dead_code)]
    fn read_bdf(h: &coral_ember::HeldDevice) -> &str {
        &h.bdf
    }
    let _ = read_bdf as fn(&coral_ember::HeldDevice) -> &str;
}
