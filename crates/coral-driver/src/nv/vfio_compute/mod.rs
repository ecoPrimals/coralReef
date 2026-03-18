// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA VFIO compute device — direct BAR0/DMA dispatch without kernel driver.
//!
//! Implements [`ComputeDevice`] using the VFIO subsystem:
//! - BAR0 MMIO for register access (GR init, GPFIFO doorbell)
//! - DMA buffers for shader code, QMD, push buffers, and user data
//! - Direct GPFIFO submission via BAR0 USERD write
//!
//! # Prerequisites (provided by ecosystem hardware setup)
//!
//! - GPU bound to `vfio-pci`
//! - IOMMU enabled and configured
//! - User has `/dev/vfio/*` permissions
//!
//! # Architecture
//!
//! ```text
//! NvVfioComputeDevice
//!   ├─ VfioDevice       (container + group + device fd)
//!   ├─ MappedBar (BAR0) (MMIO register access)
//!   ├─ DmaBuffer pool   (IOMMU-mapped host memory for GPU)
//!   │   ├─ GPFIFO ring  (command entries)
//!   │   ├─ USERD page   (doorbell for GPFIFO put pointer)
//!   │   └─ user buffers (shader, QMD, data)
//!   └─ pushbuf + QMD    (reuses coral-driver's existing builders)
//! ```

mod dispatch;
mod init;
mod submission;

use crate::error::{DriverError, DriverResult};
use crate::vfio::channel::VfioChannel;
use crate::vfio::device::{MappedBar, VfioDevice};
use crate::vfio::dma::DmaBuffer;
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use std::collections::HashMap;

/// BAR0 register offsets for NVIDIA GPU.
mod bar0_reg {
    /// Boot0 register — chip identification.
    pub const BOOT0: usize = 0x0000_0000;
}

/// GPFIFO configuration constants.
pub(super) mod gpfifo {
    /// Number of GPFIFO entries (must be power of 2).
    pub const ENTRIES: usize = 128;
    /// Size of each GPFIFO entry in bytes.
    pub const ENTRY_SIZE: usize = 8;
    /// Total GPFIFO ring size in bytes.
    pub const RING_SIZE: usize = ENTRIES * ENTRY_SIZE;

    /// Encode a GPFIFO indirect-buffer entry (NVB06F GP_ENTRY format).
    pub fn encode_entry(gpu_addr: u64, len_bytes: u32) -> u64 {
        let lo = gpu_addr & 0xFFFF_FFFC;
        let hi_addr = (gpu_addr >> 32) & 0xFF;
        let len_dwords = u64::from(len_bytes / 4);
        let hi = hi_addr | (len_dwords << 10);
        lo | (hi << 32)
    }
}

/// IOVA base for user DMA allocations — above GPFIFO/USERD.
const USER_IOVA_BASE: u64 = 0x10_0000;

/// GPFIFO ring IOVA.
const GPFIFO_IOVA: u64 = 0x1000;

/// USERD page IOVA.
const USERD_IOVA: u64 = 0x2000;

/// Local memory window address for Volta+ (SM >= 70).
pub(super) const LOCAL_MEM_WINDOW_VOLTA: u64 = 0xFF00_0000_0000_0000;

/// Local memory window address for pre-Volta (SM < 70).
pub(super) const LOCAL_MEM_WINDOW_LEGACY: u64 = 0xFF00_0000;

/// Map SM version to chip codename for firmware lookup.
pub(super) const fn sm_to_chip(sm: u32) -> &'static str {
    match sm {
        50..=52 => "gm200",
        60..=62 => "gp100",
        70 => "gv100",
        75 => "tu102",
        80 => "ga100",
        86..=87 => "ga102",
        89 => "ad102",
        _ => "gv100",
    }
}

/// DMA-backed GPU buffer tracked by the VFIO device.
struct VfioBuffer {
    dma: DmaBuffer,
    size: u64,
}

