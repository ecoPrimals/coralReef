// SPDX-License-Identifier: AGPL-3.0-or-later

//! Pure buffer layout and SEC2 poll-step helpers for system-memory ACR boot.

use crate::vfio::channel::registers::falcon;

use super::super::firmware::{AcrFirmwareSet, ParsedAcrFirmware};
use super::super::sysmem_iova;
use super::super::wpr::build_wpr;
use super::boot_config::BootConfig;

/// Planned IOVA layout and sizes for SysMem ACR DMA regions (`low_catch`, firmware,
/// WPR, shadow, and logical blob targeting).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AcrBufferLayout {
    /// Low catch-all IOVA base.
    pub low_catch_iova: u64,
    /// Low catch-all mapping size in bytes.
    pub low_catch_size: usize,
    /// ACR firmware payload (patched `ucode_load`) IOVA base.
    pub fw_iova: u64,
    /// Page-rounded ACR payload size in bytes.
    pub fw_size: usize,
    /// WPR buffer IOVA base.
    pub wpr_iova: u64,
    /// Page-rounded WPR DMA size in bytes.
    pub wpr_size: usize,
    /// Actual `build_wpr` image length (descriptor patching uses this, not page size).
    pub wpr_payload_len: usize,
    /// Shadow WPR IOVA base.
    pub shadow_iova: u64,
    /// Shadow buffer size (matches WPR page-rounded size).
    pub shadow_size: usize,
    /// Logical blob DMA target IOVA (`0` when blob DMA is skipped).
    pub blob_iova: u64,
    /// Logical blob size in bytes (`0` when skipped).
    pub blob_size: usize,
    /// Mirrors [`BootConfig::blob_size_zero`]: firmware skips internal blob DMA.
    pub skip_blob_dma: bool,
}

impl AcrBufferLayout {
    /// Builds WPR once and derives all IOVA/size fields used for DMA allocation.
    #[must_use]
    pub fn compute(
        config: &BootConfig,
        fw: &AcrFirmwareSet,
        parsed: &ParsedAcrFirmware,
    ) -> (Self, Vec<u8>) {
        let wpr_data = build_wpr(fw, sysmem_iova::WPR);
        let layout = Self::from_sizes(config, parsed.acr_payload.len(), wpr_data.len());
        (layout, wpr_data)
    }

    /// Pure layout from payload lengths (for unit tests and size checks without firmware I/O).
    #[must_use]
    pub fn from_sizes(config: &BootConfig, acr_payload_len: usize, wpr_payload_len: usize) -> Self {
        let fw_size = acr_payload_len.div_ceil(4096) * 4096;
        let wpr_size = wpr_payload_len.div_ceil(4096) * 4096;
        let skip = config.blob_size_zero;
        let (blob_iova, blob_size) = if skip {
            (0_u64, 0_usize)
        } else {
            (sysmem_iova::WPR, wpr_payload_len)
        };
        Self {
            low_catch_iova: sysmem_iova::LOW_CATCH,
            low_catch_size: sysmem_iova::LOW_CATCH_SIZE,
            fw_iova: sysmem_iova::ACR,
            fw_size: fw_size.max(4096),
            wpr_iova: sysmem_iova::WPR,
            wpr_size: wpr_size.max(4096),
            wpr_payload_len,
            shadow_iova: sysmem_iova::SHADOW,
            shadow_size: wpr_size.max(4096),
            blob_iova,
            blob_size,
            skip_blob_dma: skip,
        }
    }

    /// End IOVA of the populated WPR image (not necessarily page-aligned).
    #[must_use]
    pub fn wpr_end_iova(&self) -> u64 {
        self.wpr_iova + self.wpr_payload_len as u64
    }
}

/// Outcome of one SEC2 boot poll iteration (Pattern B: CPUCTL + mailbox + PC stability).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PollResult {
    /// Keep polling.
    Continue,
    /// Stop: mailbox non-zero, CPU halted, or held in reset.
    Respond,
    /// Stop: PC unchanged for enough consecutive samples.
    Settled,
    /// Stop: wall-clock budget exceeded.
    Timeout,
}

