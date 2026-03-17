// SPDX-License-Identifier: AGPL-3.0-only
//! GPFIFO submission and completion polling.

use crate::error::{DriverError, DriverResult};
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
    pub(super) fn submit_pushbuf(&mut self, pb_iova: u64, pb_size: u32) -> DriverResult<()> {
        let entry = gpfifo::encode_entry(pb_iova, pb_size);

        let slot = (self.gpfifo_put as usize) % gpfifo::ENTRIES;
        let offset = slot * gpfifo::ENTRY_SIZE;

        // Volatile write GPFIFO entry to DMA ring.
        let ring_ptr = self.gpfifo_ring.vaddr().cast_mut();
        // SAFETY: gpfifo_ring.vaddr() is valid from DmaBuffer::new; offset is
        // bounds-checked by slot % ENTRIES; volatile required for GPU DMA visibility.
        unsafe {
            std::ptr::write_volatile(ring_ptr.add(offset).cast::<u64>(), entry);
        }

        self.gpfifo_put = self.gpfifo_put.wrapping_add(1);

        // Volatile write GP_PUT to USERD at Volta RAMUSERD offset 0x8C.
        let userd_ptr = self.userd.vaddr().cast_mut();
        // SAFETY: userd.vaddr() is valid from DmaBuffer::new; ramuserd::GP_PUT
        // (0x8C) is within the 4096-byte USERD page; volatile required for GPU DMA.
        unsafe {
            std::ptr::write_volatile(
                userd_ptr.add(ramuserd::GP_PUT).cast::<u32>(),
                self.gpfifo_put,
            );
        }

        // Full memory fence to ensure DMA writes are globally visible
        // before the MMIO doorbell write crosses the PCIe bus.
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

        // Notify the GPU via NV_USERMODE_NOTIFY_CHANNEL_PENDING.
        self.bar0
            .write_u32(VfioChannel::doorbell_offset(), self.channel.id())
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("doorbell: {e}"))))?;

        Ok(())
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
        let userd_ptr = self.userd.vaddr();

        loop {
            // SAFETY: userd DMA page is valid for the lifetime of the device;
            // GP_GET at Volta RAMUSERD offset 0x88 is a u32 written by the
            // GPU via IOMMU DMA. Volatile read required for async GPU writes.
            let gp_get =
                unsafe { std::ptr::read_volatile(userd_ptr.add(ramuserd::GP_GET).cast::<u32>()) };

            if gp_get >= self.gpfifo_put {
                return Ok(());
            }

            if std::time::Instant::now() > deadline {
                // SAFETY: same as GP_GET — userd DMA page valid; GP_PUT within bounds.
                let gp_put_val = unsafe {
                    std::ptr::read_volatile(userd_ptr.add(ramuserd::GP_PUT).cast::<u32>())
                };
                let r = |reg: usize| self.bar0.read_u32(reg).unwrap_or(0xDEAD);
                let pfifo_intr = r(0x2100);
                let pccsr_chan0 = r(0x80_0004);
                let pbdma_intr: [u32; 4] = [
                    r(0x40108),
                    r(0x40108 + 0x2000),
                    r(0x40108 + 0x4000),
                    r(0x40108 + 0x6000),
                ];
                let pbdma_hce: [u32; 4] = [
                    r(0x40148),
                    r(0x40148 + 0x2000),
                    r(0x40148 + 0x4000),
                    r(0x40148 + 0x6000),
                ];
                let pbdma_idle: [u32; 4] = [r(0x3080), r(0x3084), r(0x3088), r(0x308C)];
                let pbdma_runl_map: [u32; 4] = [r(0x2390), r(0x2394), r(0x2398), r(0x239C)];
                let mmu_fault_status = r(0x0010_0A2C);
                let mmu_hubtlb_err = r(0x0010_4A20);
                let priv_ring_intr = r(0x0001_2070);
                tracing::error!(
                    gp_get,
                    gp_put = gp_put_val,
                    expected = self.gpfifo_put,
                    channel_id = self.channel.id(),
                    pfifo_intr = format!("{pfifo_intr:#010x}"),
                    pccsr_chan0 = format!("{pccsr_chan0:#010x}"),
                    pbdma_0_intr = format!("{:#010x}", pbdma_intr[0]),
                    pbdma_0_hce = format!("{:#010x}", pbdma_hce[0]),
                    pbdma_0_idle = format!("{:#010x}", pbdma_idle[0]),
                    pbdma_1_intr = format!("{:#010x}", pbdma_intr[1]),
                    pbdma_1_hce = format!("{:#010x}", pbdma_hce[1]),
                    pbdma_1_idle = format!("{:#010x}", pbdma_idle[1]),
                    pbdma_2_intr = format!("{:#010x}", pbdma_intr[2]),
                    pbdma_2_hce = format!("{:#010x}", pbdma_hce[2]),
                    pbdma_2_idle = format!("{:#010x}", pbdma_idle[2]),
                    pbdma_3_intr = format!("{:#010x}", pbdma_intr[3]),
                    pbdma_3_hce = format!("{:#010x}", pbdma_hce[3]),
                    pbdma_3_idle = format!("{:#010x}", pbdma_idle[3]),
                    pbdma_runl_map_0 = format!("{:#010x}", pbdma_runl_map[0]),
                    pbdma_runl_map_1 = format!("{:#010x}", pbdma_runl_map[1]),
                    pbdma_runl_map_2 = format!("{:#010x}", pbdma_runl_map[2]),
                    pbdma_runl_map_3 = format!("{:#010x}", pbdma_runl_map[3]),
                    mmu_fault_status = format!("{mmu_fault_status:#010x}"),
                    mmu_hubtlb_err = format!("{mmu_hubtlb_err:#010x}"),
                    priv_ring_intr = format!("{priv_ring_intr:#010x}"),
                    "Fence timeout: GPFIFO completion did not complete within timeout"
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
