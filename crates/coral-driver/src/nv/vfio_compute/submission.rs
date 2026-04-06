// SPDX-License-Identifier: AGPL-3.0-or-later
//! GPFIFO submission and completion polling.

use crate::error::{DriverError, DriverResult};
use crate::vfio::cache_ops::{clflush_range, memory_fence};
use crate::vfio::channel::mmu_fault;
use crate::vfio::channel::registers::{misc, pccsr, pfifo};
use crate::vfio::channel::{VfioChannel, ramuserd};

use std::borrow::Cow;

use super::NvVfioComputeDevice;
use super::gpfifo;

/// Sync timeout — 5 seconds matches the nouveau/UVM paths.
const SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Spin-loop sleep interval for GPFIFO polling.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_micros(10);

impl NvVfioComputeDevice {
    /// Submit a push buffer via GPFIFO.
    ///
    /// Writes a GPFIFO entry pointing to the given IOVA/size, updates
    /// GP_PUT in the USERD page at the correct Volta RAMUSERD offset,
    /// then notifies the GPU via the USERMODE doorbell register.
    ///
    /// Uses volatile writes + memory fences to ensure the GPU sees the
    /// GPFIFO entry and GP_PUT before the doorbell MMIO write.
    ///
    /// H1 experiment: On non-coherent platforms (e.g. AMD Zen 2 + VFIO), CPU
    /// writes may remain in cache; PBDMA reads via IOMMU see stale data. We
    /// flush the GPFIFO slot and USERD page from CPU cache before the doorbell.
    pub(super) fn submit_pushbuf(&mut self, pb_iova: u64, pb_size: u32) -> DriverResult<()> {
        let entry = gpfifo::encode_entry(pb_iova, pb_size);

        let slot = (self.gpfifo_put as usize) % gpfifo::ENTRIES;
        let offset = slot * gpfifo::ENTRY_SIZE;

        self.gpfifo_ring.volatile_write_u64(offset, entry);
        self.gpfifo_put = self.gpfifo_put.wrapping_add(1);
        self.userd
            .volatile_write_u32(ramuserd::GP_PUT, self.gpfifo_put);

        // H1 hypothesis: CPU writes GP_PUT to DMA-mapped USERD page, but the write
        // sits in CPU cache. PBDMA reads via IOMMU DMA and sees stale zero, so
        // GP_GET never advances. Fix: flush both GPFIFO entry and USERD from CPU
        // cache before the doorbell, so PBDMA sees the latest values when it
        // reads after the doorbell notification.
        clflush_range(&self.gpfifo_ring.as_slice()[offset..offset + gpfifo::ENTRY_SIZE]);
        clflush_range(self.userd.as_slice());
        memory_fence();

        // Notify the GPU via NV_USERMODE_NOTIFY_CHANNEL_PENDING.
        self.bar0
            .write_u32(VfioChannel::doorbell_offset(), self.channel.id())
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("doorbell: {e}"))))?;

        Ok(())
    }

    /// Submit a push buffer via GPFIFO with timed post-doorbell diagnostic captures.
    ///
    /// Identical to `submit_pushbuf` but captures PBDMA + PCCSR state at
    /// fixed intervals (1ms, 10ms, 100ms, 500ms, 1s) after the doorbell
    /// write. Returns the timed captures for analysis.
    pub(super) fn submit_pushbuf_traced(
        &mut self,
        pb_iova: u64,
        pb_size: u32,
    ) -> DriverResult<Vec<super::diagnostics::TimedCapture>> {
        use super::diagnostics::{
            PbdmaSnapshot, PccsrSnapshot, TimedCapture, find_pbdmas_for_runlist,
        };

        self.submit_pushbuf(pb_iova, pb_size)?;

        let pbdma_map = self.bar0.read_u32(pfifo::PBDMA_MAP).unwrap_or(0);
        let gr_pbdmas = find_pbdmas_for_runlist(pbdma_map, &self.bar0, 1);
        let channel_id = self.channel.id();
        let start = std::time::Instant::now();

        let intervals: &[(&str, u64)] = &[
            ("t+1ms", 1_000),
            ("t+10ms", 10_000),
            ("t+100ms", 100_000),
            ("t+500ms", 500_000),
            ("t+1s", 1_000_000),
        ];

        let mut captures = Vec::with_capacity(intervals.len());

        for &(label, target_us) in intervals {
            let target = std::time::Duration::from_micros(target_us);
            let elapsed = start.elapsed();
            if elapsed < target {
                std::thread::sleep(target - elapsed);
            }

            let pccsr = PccsrSnapshot::capture(&self.bar0, channel_id);
            let pbdma_snapshots: Vec<PbdmaSnapshot> = gr_pbdmas
                .iter()
                .map(|&id| PbdmaSnapshot::capture(&self.bar0, id, start))
                .collect();

            tracing::debug!(
                label,
                pccsr_status = pccsr.status_name(),
                pccsr_chan = format!("{:#010x}", pccsr.channel),
                pbdma_count = pbdma_snapshots.len(),
                "post-doorbell diagnostic capture"
            );

            captures.push(TimedCapture {
                label,
                delay_us: start.elapsed().as_micros() as u64,
                pccsr,
                pbdma_snapshots,
            });
        }

        Ok(captures)
    }

    /// Poll USERD GP_GET until it catches up to GP_PUT, indicating
    /// the GPU has consumed all submitted GPFIFO entries.
    ///
    /// The GPU writes GP_GET back to the USERD DMA page at the Volta
    /// RAMUSERD offset (0x88). We poll this with a spin-loop + sleep,
    /// matching the UVM path's `poll_gpfifo_completion()` pattern.
    pub(super) fn poll_gpfifo_completion(&self) -> DriverResult<()> {
        if self.gpfifo_put == 0 {
            return Ok(());
        }

        let deadline = std::time::Instant::now() + SYNC_TIMEOUT;

        loop {
            let gp_get = self.userd.volatile_read_u32(ramuserd::GP_GET);

            if gp_get >= self.gpfifo_put {
                return Ok(());
            }

            if std::time::Instant::now() > deadline {
                let gp_put_val = self.userd.volatile_read_u32(ramuserd::GP_PUT);
                let r = |reg: usize| self.bar0.read_u32(reg).unwrap_or(0xDEAD);

                let mmu_info = mmu_fault::read_mmu_faults(&self.bar0);
                mmu_fault::log_mmu_faults(&mmu_info);

                let pfifo_intr = r(pfifo::INTR);
                let pccsr_chan = r(pccsr::channel(self.channel.id()));
                let priv_ring_intr = r(misc::PRIV_RING);
                let pbdma_intr: [u32; 2] = [r(0x40108), r(0x40108 + 0x2000)];
                let pbdma_state: [u32; 2] = [r(0x400B0), r(0x400B0 + 0x2000)];

                tracing::error!(
                    gp_get,
                    gp_put = gp_put_val,
                    expected = self.gpfifo_put,
                    channel_id = self.channel.id(),
                    pfifo_intr = format!("{pfifo_intr:#010x}"),
                    pccsr_chan = format!("{pccsr_chan:#010x}"),
                    priv_ring_intr = format!("{priv_ring_intr:#010x}"),
                    pbdma_0_intr = format!("{:#010x}", pbdma_intr[0]),
                    pbdma_0_state = format!("{:#010x}", pbdma_state[0]),
                    pbdma_1_intr = format!("{:#010x}", pbdma_intr[1]),
                    pbdma_1_state = format!("{:#010x}", pbdma_state[1]),
                    "Fence timeout: GPFIFO completion stalled — see MMU fault above"
                );
                return Err(DriverError::FenceTimeout {
                    ms: SYNC_TIMEOUT.as_millis() as u64,
                });
            }

            std::hint::spin_loop();
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}
