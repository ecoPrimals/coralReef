// SPDX-License-Identifier: AGPL-3.0-or-later
//! Raw VFIO device handle for diagnostics and low-level BAR0 access.

use crate::error::DriverResult;
use crate::vfio::device::{DmaBackend, MappedBar, VfioDevice};
use crate::vfio::dma::DmaBuffer;

use super::layout::{GPFIFO_IOVA, USERD_IOVA, gpfifo};

use std::os::fd::AsRawFd;

/// Raw VFIO device handle for diagnostic/experimental access to BAR0.
///
/// Drop order: DMA buffers drop before `device` (which closes the container fd).
pub struct RawVfioDevice {
    /// MMIO-mapped BAR0 region for register access.
    pub bar0: MappedBar,
    /// Shared VFIO container handle for DMA mapping and diagnostics.
    pub container: DmaBackend,
    /// DMA buffer holding the GPFIFO command ring.
    pub gpfifo_ring: DmaBuffer,
    /// DMA buffer for the USERD (user data) doorbell page.
    pub userd: DmaBuffer,
    #[expect(dead_code, reason = "kept alive for fd lifecycle")]
    device: VfioDevice,
}

impl RawVfioDevice {
    /// Raw numeric VFIO container fd (same open file as [`Self::container`]).
    #[must_use]
    pub fn container_fd(&self) -> std::os::fd::RawFd {
        match &self.container {
            DmaBackend::LegacyContainer(fd) => fd.as_raw_fd(),
            DmaBackend::Iommufd { fd, .. } => fd.as_raw_fd(),
        }
    }

    /// Open a raw VFIO device by PCI BDF address (e.g. `"0000:06:00.0"`).
    pub fn open(bdf: &str) -> DriverResult<Self> {
        if let Err(e) = crate::vfio::channel::devinit::force_pci_d0(bdf) {
            tracing::warn!(bdf, error = %e, "force_pci_d0 failed (may already be in D0)");
        }
        let device = VfioDevice::open(bdf)?;
        Self::from_device(device)
    }

    /// Open using legacy VFIO group path, bypassing iommufd.
    pub fn open_legacy(bdf: &str) -> DriverResult<Self> {
        if let Err(e) = crate::vfio::channel::devinit::force_pci_d0(bdf) {
            tracing::warn!(bdf, error = %e, "force_pci_d0 failed (may already be in D0)");
        }
        let device = VfioDevice::open_legacy(bdf)?;
        Self::from_device(device)
    }

    fn from_device(device: VfioDevice) -> DriverResult<Self> {
        let container = device.dma_backend();
        let bar0 = device.map_bar(0)?;
        let gpfifo_ring = DmaBuffer::new(container.clone(), gpfifo::RING_SIZE, GPFIFO_IOVA)?;
        let userd = DmaBuffer::new(container.clone(), 4096, USERD_IOVA)?;
        Ok(Self {
            device,
            bar0,
            container,
            gpfifo_ring,
            userd,
        })
    }

    /// Returns the IOVA of the GPFIFO ring buffer.
    pub const fn gpfifo_iova() -> u64 {
        GPFIFO_IOVA
    }

    /// Returns the number of GPFIFO ring entries.
    pub const fn gpfifo_entries() -> u32 {
        gpfifo::ENTRIES as u32
    }

    /// Returns the IOVA of the USERD doorbell page.
    pub const fn userd_iova() -> u64 {
        USERD_IOVA
    }

    /// Perform VFIO device reset (FLR).
    ///
    /// # Errors
    ///
    /// Returns error if the reset ioctl fails.
    pub fn reset(&self) -> DriverResult<()> {
        self.device.reset().map_err(Into::into)
    }

    /// Perform PCI Secondary Bus Reset via VFIO.
    ///
    /// # Errors
    ///
    /// Returns error if the hot reset ioctl fails.
    pub fn pci_hot_reset(&self) -> DriverResult<()> {
        self.device.pci_hot_reset().map_err(Into::into)
    }

    /// Re-enable PCI bus master after a reset clears it.
    ///
    /// # Errors
    ///
    /// Returns error on PCI config write failure.
    pub fn enable_bus_master(&self) -> DriverResult<()> {
        self.device.enable_bus_master().map_err(Into::into)
    }

    /// Leaks the device handle without running drop (for diagnostic use).
    pub fn leak(self) {
        std::mem::forget(self);
    }
}

impl Drop for RawVfioDevice {
    fn drop(&mut self) {
        self.device.disable_bus_master();
        std::thread::sleep(std::time::Duration::from_millis(10));
        super::quiesce::quiesce_gpu_engines(&self.bar0);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
