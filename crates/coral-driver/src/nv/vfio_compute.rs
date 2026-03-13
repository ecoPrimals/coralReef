// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA VFIO compute device — direct BAR0/DMA dispatch without kernel driver.
//!
//! Implements [`ComputeDevice`] using the VFIO subsystem:
//! - BAR0 MMIO for register access (GR init, GPFIFO doorbell)
//! - DMA buffers for shader code, QMD, push buffers, and user data
//! - Direct GPFIFO submission via BAR0 USERD write
//!
//! # Prerequisites (provided by toadStool)
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

use crate::error::{DriverError, DriverResult};
use crate::vfio::channel::{VfioChannel, ramuserd};
use crate::vfio::device::{MappedBar, VfioDevice};
use crate::vfio::dma::DmaBuffer;
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use super::pushbuf::PushBuf;
use super::qmd;

use std::borrow::Cow;
use std::collections::HashMap;

/// BAR0 register offsets for NVIDIA GPU.
mod bar0_reg {
    /// Boot0 register — chip identification.
    pub const BOOT0: usize = 0x0000_0000;
}

/// Sync timeout — 5 seconds matches the nouveau/UVM paths.
const SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Spin-loop sleep interval for GPFIFO polling.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_micros(10);

/// GPFIFO configuration constants.
mod gpfifo {
    /// Number of GPFIFO entries (must be power of 2).
    pub const ENTRIES: usize = 128;
    /// Size of each GPFIFO entry in bytes.
    pub const ENTRY_SIZE: usize = 8;
    /// Total GPFIFO ring size in bytes.
    pub const RING_SIZE: usize = ENTRIES * ENTRY_SIZE;

    /// GPFIFO entry: encode an IB (indirect buffer) entry.
    ///
    /// Format (64 bits):
    /// - `[39:0]`  = GPU virtual address >> 2
    /// - `[41:40]` = reserved
    /// - `[72:42]` = length in dwords
    /// - `[73]`    = reserved
    pub fn encode_entry(gpu_addr: u64, len_bytes: u32) -> u64 {
        let addr_field = gpu_addr >> 2;
        let len_dwords = u64::from(len_bytes / 4);
        addr_field | (len_dwords << 42)
    }
}

/// DMA-backed GPU buffer tracked by the VFIO device.
struct VfioBuffer {
    dma: DmaBuffer,
    size: u64,
}

/// NVIDIA compute device via VFIO — direct BAR0 + DMA dispatch.
///
/// This is the sovereign compute path where coralReef owns the entire
/// hardware interaction: no kernel GPU driver (nouveau/nvidia) needed,
/// only the VFIO-IOMMU plumbing (provided by toadStool).
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

/// IOVA base for user DMA allocations — above GPFIFO/USERD.
const USER_IOVA_BASE: u64 = 0x10_0000;

/// GPFIFO ring IOVA.
const GPFIFO_IOVA: u64 = 0x1000;

/// USERD page IOVA.
const USERD_IOVA: u64 = 0x2000;

/// Local memory window address for Volta+ (SM >= 70).
const LOCAL_MEM_WINDOW_VOLTA: u64 = 0xFF00_0000_0000_0000;

/// Local memory window address for pre-Volta (SM < 70).
const LOCAL_MEM_WINDOW_LEGACY: u64 = 0xFF00_0000;

