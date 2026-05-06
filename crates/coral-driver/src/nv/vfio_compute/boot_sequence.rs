// SPDX-License-Identifier: AGPL-3.0-or-later
//! Generation-dispatched sovereign boot sequences.
//!
//! Encapsulates the per-generation differences in the VFIO sovereign boot
//! pipeline: cold initialization, channel creation, fence allocation, and
//! warm restart. All generation-specific branching lives here instead of
//! scattered `match profile.page_table_format` arms in `device_open.rs`.
//!
//! # Usage
//!
//! ```ignore
//! let seq = boot_sequence_for(profile);
//! seq.cold_init(&bar0)?;
//! let channel = seq.create_channel(container, &bar0, gpfifo_iova, entries, userd_iova)?;
//! ```

use crate::error::{DriverError, DriverResult};
use crate::nv::generation::{
    CompletionStrategy, GenerationProfile, is_kepler, uses_semaphore_fence,
};
use crate::vfio::channel::VfioChannel;
use crate::vfio::device::{DmaBackend, MappedBar};
use crate::vfio::dma::DmaBuffer;

use super::hardware_guard::GuardedBar;
use super::layout::{FENCE_BUF_IOVA, FENCE_PB_IOVA};

/// Describes the generation-specific boot and channel creation behavior for
/// a sovereign VFIO compute device.
pub(crate) trait SovereignBootSequence {
    /// Perform cold GR initialization on BAR0 (PGRAPH reset, firmware load, etc.).
    fn cold_init(&self, bar0: &MappedBar) -> DriverResult<()>;

    /// Create a GPFIFO channel appropriate for this generation's page table format.
    fn create_channel(
        &self,
        container: DmaBackend,
        bar0: &MappedBar,
        gpfifo_iova: u64,
        gpfifo_entries: u32,
        userd_iova: u64,
        runlist_id: u32,
    ) -> DriverResult<VfioChannel>;

    /// Allocate semaphore fence buffers if this generation requires them.
    /// Returns `(fence_buf, fence_pb_buf)` — both `None` for pre-Blackwell.
    fn alloc_fence_buffers(
        &self,
        container: DmaBackend,
    ) -> DriverResult<(Option<DmaBuffer>, Option<DmaBuffer>)>;

    /// Whether this generation uses semaphore-based completion (vs GP_GET poll).
    fn uses_semaphore_fence(&self) -> bool;

    /// The completion strategy for this generation.
    #[expect(dead_code, reason = "part of SovereignBootSequence trait — wired in device_open")]
    fn completion_strategy(&self) -> CompletionStrategy;
}

/// Kepler (GK110/GK210) — direct PIO falcon boot, 2-level page tables.
pub(crate) struct KeplerBoot;

/// Volta/Turing/Ampere/Ada — SEC2/ACR falcon chain, 5-level page tables.
pub(crate) struct VoltaBoot {
    pub sm_version: u32,
}

/// Blackwell+ — kernel module promotes GR context, semaphore fences.
pub(crate) struct BlackwellBoot {
    pub sm_version: u32,
}

impl SovereignBootSequence for KeplerBoot {
    fn cold_init(&self, bar0: &MappedBar) -> DriverResult<()> {
        super::init::kepler_cold_init(bar0);
        Ok(())
    }

    fn create_channel(
        &self,
        container: DmaBackend,
        bar0: &MappedBar,
        gpfifo_iova: u64,
        gpfifo_entries: u32,
        userd_iova: u64,
        runlist_id: u32,
    ) -> DriverResult<VfioChannel> {
        let guard = GuardedBar::new(bar0, 32).map_err(|r| {
            DriverError::HardwareGuardRefusal(r.to_string().into())
        })?;
        tracing::info!("Kepler boot: using 2-level page table channel");
        VfioChannel::create_kepler(
            container,
            &guard,
            gpfifo_iova,
            gpfifo_entries,
            userd_iova,
            runlist_id,
        )
    }

    fn alloc_fence_buffers(
        &self,
        _container: DmaBackend,
    ) -> DriverResult<(Option<DmaBuffer>, Option<DmaBuffer>)> {
        Ok((None, None))
    }

