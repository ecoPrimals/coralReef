// SPDX-License-Identifier: AGPL-3.0-or-later

//! DMA buffer bundle and IOVA metadata for the sysmem ACR boot path.

use crate::vfio::device::DmaBackend;
use crate::vfio::dma::DmaBuffer;

/// Holds all IOMMU-backed buffers and derived sizes for sysmem ACR boot.
pub(super) struct SysmemDmaState {
    pub _low_catch: DmaBuffer,
    pub _mid_gap1: Option<DmaBuffer>,
    pub _mid_gap2: Option<DmaBuffer>,
    pub inst_dma: DmaBuffer,
    pub pd3_dma: DmaBuffer,
    pub pd2_dma: DmaBuffer,
    pub pd1_dma: DmaBuffer,
    pub pd0_dma: DmaBuffer,
    pub pt0_dma: DmaBuffer,
    pub acr_dma: DmaBuffer,
    pub wpr_dma: DmaBuffer,
    pub shadow_dma: DmaBuffer,
    pub wpr_data: Vec<u8>,
    pub acr_payload_size: usize,
    pub wpr_base_iova: u64,
    pub wpr_end_iova: u64,
    pub shadow_iova: u64,
    /// Filled after page-table setup (covers VA from WPR end to 2 MiB).
    pub _high_catch: Option<DmaBuffer>,
    /// Retained so `allocate_dma` can clone the backend for gap buffers.
    pub container: DmaBackend,
}
