// SPDX-License-Identifier: AGPL-3.0-or-later

//! BAR0 register base offsets for Volta-class GPUs.

#[expect(
    dead_code,
    reason = "some register constants reserved for completeness; BAR0_WINDOW, STRAP_BASE"
)]
pub(crate) mod volta_regs {
    pub const BOOT0: usize = 0x0000_0000;
    pub const PMC_ENABLE: usize = 0x0000_0200;
    pub const PFIFO_ENABLE: usize = 0x0000_2200;
    pub const PBDMA_MAP: usize = 0x0000_2004;
    pub const BAR0_WINDOW: usize = 0x0000_1700;
    pub const BAR2_BLOCK: usize = 0x0000_1714;
    pub const PRAMIN_BASE: usize = 0x0070_0000;

    pub const PMU_BASE: usize = 0x0010_A000;
    pub const GR_BASE: usize = 0x0040_0000;
    pub const CE_BASE: usize = 0x0010_4000;
    pub const PDISP_BASE: usize = 0x0061_0000;
    pub const NVDEC_BASE: usize = 0x0008_4000;
    pub const NVENC_BASE: usize = 0x001C_8000;

    pub const FBPA0_BASE: usize = 0x009A_0000;
    pub const FBPA_STRIDE: usize = 0x0000_4000;
    pub const LTC_BASE: usize = 0x0017_E000;
    pub const LTC_STRIDE: usize = 0x0000_2000;

    pub const PCLOCK_BASE: usize = 0x0013_7000;
    pub const CLK_BASE: usize = 0x0013_2000;

    pub const THERMAL_BASE: usize = 0x0002_0400;
    pub const FUSE_BASE: usize = 0x0002_1000;
    pub const STRAP_BASE: usize = 0x0010_1000;

    pub const PFB_BASE: usize = 0x0010_0000;
    pub const PTIMER_BASE: usize = 0x0000_9000;
}