/// NVIDIA compute device via VFIO — direct BAR0 + DMA dispatch.
pub struct NvVfioComputeDevice {
    #[expect(dead_code, reason = "kept alive for fd lifecycle; used by DmaBuffer")]
    device: VfioDevice,
    bar0: MappedBar,
    sm_version: u32,
    compute_class: u32,
    gpfifo_ring: DmaBuffer,
    gpfifo_put: u32,
    userd: DmaBuffer,
    channel: VfioChannel,
    next_handle: u32,
    next_iova: u64,
    container_fd: std::os::fd::RawFd,
    buffers: HashMap<u32, VfioBuffer>,
    inflight: Vec<BufferHandle>,
}

/// GR engine diagnostic status from BAR0 registers.
#[derive(Debug)]
pub struct GrEngineStatus {
    /// BAR0 register value for PGRAPH status (offset 0x0040_0700).
    pub pgraph_status: u32,
    /// BAR0 register value for FECS CPU control (offset 0x0040_9100).
    pub fecs_cpuctl: u32,
    /// BAR0 register value for FECS mailbox 0 (offset 0x0040_9130).
    pub fecs_mailbox0: u32,
    /// BAR0 register value for FECS mailbox 1 (offset 0x0040_9134).
    pub fecs_mailbox1: u32,
    /// BAR0 register value for FECS hardware config (offset 0x0040_9800).
    pub fecs_hwcfg: u32,
    /// BAR0 register value for GPCCS CPU control (offset 0x0041_a100).
    pub gpccs_cpuctl: u32,
    /// BAR0 register value for PMC enable (offset 0x0000_0200).
    pub pmc_enable: u32,
    /// BAR0 register value for PFIFO enable (offset 0x0000_2504).
    pub pfifo_enable: u32,
}

impl GrEngineStatus {
    /// Returns `true` if the FECS (Firmware Engine Control Subsystem) is halted.
    #[must_use]
    pub fn fecs_halted(&self) -> bool {
        self.fecs_cpuctl & 0x20 != 0 || self.fecs_cpuctl == 0xDEAD_DEAD
    }

    /// Returns `true` if the GR (Graphics) engine is enabled in PMC.
    #[must_use]
    pub fn gr_enabled(&self) -> bool {
        self.pmc_enable & (1 << 12) != 0
    }
}

impl std::fmt::Display for GrEngineStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "GR: pmc={:#010x} pfifo={:#010x} pgraph={:#010x} fecs_cpu={:#010x} fecs_mb0={:#010x} fecs_mb1={:#010x} fecs_hw={:#010x} gpccs={:#010x} [fecs_halted={} gr_en={}]",
            self.pmc_enable,
            self.pfifo_enable,
            self.pgraph_status,
            self.fecs_cpuctl,
            self.fecs_mailbox0,
            self.fecs_mailbox1,
            self.fecs_hwcfg,
            self.gpccs_cpuctl,
            self.fecs_halted(),
            self.gr_enabled()
        )
    }
}

/// Raw VFIO device handle for diagnostic/experimental access to BAR0.
pub struct RawVfioDevice {
    #[expect(dead_code, reason = "kept alive for fd lifecycle")]
    device: VfioDevice,
    /// MMIO-mapped BAR0 region for register access.
    pub bar0: MappedBar,
    /// VFIO container file descriptor for DMA mapping.
    pub container_fd: std::os::fd::RawFd,
    /// DMA buffer holding the GPFIFO command ring.
    pub gpfifo_ring: DmaBuffer,
    /// DMA buffer for the USERD (user data) doorbell page.
    pub userd: DmaBuffer,
}