/// Mutable state for [`sec2_poll_check`]: PC progression sampling and settle detection.
#[derive(Debug, Clone)]
pub(super) struct Sec2PollState {
    /// Last observed program counter.
    pub last_pc: u32,
    /// Consecutive polls where `PC` equaled `last_pc`.
    pub settled_count: u32,
    /// Human-readable PC progression samples for diagnostics.
    pub pc_samples: Vec<String>,
}

impl Sec2PollState {
    /// Creates empty poll state before the first sample.
    #[must_use]
    pub fn new() -> Self {
        Self {
            last_pc: 0,
            settled_count: 0,
            pc_samples: Vec::new(),
        }
    }
}

impl Default for Sec2PollState {
    fn default() -> Self {
        Self::new()
    }
}

/// Pure poll step: updates [`Sec2PollState`] and returns whether to stop.
///
/// `elapsed_ms` and `timeout_ms` use the same unit as `std::time::Instant::elapsed().as_millis()`.
#[must_use]
pub(super) fn sec2_poll_check(
    cpuctl: u32,
    mb0: u32,
    pc: u32,
    elapsed_ms: u128,
    timeout_ms: u128,
    state: &mut Sec2PollState,
) -> PollResult {
    if pc != state.last_pc {
        state.pc_samples.push(format!("{:#06x}@{elapsed_ms}ms", pc));
        state.last_pc = pc;
        state.settled_count = 0;
    } else {
        state.settled_count = state.settled_count.saturating_add(1);
    }

    let halted = cpuctl & falcon::CPUCTL_HALTED != 0;
    let hreset_back = cpuctl & falcon::CPUCTL_HRESET != 0;

    if mb0 != 0 || halted || hreset_back {
        PollResult::Respond
    } else if state.settled_count > 200 {
        PollResult::Settled
    } else if elapsed_ms > timeout_ms {
        PollResult::Timeout
    } else {
        PollResult::Continue
    }
}

#[cfg(test)]
mod tests {
    use crate::vfio::channel::registers::falcon;

    use super::super::super::sysmem_iova;
    use super::{AcrBufferLayout, BootConfig, PollResult, Sec2PollState, sec2_poll_check};

    #[test]
    fn acr_buffer_layout_from_sizes_rounds_fw_and_wpr() {
        let config = BootConfig {
            pde_upper: true,
            acr_vram_pte: false,
            blob_size_zero: true,
            bind_vram: false,
            imem_preload: false,
            tlb_invalidate: true,
        };
        let layout = AcrBufferLayout::from_sizes(&config, 5000, 9000);
        assert_eq!(layout.fw_size, 8192);
        assert_eq!(layout.wpr_size, 12288);
        assert_eq!(layout.shadow_size, layout.wpr_size);
        assert_eq!(layout.wpr_payload_len, 9000);
        assert_eq!(layout.wpr_end_iova(), sysmem_iova::WPR + 9000);
        assert!(layout.skip_blob_dma);
        assert_eq!(layout.blob_iova, 0);
        assert_eq!(layout.blob_size, 0);
    }

    #[test]
    fn acr_buffer_layout_full_init_sets_blob_to_wpr() {
        let config = BootConfig::full_init();
        let layout = AcrBufferLayout::from_sizes(&config, 4096, 4096);
        assert!(!layout.skip_blob_dma);
        assert_eq!(layout.blob_iova, sysmem_iova::WPR);
        assert_eq!(layout.blob_size, 4096);
    }

    #[test]
    fn acr_buffer_layout_iovas_match_sysmem_constants() {
        let config = BootConfig::exp095_baseline();
        let layout = AcrBufferLayout::from_sizes(&config, 1, 1);
        assert_eq!(layout.low_catch_iova, sysmem_iova::LOW_CATCH);
        assert_eq!(layout.low_catch_size, sysmem_iova::LOW_CATCH_SIZE);
        assert_eq!(layout.fw_iova, sysmem_iova::ACR);
        assert_eq!(layout.wpr_iova, sysmem_iova::WPR);
        assert_eq!(layout.shadow_iova, sysmem_iova::SHADOW);
    }

    #[test]
    fn sec2_poll_respond_on_mailbox() {
        let mut st = Sec2PollState::new();
        let r = sec2_poll_check(0, 1, 0x100, 10, 5000, &mut st);
        assert_eq!(r, PollResult::Respond);
    }

