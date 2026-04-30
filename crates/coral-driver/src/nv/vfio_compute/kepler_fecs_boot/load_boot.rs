// SPDX-License-Identifier: AGPL-3.0-or-later
//! Cold-path orchestration: firmware resolve → GR precursor → PIO upload → boot protocol.

use super::super::hardware_guard::GuardedBar;

pub(in crate::nv::vfio_compute) fn kepler_load_and_boot_fecs(
    guard: &GuardedBar<'_>,
    cached_gpc_count: u32,
    cached_tpc_total: u32,
    cached_tpc_counts: &[(u32, u32); 8],
) {
    let Some(blobs) = super::firmware::resolve_kepler_firmware() else {
        return;
    };

    tracing::info!(
        fecs_code = blobs.fecs_code.len(),
        fecs_data = blobs.fecs_data.len(),
        gpccs_code = blobs.gpccs_code.len(),
        gpccs_data = blobs.gpccs_data.len(),
        "Loading GK110 FECS/GPCCS firmware via PIO"
    );

    super::gr_precursor::run_gr_boot_precursor(
        guard,
        cached_gpc_count,
        cached_tpc_total,
        cached_tpc_counts,
    );

    if !super::firmware_upload::upload_kepler_firmware(guard, &blobs) {
        return;
    }

    super::boot_protocol::run_kepler_boot_protocols(guard, &blobs);
}
