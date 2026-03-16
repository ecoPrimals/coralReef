// SPDX-License-Identifier: AGPL-3.0-only
//! GV100 (Volta) HBM2 register constants.

use super::types::{FbpaOffset, LtcOffset, PfbOffset};

/// Volta-specific FBPA/LTC/PFB register constants.
pub mod volta_hbm2 {
    use super::*;

    pub const FBPA0_BASE: usize = 0x009A_0000;
    pub const FBPA_STRIDE: usize = 0x0000_4000;
    pub const FBPA_COUNT: usize = 4;

    pub const LTC_BASE: usize = 0x0017_E000;
    pub const LTC_STRIDE: usize = 0x0000_2000;
    pub const LTC_COUNT: usize = 6;

    pub const PFB_BASE: usize = 0x0010_0000;
    pub const PFB_CFG0: PfbOffset = PfbOffset(0x0010_0000);
    pub const PFB_CFG1: PfbOffset = PfbOffset(0x0010_0004);
    pub const PFB_MEM_STATUS: PfbOffset = PfbOffset(0x0010_0800);
    pub const PFB_MEM_CTRL: PfbOffset = PfbOffset(0x0010_0804);
    pub const PFB_NISO_FLUSH_LO: PfbOffset = PfbOffset(0x0010_0B20);
    pub const PFB_NISO_FLUSH_HI: PfbOffset = PfbOffset(0x0010_0B24);

    pub const PCLOCK_BASE: usize = 0x0013_7000;
    pub const CLK_BASE: usize = 0x0013_2000;

    pub const PMC_ENABLE: usize = 0x0000_0200;
    pub const FB_ENABLE_BIT: u32 = 1 << 20;
    pub const LTC_ENABLE_BIT: u32 = 1 << 21;

    pub const PRAMIN_BASE: usize = 0x0070_0000;
    pub const BAR0_WINDOW: usize = 0x0000_1700;

    /// Relative offsets within each FBPA partition for key registers.
    pub const FBPA_CMD: FbpaOffset = FbpaOffset(0x00);
    pub const FBPA_CFG: FbpaOffset = FbpaOffset(0x04);
    pub const FBPA_TIMING0: FbpaOffset = FbpaOffset(0x80);
    pub const FBPA_TIMING1: FbpaOffset = FbpaOffset(0x84);
    pub const FBPA_TIMING2: FbpaOffset = FbpaOffset(0x88);

    /// Compute the absolute BAR0 offset for a register within a specific FBPA partition.
    pub fn fbpa_reg(partition: usize, rel: FbpaOffset) -> usize {
        FBPA0_BASE + partition * FBPA_STRIDE + rel.0
    }

    /// Compute the absolute BAR0 offset for a register within a specific LTC partition.
    pub fn ltc_reg(partition: usize, rel: LtcOffset) -> usize {
        LTC_BASE + partition * LTC_STRIDE + rel.0
    }
}
