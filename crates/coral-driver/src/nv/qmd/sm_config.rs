// SPDX-License-Identifier: AGPL-3.0-or-later
//! Volta+ shared-memory partition encoding for SKED scheduling.

/// Compute the SM config shared memory partition size for Volta+ hardware.
///
/// Maps a shared memory byte count to the hardware encoding used by
/// `MIN/MAX/TARGET_SM_CONFIG_SHARED_MEM_SIZE`. The encoding is
/// `(partition_kb / 4) + 1`, with a minimum 8 KB partition.
///
/// Mirrors NVK's `gv100_sm_config_smem_size()`.
pub(crate) const fn gv100_sm_config_smem_size(bytes: u32) -> u64 {
    let size = if bytes > 64 * 1024 {
        96 * 1024
    } else if bytes > 32 * 1024 {
        64 * 1024
    } else if bytes > 16 * 1024 {
        32 * 1024
    } else if bytes > 8 * 1024 {
        16 * 1024
    } else {
        8 * 1024
    };
    (size / 4096 + 1) as u64
}