impl RawVfioDevice {
    /// Open a raw VFIO device by PCI BDF address (e.g. `"0000:06:00.0"`).
    pub fn open(bdf: &str) -> DriverResult<Self> {
        if let Err(e) = crate::vfio::channel::devinit::force_pci_d0(bdf) {
            tracing::warn!(bdf, error = %e, "force_pci_d0 failed (may already be in D0)");
        }
        let device = VfioDevice::open(bdf)?;
        let container_fd = device.container_fd();
        let bar0 = device.map_bar(0)?;
        let gpfifo_ring = DmaBuffer::new(container_fd, gpfifo::RING_SIZE, GPFIFO_IOVA)?;
        let userd = DmaBuffer::new(container_fd, 4096, USERD_IOVA)?;
        Ok(Self {
            device,
            bar0,
            container_fd,
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

    /// Leaks the device handle without running drop (for diagnostic use).
    pub fn leak(self) {
        std::mem::forget(self);
    }
}

impl NvVfioComputeDevice {
    /// Opens an NVIDIA VFIO compute device by PCI BDF, SM version, and compute class.
    pub fn open(bdf: &str, sm_version: u32, compute_class: u32) -> DriverResult<Self> {
        let device = VfioDevice::open(bdf)?;
        let container_fd = device.container_fd();
        let bar0 = device.map_bar(0)?;

        let chip_id = bar0.read_u32(bar0_reg::BOOT0)?;
        tracing::info!(
            bdf,
            chip_id = format_args!("{chip_id:#010x}"),
            sm_version,
            "VFIO GPU opened via BAR0"
        );

        NvVfioComputeDevice::apply_gr_bar0_init(&bar0, sm_version);

        let gpfifo_ring = DmaBuffer::new(container_fd, gpfifo::RING_SIZE, GPFIFO_IOVA)?;
        let userd = DmaBuffer::new(container_fd, 4096, USERD_IOVA)?;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "GPFIFO entries constant always fits u32"
        )]
        let channel = VfioChannel::create(
            container_fd,
            &bar0,
            GPFIFO_IOVA,
            gpfifo::ENTRIES as u32,
            USERD_IOVA,
            0,
        )?;

        let mut dev = Self {
            device,
            bar0,
            sm_version,
            compute_class,
            gpfifo_ring,
            gpfifo_put: 0,
            userd,
            channel,
            next_handle: 1,
            next_iova: USER_IOVA_BASE,
            container_fd,
            buffers: HashMap::new(),
            inflight: Vec::new(),
        };

        dev.apply_fecs_channel_init();

        Ok(dev)
    }

    /// Reads GR engine diagnostic status from BAR0 registers.
    pub fn gr_engine_status(&self) -> GrEngineStatus {
        let r = |off: usize| self.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);

        GrEngineStatus {
            pgraph_status: r(0x0040_0700),
            fecs_cpuctl: r(0x0040_9100),
            fecs_mailbox0: r(0x0040_9130),
            fecs_mailbox1: r(0x0040_9134),
            fecs_hwcfg: r(0x0040_9800),
            gpccs_cpuctl: r(0x0041_a100),
            pmc_enable: r(0x0000_0200),
            pfifo_enable: r(0x0000_2504),
        }
    }

    pub(super) fn alloc_dma(&mut self, size: usize) -> DriverResult<(BufferHandle, u64)> {
        let aligned = size.div_ceil(4096) * 4096;
        let iova = self.next_iova;
        self.next_iova += aligned as u64;

        let dma = DmaBuffer::new(self.container_fd, size, iova)?;
        let handle_id = self.next_handle;
        self.next_handle += 1;

        let handle = BufferHandle(handle_id);
        self.buffers.insert(
            handle_id,
            VfioBuffer {
                dma,
                size: size as u64,
            },
        );

        Ok((handle, iova))
    }
}

impl ComputeDevice for NvVfioComputeDevice {
    fn alloc(&mut self, size: u64, _domain: MemoryDomain) -> DriverResult<BufferHandle> {
        let size_usize = usize::try_from(size).map_err(|_| DriverError::AllocFailed {
            size,
            domain: _domain,
        })?;
        let (handle, _iova) = self.alloc_dma(size_usize)?;
        Ok(handle)
    }

