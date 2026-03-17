// SPDX-License-Identifier: AGPL-3.0-only
//! VFIO GR init — BAR0 register writes and FECS channel methods.

use crate::ComputeDevice;
use crate::error::{DriverError, DriverResult};
use crate::gsp::{self, GrFirmwareBlobs, GrInitSequence};
use crate::vfio::device::MappedBar;

use super::super::pushbuf::PushBuf;
use super::{NvVfioComputeDevice, sm_to_chip};

impl NvVfioComputeDevice {
    /// Apply BAR0 GR init writes from NVIDIA firmware blobs.
    ///
    /// Parses `sw_bundle_init.bin` etc. from `/lib/firmware/nvidia/{chip}/gr/`,
    /// builds the init sequence, then applies the BAR0-targeted writes
    /// (PMC engine enable, FIFO enable, PGRAPH register programming).
    pub(super) fn apply_gr_bar0_init(bar0: &MappedBar, sm_version: u32) {
        let chip = sm_to_chip(sm_version);
        let blobs = match GrFirmwareBlobs::parse(chip) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(chip, error = %e, "GR firmware not available — skipping BAR0 GR init");
                return;
            }
        };

        let seq = if sm_version == 70 {
            GrInitSequence::for_gv100(&blobs)
        } else {
            GrInitSequence::from_blobs(&blobs)
        };

        let (bar0_writes, fecs_entries) = gsp::split_for_application(&seq);

        tracing::info!(
            chip,
            bar0_writes = bar0_writes.len(),
            fecs_entries = fecs_entries.len(),
            total = seq.len(),
            "sovereign VFIO GR init: applying {} BAR0 register writes",
            bar0_writes.len()
        );

        // Only apply writes with 4-byte-aligned offsets that fit within BAR0.
        let bar0_size = bar0.size() as u32;
        let writes: Vec<(u32, u32)> = bar0_writes
            .iter()
            .filter(|w| {
                if w.offset % 4 != 0 {
                    tracing::debug!(
                        chip,
                        offset = format!("{:#010x}", w.offset),
                        "skipping non-aligned BAR0 write"
                    );
                    return false;
                }
                if w.offset + 4 > bar0_size {
                    tracing::debug!(
                        chip,
                        offset = format!("{:#010x}", w.offset),
                        bar0_size = format!("{bar0_size:#010x}"),
                        "skipping out-of-range BAR0 write"
                    );
                    return false;
                }
                true
            })
            .map(|w| (w.offset, w.value))
            .collect();

        let (applied, failed) = bar0.apply_gr_bar0_writes(&writes);

        if failed > 0 {
            tracing::warn!(chip, applied, failed, "BAR0 GR init had write failures");
        } else {
            tracing::info!(chip, applied, "BAR0 GR init complete");
        }

        // Brief settle after engine enable writes.
        for w in &bar0_writes {
            if w.delay_us > 0 {
                std::thread::sleep(std::time::Duration::from_micros(u64::from(w.delay_us)));
            }
        }
    }

    /// Submit FECS channel init methods via GPFIFO after channel creation.
    ///
    /// Builds a push buffer containing the GR context setup methods
    /// from `sw_bundle_init.bin` / `sw_method_init.bin` (entries with
    /// offsets <= 0x7FFC that are submittable as channel methods).
    pub(super) fn apply_fecs_channel_init(&mut self) {
        let chip = sm_to_chip(self.sm_version);
        let blobs = match GrFirmwareBlobs::parse(chip) {
            Ok(b) => b,
            Err(e) => {
                tracing::debug!(chip, error = %e, "firmware not available — skipping FECS init");
                return;
            }
        };

        let seq = if self.sm_version == 70 {
            GrInitSequence::for_gv100(&blobs)
        } else {
            GrInitSequence::from_blobs(&blobs)
        };

        let (_bar0, fecs) = gsp::split_for_application(&seq);

        let channel_methods: Vec<(u32, u32)> = fecs
            .iter()
            .filter(|w| {
                matches!(
                    w.category,
                    gsp::RegCategory::BundleInit | gsp::RegCategory::MethodInit
                )
            })
            .map(|w| (w.offset, w.value))
            .collect();

        if channel_methods.is_empty() {
            tracing::debug!(chip, "no FECS channel methods to submit");
            return;
        }

        tracing::info!(
            chip,
            entries = channel_methods.len(),
            "submitting FECS channel methods via GPFIFO"
        );

        let pb = PushBuf::gr_context_init(self.compute_class, &channel_methods);
        let pb_bytes = pb.as_bytes();

        let pb_result = (|| -> DriverResult<()> {
            let (pb_handle, pb_iova) = self.alloc_dma(pb_bytes.len())?;
            self.upload(pb_handle, 0, pb_bytes)?;

            let pb_size = u32::try_from(pb_bytes.len())
                .map_err(|_| DriverError::platform_overflow("FECS pushbuf size fits u32"))?;

            self.submit_pushbuf(pb_iova, pb_size)?;

            // Wait for FECS init to complete before returning the device
            // as ready for compute dispatch.
            self.poll_gpfifo_completion()?;

            let _ = self.free(pb_handle);
            Ok(())
        })();

        match pb_result {
            Ok(()) => tracing::info!(chip, "FECS channel init complete — GR engine ready"),
            Err(e) => {
                tracing::warn!(chip, error = %e, "FECS channel init failed (expected on cold VFIO — GR engine requires falcon firmware)")
            }
        }
    }
}