    fn uses_semaphore_fence(&self) -> bool {
        false
    }

    fn completion_strategy(&self) -> CompletionStrategy {
        CompletionStrategy::GpGetPoll
    }
}

impl SovereignBootSequence for VoltaBoot {
    fn cold_init(&self, bar0: &MappedBar) -> DriverResult<()> {
        super::NvVfioComputeDevice::apply_gr_bar0_init(bar0, self.sm_version);
        Ok(())
    }

    fn create_channel(
        &self,
        container: DmaBackend,
        bar0: &MappedBar,
        gpfifo_iova: u64,
        gpfifo_entries: u32,
        userd_iova: u64,
        runlist_id: u32,
    ) -> DriverResult<VfioChannel> {
        VfioChannel::create(
            container,
            bar0,
            gpfifo_iova,
            gpfifo_entries,
            userd_iova,
            runlist_id,
        )
    }

    fn alloc_fence_buffers(
        &self,
        _container: DmaBackend,
    ) -> DriverResult<(Option<DmaBuffer>, Option<DmaBuffer>)> {
        Ok((None, None))
    }

    fn uses_semaphore_fence(&self) -> bool {
        false
    }

    fn completion_strategy(&self) -> CompletionStrategy {
        CompletionStrategy::GpGetPoll
    }
}

impl SovereignBootSequence for BlackwellBoot {
    fn cold_init(&self, bar0: &MappedBar) -> DriverResult<()> {
        super::NvVfioComputeDevice::apply_gr_bar0_init(bar0, self.sm_version);
        Ok(())
    }

    fn create_channel(
        &self,
        container: DmaBackend,
        bar0: &MappedBar,
        gpfifo_iova: u64,
        gpfifo_entries: u32,
        userd_iova: u64,
        runlist_id: u32,
    ) -> DriverResult<VfioChannel> {
        VfioChannel::create(
            container,
            bar0,
            gpfifo_iova,
            gpfifo_entries,
            userd_iova,
            runlist_id,
        )
    }

    fn alloc_fence_buffers(
        &self,
        container: DmaBackend,
    ) -> DriverResult<(Option<DmaBuffer>, Option<DmaBuffer>)> {
        let fb = DmaBuffer::new(container.clone(), 4096, FENCE_BUF_IOVA)?;
        fb.volatile_write_u32(0, 0);
        let fpb = DmaBuffer::new(container, 4096, FENCE_PB_IOVA)?;
        tracing::info!("Blackwell: semaphore fence buffers allocated");
        Ok((Some(fb), Some(fpb)))
    }

    fn uses_semaphore_fence(&self) -> bool {
        true
    }

    fn completion_strategy(&self) -> CompletionStrategy {
        CompletionStrategy::SemaphoreFence
    }
}

/// Select the appropriate boot sequence implementation for a generation profile.
pub(crate) fn boot_sequence_for(
    profile: &'static GenerationProfile,
    sm_version: u32,
) -> Box<dyn SovereignBootSequence> {
    if is_kepler(profile) {
        Box::new(KeplerBoot)
    } else if uses_semaphore_fence(profile) {
        Box::new(BlackwellBoot { sm_version })
    } else {
        Box::new(VoltaBoot { sm_version })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nv::generation::profile_for_sm;

    #[test]
    fn kepler_profile_selects_kepler_boot() {
        let profile = profile_for_sm(37);
        let seq = boot_sequence_for(profile, 37);
        assert!(!seq.uses_semaphore_fence());
        assert_eq!(seq.completion_strategy(), CompletionStrategy::GpGetPoll);
    }

    #[test]
    fn volta_profile_selects_volta_boot() {
        let profile = profile_for_sm(70);
        let seq = boot_sequence_for(profile, 70);
        assert!(!seq.uses_semaphore_fence());
        assert_eq!(seq.completion_strategy(), CompletionStrategy::GpGetPoll);
    }

    #[test]
    fn blackwell_profile_selects_blackwell_boot() {
        let profile = profile_for_sm(120);
        let seq = boot_sequence_for(profile, 120);
        assert!(seq.uses_semaphore_fence());
        assert_eq!(seq.completion_strategy(), CompletionStrategy::SemaphoreFence);
    }
}
