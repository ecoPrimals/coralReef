// SPDX-License-Identifier: AGPL-3.0-or-later
//! Thin facade — Kepler FECS POST-done path and sovereign cold boot orchestration.

mod boot_protocol;
mod firmware;
mod firmware_upload;
mod gr_precursor;
mod load_boot;
mod post_done;
mod reg_access;

pub(super) fn kepler_load_and_boot_fecs(
    guard: &super::hardware_guard::GuardedBar<'_>,
    cached_gpc_count: u32,
    cached_tpc_total: u32,
    cached_tpc_counts: &[(u32, u32); 8],
) {
    load_boot::kepler_load_and_boot_fecs(
        guard,
        cached_gpc_count,
        cached_tpc_total,
        cached_tpc_counts,
    );
}

#[expect(dead_code, reason = "WIP: hotspring Kepler boot strategies")]
pub(super) fn kepler_post_done_boot_fecs(
    guard: &super::hardware_guard::GuardedBar<'_>,
    cached_gpc_count: u32,
    cached_tpc_total: u32,
    cached_tpc_counts: &[(u32, u32); 8],
) {
    post_done::kepler_post_done_boot_fecs(
        guard,
        cached_gpc_count,
        cached_tpc_total,
        cached_tpc_counts,
    );
}
