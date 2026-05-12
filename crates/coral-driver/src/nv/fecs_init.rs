// SPDX-License-Identifier: AGPL-3.0-or-later
//! FECS channel method submission — Phase 3 of nouveau device init.
//!
//! After BAR0 GR register init and channel creation, some architectures
//! require a small set of FECS method entries to be submitted via the
//! channel push buffer.  Only entries with addresses ≤ 0x7FFC qualify
//! (13-bit push buffer method encoding limit).

use super::{generation, ioctl, probe, pushbuf, NvDevice};
use crate::gsp::{self, GrFirmwareBlobs, GrInitSequence};
use crate::{ComputeDevice, MemoryDomain};

impl NvDevice {
    /// Submit low-address FECS method entries via the channel push buffer.
    ///
    /// This is Phase 3 of device init — runs AFTER BAR0 GR init and channel
    /// creation. Submits only entries with addresses <= 0x7FFC (valid for
    /// 13-bit push buffer method encoding). Most architectures have zero
    /// such entries; the bulk of GR init is BAR0 register writes handled
    /// by [`probe::try_bar0_gr_init`].
    #[cfg(feature = "nouveau")]
    #[expect(dead_code, reason = "WIP: nouveau FECS channel init for sovereign warm handoff")]
    pub(super) fn try_fecs_channel_init(&mut self) {
        let chip = probe::sm_to_chip(self.sm_version);
        let blobs = match GrFirmwareBlobs::parse(chip) {
            Ok(b) => b,
            Err(e) => {
                tracing::debug!(chip, error = %e, "firmware not available — skipping FECS channel init");
                return;
            }
        };

        let profile = generation::profile_for_sm(self.sm_version);
        let seq = GrInitSequence::for_profile(&blobs, profile);
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

        let pb = pushbuf::PushBuf::gr_context_init(self.compute_class, &channel_methods);
        let pb_bytes = pb.as_bytes();

        let Ok(pb_size) = u64::try_from(pb_bytes.len()) else {
            tracing::warn!("GR init pushbuf too large — skipping");
            return;
        };

        let pb_handle = match self.alloc(pb_size, MemoryDomain::Gtt) {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(error = %e, "failed to allocate GR init pushbuf");
                return;
            }
        };

        if let Err(e) = self.upload(pb_handle, 0, pb_bytes) {
            tracing::warn!(error = %e, "failed to upload GR init pushbuf");
            let _ = self.free(pb_handle);
            return;
        }

        tracing::info!(
            chip,
            entries = channel_methods.len(),
            "submitting FECS channel methods"
        );

        let submit_result = if self.new_uapi {
            let pb_va = self.buffers.get(&pb_handle.0).map_or(0, |b| b.gpu_va);
            let Ok(push_len) = u32::try_from(pb_size) else {
                tracing::warn!("GR init pushbuf size exceeds u32 — skipping");
                let _ = self.free(pb_handle);
                return;
            };
            if let Some(syncobj) = self.exec_syncobj {
                ioctl::exec_submit_with_signal(
                    self.drm.fd(),
                    self.channel,
                    pb_va,
                    push_len,
                    syncobj,
                )
            } else {
                ioctl::exec_submit(self.drm.fd(), self.channel, pb_va, push_len)
            }
        } else {
            let pb_gem = self.buffers.get(&pb_handle.0).map_or(0, |b| b.gem_handle);
            ioctl::pushbuf_submit(self.drm.fd(), self.channel, pb_gem, 0, pb_size, &[pb_gem])
        };

        match submit_result {
            Ok(()) => {
                tracing::info!(chip, "FECS channel method init submitted");
                if let Some(syncobj) = self.exec_syncobj {
                    if let Err(e) =
                        ioctl::syncobj_wait(self.drm.fd(), syncobj, probe::syncobj_deadline())
                    {
                        tracing::warn!(error = %e, "FECS init syncobj wait failed");
                    }
                } else if let Some(gem) = self.buffers.get(&pb_handle.0).map(|b| b.gem_handle) {
                    let _ = ioctl::gem_cpu_prep(self.drm.fd(), gem);
                }
            }
            Err(e) => {
                tracing::warn!(chip, error = %e, "FECS channel method init failed");
            }
        }

        let _ = self.free(pb_handle);
    }
}