    #[test]
    fn sec2_poll_respond_on_halted() {
        let mut st = Sec2PollState::new();
        let r = sec2_poll_check(falcon::CPUCTL_HALTED, 0, 0x100, 10, 5000, &mut st);
        assert_eq!(r, PollResult::Respond);
    }

    #[test]
    fn sec2_poll_settled_after_stable_pc() {
        let mut st = Sec2PollState::new();
        let _ = sec2_poll_check(0, 0, 0xABC, 0, 5000, &mut st);
        // 200 stable-PC steps → settled_count reaches 200 (still Continue); next step → 201 → Settled.
        for i in 0..200 {
            assert_eq!(
                sec2_poll_check(0, 0, 0xABC, u128::from(i as u32), 5000, &mut st),
                PollResult::Continue
            );
        }
        let r = sec2_poll_check(0, 0, 0xABC, 200, 5000, &mut st);
        assert_eq!(r, PollResult::Settled);
    }

    #[test]
    fn sec2_poll_timeout() {
        let mut st = Sec2PollState::new();
        let r = sec2_poll_check(0, 0, 0x200, 6000, 5000, &mut st);
        assert_eq!(r, PollResult::Timeout);
    }

    #[test]
    fn sec2_poll_pc_samples_record_progression() {
        let mut st = Sec2PollState::new();
        let _ = sec2_poll_check(0, 0, 0x100, 0, 5000, &mut st);
        let _ = sec2_poll_check(0, 0, 0x200, 5, 5000, &mut st);
        assert_eq!(st.pc_samples.len(), 2);
        assert!(st.pc_samples[0].contains("100"));
        assert!(st.pc_samples[1].contains("200"));
    }

    #[test]
    fn acr_buffer_layout_very_large_payload_rounds_to_pages() {
        let config = BootConfig {
            pde_upper: true,
            acr_vram_pte: false,
            blob_size_zero: true,
            bind_vram: false,
            imem_preload: false,
            tlb_invalidate: true,
        };
        let layout = AcrBufferLayout::from_sizes(&config, 12_000_000, 12_000_001);
        assert_eq!(layout.fw_size, 12_000_000_usize.div_ceil(4096) * 4096);
        assert_eq!(layout.wpr_size, 12_000_001_usize.div_ceil(4096) * 4096);
        assert_eq!(layout.wpr_payload_len, 12_000_001);
    }

    #[test]
    fn acr_buffer_layout_skip_blob_dma_vs_blob_mapped() {
        let mut c_skip = BootConfig::exp095_baseline();
        c_skip.blob_size_zero = true;
        let mut c_blob = BootConfig::exp095_baseline();
        c_blob.blob_size_zero = false;
        let skip = AcrBufferLayout::from_sizes(&c_skip, 4096, 8192);
        let with_blob = AcrBufferLayout::from_sizes(&c_blob, 4096, 8192);
        assert!(skip.skip_blob_dma);
        assert_eq!(skip.blob_size, 0);
        assert!(!with_blob.skip_blob_dma);
        assert_eq!(with_blob.blob_size, 8192);
        assert_eq!(with_blob.blob_iova, sysmem_iova::WPR);
    }

    #[test]
    fn sec2_poll_pc_progression_resets_settle_counter() {
        let mut st = Sec2PollState::new();
        let _ = sec2_poll_check(0, 0, 0x300, 0, 5000, &mut st);
        let _ = sec2_poll_check(0, 0, 0x300, 1, 5000, &mut st);
        assert_eq!(st.settled_count, 1);
        let _ = sec2_poll_check(0, 0, 0x301, 2, 5000, &mut st);
        assert_eq!(st.settled_count, 0);
        assert_eq!(st.last_pc, 0x301);
    }

    #[test]
    fn sec2_poll_pc_stuck_increments_settled_without_progress_samples() {
        let mut st = Sec2PollState::new();
        let _ = sec2_poll_check(0, 0, 0xDEAD, 0, 5000, &mut st);
        for i in 1_u32..=50 {
            let _ = sec2_poll_check(0, 0, 0xDEAD, u128::from(i), 5000, &mut st);
        }
        assert_eq!(st.settled_count, 50);
        assert_eq!(st.pc_samples.len(), 1);
    }
}