    fn free(&mut self, handle: BufferHandle) -> DriverResult<()> {
        self.buffers
            .remove(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        Ok(())
    }

    fn upload(&mut self, handle: BufferHandle, offset: u64, data: &[u8]) -> DriverResult<()> {
        let buf = self
            .buffers
            .get_mut(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        let off = offset as usize;
        let slice = buf.dma.as_mut_slice();
        if off + data.len() > slice.len() {
            return Err(DriverError::SubmitFailed(
                "upload exceeds buffer bounds".into(),
            ));
        }
        slice[off..off + data.len()].copy_from_slice(data);
        Ok(())
    }

    fn readback(&self, handle: BufferHandle, offset: u64, len: usize) -> DriverResult<Vec<u8>> {
        let buf = self
            .buffers
            .get(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        let off = offset as usize;
        let slice = buf.dma.as_slice();
        if off + len > slice.len() {
            return Err(DriverError::SubmitFailed(
                "readback exceeds buffer bounds".into(),
            ));
        }
        Ok(slice[off..off + len].to_vec())
    }

    fn dispatch(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
    ) -> DriverResult<()> {
        let mut temps: Vec<BufferHandle> = Vec::with_capacity(4);
        let result = self.dispatch_inner(shader, buffers, dims, info, &mut temps);
        if result.is_ok() {
            self.inflight.extend(temps);
        } else {
            for h in temps {
                let _ = self.free(h);
            }
        }
        result
    }

    fn sync(&mut self) -> DriverResult<()> {
        self.poll_gpfifo_completion()?;
        let inflight = std::mem::take(&mut self.inflight);
        for handle in inflight {
            let _ = self.free(handle);
        }
        Ok(())
    }
}

impl Drop for NvVfioComputeDevice {
    fn drop(&mut self) {
        let inflight = std::mem::take(&mut self.inflight);
        for h in inflight {
            let _ = self.free(h);
        }
        let handles: Vec<BufferHandle> = self.buffers.keys().map(|k| BufferHandle(*k)).collect();
        for h in handles {
            let _ = self.free(h);
        }
    }
}

impl std::fmt::Debug for NvVfioComputeDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NvVfioComputeDevice")
            .field("sm_version", &self.sm_version)
            .field("compute_class", &self.compute_class)
            .field("buffers", &self.buffers.len())
            .field("gpfifo_put", &self.gpfifo_put)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpfifo_entry_encoding() {
        let addr = 0x1000_u64;
        let size = 64_u32;
        let entry = gpfifo::encode_entry(addr, size);
        let dw0 = entry as u32;
        assert_eq!(dw0, 0x1000, "DW0 = addr with type=0");
        let dw1 = (entry >> 32) as u32;
        let len_field = (dw1 >> 10) & 0x1F_FFFF;
        assert_eq!(len_field, 16, "length = 16 dwords");
        let recovered = (dw0 as u64 & 0xFFFF_FFFC) | ((dw1 as u64 & 0xFF) << 32);
        assert_eq!(recovered, addr);
    }

    #[test]
    fn gpfifo_entry_zero() {
        assert_eq!(gpfifo::encode_entry(0, 0), 0);
    }

    #[test]
    fn gpfifo_ring_size() {
        assert_eq!(gpfifo::RING_SIZE, 128 * 8);
    }

    #[test]
    fn gpfifo_entry_large_addr() {
        let addr = 0x10_0000_0000_u64;
        let size = 256_u32;
        let entry = gpfifo::encode_entry(addr, size);
        let dw0 = entry as u32;
        let dw1 = (entry >> 32) as u32;
        let recovered = (dw0 as u64 & 0xFFFF_FFFC) | ((dw1 as u64 & 0xFF) << 32);
        assert_eq!(recovered, addr);
        let len_field = (dw1 >> 10) & 0x1F_FFFF;
        assert_eq!(len_field, 64, "length = 64 dwords");
    }

    #[test]
    fn iova_constants_non_overlapping() {
        const { assert!(GPFIFO_IOVA < USERD_IOVA) };
        const { assert!(USERD_IOVA + 4096 <= USER_IOVA_BASE) };
    }

    #[test]
    fn open_nonexistent_device() {
        let result = NvVfioComputeDevice::open("9999:99:99.9", 86, 0xC6C0);
        assert!(result.is_err());
    }

    #[test]
    fn local_mem_window_volta() {
        assert_eq!(LOCAL_MEM_WINDOW_VOLTA, 0xFF00_0000_0000_0000);
    }

    #[test]
    fn local_mem_window_legacy() {
        assert_eq!(LOCAL_MEM_WINDOW_LEGACY, 0xFF00_0000);
    }
}
