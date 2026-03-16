// SPDX-License-Identifier: AGPL-3.0-only
//! FBPA partition snapshot for training diagnostics.

use crate::vfio::device::MappedBar;

use super::constants::volta_hbm2;
use super::types::FbpaOffset;

/// Snapshot of a single FBPA partition's key registers.
#[derive(Debug, Clone)]
pub struct FbpaSnapshot {
    pub index: usize,
    pub base: usize,
    pub cfg: u32,
    pub timing0: u32,
    pub timing1: u32,
    pub timing2: u32,
    pub alive: bool,
}

/// Snapshot all FBPA partitions.
pub fn snapshot_fbpa(bar0: &MappedBar, count: usize) -> Vec<FbpaSnapshot> {
    (0..count)
        .map(|i| {
            let base = volta_hbm2::fbpa_reg(i, FbpaOffset(0));
            let r = |off: FbpaOffset| {
                bar0.read_u32(volta_hbm2::fbpa_reg(i, off))
                    .unwrap_or(0xDEAD_DEAD)
            };
            let cfg = r(volta_hbm2::FBPA_CFG);
            let is_err = |v: u32| v == 0xFFFF_FFFF || v == 0xDEAD_DEAD || (v >> 16) == 0xBADF;
            FbpaSnapshot {
                index: i,
                base,
                cfg,
                timing0: r(volta_hbm2::FBPA_TIMING0),
                timing1: r(volta_hbm2::FBPA_TIMING1),
                timing2: r(volta_hbm2::FBPA_TIMING2),
                alive: !is_err(cfg),
            }
        })
        .collect()
}