impl NvVfioComputeDevice {
    /// Open an NVIDIA GPU via VFIO and prepare for compute dispatch.
    ///
    /// `bdf` is the PCIe Bus:Device.Function address (e.g. `"0000:01:00.0"`).
    /// `sm_version` is the SM architecture version (e.g. 70 for Volta).
    /// `compute_class` is the NVIDIA compute class constant.
    ///
    /// # Errors
    ///
    /// Returns error if VFIO open, BAR0 mapping, or DMA allocation fails.
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
            0, // channel ID 0
        )?;

        Ok(Self {
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
        })
    }

    /// Allocate a DMA buffer and assign it a new IOVA.
    fn alloc_dma(&mut self, size: usize) -> DriverResult<(BufferHandle, u64)> {
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

    /// Submit a push buffer via GPFIFO.
    ///
    /// Writes a GPFIFO entry pointing to the given IOVA/size, updates
    /// GP_PUT in the USERD page at the correct Volta RAMUSERD offset,
    /// then notifies the GPU via the USERMODE doorbell register.
    fn submit_pushbuf(&mut self, pb_iova: u64, pb_size: u32) -> DriverResult<()> {
        let entry = gpfifo::encode_entry(pb_iova, pb_size);
        let entry_bytes = entry.to_le_bytes();

        let slot = (self.gpfifo_put as usize) % gpfifo::ENTRIES;
        let offset = slot * gpfifo::ENTRY_SIZE;

        let ring = self.gpfifo_ring.as_mut_slice();
        ring[offset..offset + 8].copy_from_slice(&entry_bytes);

        self.gpfifo_put = self.gpfifo_put.wrapping_add(1);

        // Write GP_PUT to RAMUSERD at the Volta-specified offset (0x8C).
        let userd_slice = self.userd.as_mut_slice();
        userd_slice[ramuserd::GP_PUT..ramuserd::GP_PUT + 4]
            .copy_from_slice(&self.gpfifo_put.to_le_bytes());

        // Notify the GPU via NV_USERMODE_NOTIFY_CHANNEL_PENDING — write the
        // channel ID to tell Host that this channel has new work available.
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
    fn poll_gpfifo_completion(&self) -> DriverResult<()> {
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
                return Err(DriverError::FenceTimeout { ms: 5000 });
            }

            std::hint::spin_loop();
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Inner dispatch — builds QMD + pushbuf, submits via GPFIFO.
    fn dispatch_inner(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
        temps: &mut Vec<BufferHandle>,
    ) -> DriverResult<()> {
        let (shader_handle, shader_iova) = self.alloc_dma(shader.len())?;
        temps.push(shader_handle);
        self.upload(shader_handle, 0, shader)?;

        // Build CBUF descriptor for group 0 (same layout as NvDevice).
        let desc_entry_size = 16_usize;
        let desc_buf_size = desc_entry_size * buffers.len().max(1);
        let (desc_handle, desc_iova) = self.alloc_dma(desc_buf_size)?;
        temps.push(desc_handle);

        let mut desc_data = vec![0u8; desc_buf_size];
        for (i, bh) in buffers.iter().enumerate() {
            if let Some(buf) = self.buffers.get(&bh.0) {
                let va = buf.dma.iova();
                let sz = u32::try_from(buf.size).unwrap_or(u32::MAX);
                let off = i * 8;
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "deliberate split into 32-bit halves"
                )]
                {
                    desc_data[off..off + 4].copy_from_slice(&(va as u32).to_le_bytes());
                    desc_data[off + 4..off + 8].copy_from_slice(&((va >> 32) as u32).to_le_bytes());
                }
                let sz_off = off + 8;
                if sz_off + 4 <= desc_data.len() {
                    desc_data[sz_off..sz_off + 4].copy_from_slice(&sz.to_le_bytes());
                }
            }
        }
        self.upload(desc_handle, 0, &desc_data)?;

        let cbufs = vec![qmd::CbufBinding {
            index: 0,
            addr: desc_iova,
            size: u32::try_from(desc_buf_size).unwrap_or(u32::MAX),
        }];

        let qmd_params = qmd::QmdParams {
            shader_va: shader_iova,
            grid: dims,
            workgroup: info.workgroup,
            gpr_count: info.gpr_count.max(4),
            shared_mem_bytes: info.shared_mem_bytes,
            barrier_count: info.barrier_count,
            cbufs,
        };
        let qmd_words = qmd::build_qmd_for_sm(self.sm_version, &qmd_params);
        let qmd_bytes: &[u8] = bytemuck::cast_slice(&qmd_words);

        let (qmd_handle, qmd_iova) = self.alloc_dma(qmd_bytes.len())?;
        temps.push(qmd_handle);
        self.upload(qmd_handle, 0, qmd_bytes)?;

        let local_mem_window = if self.sm_version >= 70 {
            LOCAL_MEM_WINDOW_VOLTA
        } else {
            LOCAL_MEM_WINDOW_LEGACY
        };
        let pb = PushBuf::compute_dispatch(self.compute_class, qmd_iova, local_mem_window);
        let pb_bytes = pb.as_bytes();

        let (pb_handle, pb_iova) = self.alloc_dma(pb_bytes.len())?;
        temps.push(pb_handle);
        self.upload(pb_handle, 0, pb_bytes)?;

        let pb_size = u32::try_from(pb_bytes.len())
            .map_err(|_| DriverError::platform_overflow("pushbuf size fits in u32"))?;
        self.submit_pushbuf(pb_iova, pb_size)?;

        Ok(())
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
        let addr_field = entry & 0xFF_FFFF_FFFF;
        assert_eq!(addr_field, addr >> 2);
        let len_field = (entry >> 42) & 0x7FFF_FFFF;
        assert_eq!(len_field, u64::from(size / 4));
    }

    #[test]
    fn gpfifo_entry_zero() {
        let entry = gpfifo::encode_entry(0, 0);
        assert_eq!(entry, 0);
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
        let recovered_addr = (entry & 0xFF_FFFF_FFFF) << 2;
        assert_eq!(recovered_addr, addr);
    }

    #[test]
    fn iova_constants_non_overlapping() {
        assert!(GPFIFO_IOVA < USERD_IOVA);
        assert!(USERD_IOVA + 4096 <= USER_IOVA_BASE);
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
