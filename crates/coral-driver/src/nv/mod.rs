// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA GPU driver — nouveau DRM backend.
//!
//! Provides compute shader dispatch via the nouveau kernel driver:
//! - GEM buffer object management
//! - Pushbuf command submission
//! - QMD (Queue Management Descriptor) construction
//! - Fence synchronization (via pushbuf completion)
//!
//! `NvDevice::open()` is feature-gated behind `--features nouveau` to
//! prevent accidental use in environments without a nouveau GPU.

pub mod ioctl;
pub mod pushbuf;
pub mod qmd;

use crate::drm::DrmDevice;
use crate::error::{DriverError, DriverResult};
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use std::collections::HashMap;

/// Default VA space base for kernel-managed allocations (from NVK ioctl trace).
///
/// NVK uses `kernel_managed_addr = 0x80_0000_0000` and `size = 0x80_0000_0000`
/// for the Volta+ VA space.
pub const NV_KERNEL_MANAGED_ADDR: u64 = 0x80_0000_0000;

/// NVIDIA GPU compute device via nouveau.
pub struct NvDevice {
    drm: DrmDevice,
    channel: u32,
    buffers: HashMap<u32, NvBuffer>,
    next_handle: u32,
    /// GEM handle of the last submitted pushbuf (for fence sync).
    last_submit_gem: Option<u32>,
}

/// A nouveau GEM buffer with optional mmap info.
#[derive(Debug)]
pub struct NvBuffer {
    pub gem_handle: u32,
    pub size: u64,
    pub gpu_va: u64,
    pub map_handle: u64,
    pub domain: MemoryDomain,
}

impl NvDevice {
    /// Open the NVIDIA GPU device via nouveau.
    ///
    /// Requires the `nouveau` feature. Without it, this method is not compiled,
    /// preventing accidental use of the incomplete backend in production.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if no nouveau render node is found or
    /// channel creation fails.
    #[cfg(feature = "nouveau")]
    pub fn open() -> DriverResult<Self> {
        let drm = DrmDevice::open_default()?;
        let driver = drm.driver_name()?;
        if driver != "nouveau" {
            return Err(DriverError::DeviceNotFound(
                format!("expected nouveau driver, found '{driver}'").into(),
            ));
        }

        let channel = ioctl::create_channel(drm.fd())?;
        tracing::info!(driver = %driver, channel, "NVIDIA nouveau channel created");

        Ok(Self {
            drm,
            channel,
            buffers: HashMap::new(),
            next_handle: 1,
            last_submit_gem: None,
        })
    }

    fn alloc_handle(&mut self) -> u32 {
        let h = self.next_handle;
        self.next_handle += 1;
        h
    }

    /// Create a minimal `NvDevice` for testing (no channel alloc).
    #[cfg(test)]
    #[expect(dead_code, reason = "available for future hardware integration tests")]
    fn new_for_testing() -> DriverResult<Self> {
        let drm = DrmDevice::open_default()?;
        Ok(Self {
            drm,
            channel: 0,
            buffers: HashMap::new(),
            next_handle: 1,
            last_submit_gem: None,
        })
    }
}

/// Reinterpret a `&[u32]` as `&[u8]` for buffer upload.
fn u32_slice_as_bytes(words: &[u32]) -> &[u8] {
    bytemuck::cast_slice(words)
}

impl ComputeDevice for NvDevice {
    fn alloc(&mut self, size: u64, domain: MemoryDomain) -> DriverResult<BufferHandle> {
        let gem_handle = ioctl::gem_new(self.drm.fd(), size, domain)?;
        let (offset, map_handle) = ioctl::gem_info(self.drm.fd(), gem_handle).unwrap_or((0, 0));
        let gpu_va = offset;

        let handle_id = self.alloc_handle();
        self.buffers.insert(
            handle_id,
            NvBuffer {
                gem_handle,
                size,
                gpu_va,
                map_handle,
                domain,
            },
        );
        Ok(BufferHandle(handle_id))
    }

    fn free(&mut self, handle: BufferHandle) -> DriverResult<()> {
        let buf = self
            .buffers
            .remove(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        crate::drm::gem_close(self.drm.fd(), buf.gem_handle)
    }

    fn upload(&mut self, handle: BufferHandle, offset: u64, data: &[u8]) -> DriverResult<()> {
        let buf = self
            .buffers
            .get(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;

        if offset + data.len() as u64 > buf.size {
            return Err(DriverError::MmapFailed(
                format!(
                    "write out of bounds: offset={offset}, len={}, size={}",
                    data.len(),
                    buf.size
                )
                .into(),
            ));
        }
        let mut region = ioctl::gem_mmap_region(self.drm.fd(), buf.map_handle, buf.size)?;
        let off = usize::try_from(offset)
            .map_err(|_| DriverError::platform_overflow("offset exceeds platform pointer width"))?;
        region.slice_at_mut(off, data.len())?.copy_from_slice(data);
        Ok(())
    }

    fn readback(&self, handle: BufferHandle, offset: u64, len: usize) -> DriverResult<Vec<u8>> {
        let buf = self
            .buffers
            .get(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;

        if offset + len as u64 > buf.size {
            return Err(DriverError::MmapFailed(
                format!(
                    "read out of bounds: offset={offset}, len={len}, size={}",
                    buf.size
                )
                .into(),
            ));
        }
        let region = ioctl::gem_mmap_region(self.drm.fd(), buf.map_handle, buf.size)?;
        let off = usize::try_from(offset)
            .map_err(|_| DriverError::platform_overflow("offset exceeds platform pointer width"))?;
        Ok(region.slice_at(off, len)?.to_vec())
    }

    fn dispatch(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
    ) -> DriverResult<()> {
        let shader_size = u64::try_from(shader.len())
            .map_err(|_| DriverError::platform_overflow("shader size fits in u64"))?;
        let shader_handle = self.alloc(shader_size, MemoryDomain::Gtt)?;
        self.upload(shader_handle, 0, shader)?;

        let shader_va = self.buffers.get(&shader_handle.0).map_or(0, |b| b.gpu_va);

        // Build CBUF bindings from buffer handles: each buffer becomes a CBUF slot
        let mut cbufs = Vec::with_capacity(buffers.len());
        for (i, bh) in buffers.iter().enumerate() {
            if let Some(buf) = self.buffers.get(&bh.0) {
                cbufs.push(qmd::CbufBinding {
                    index: u32::try_from(i)
                        .map_err(|_| DriverError::platform_overflow("CBUF index fits in u32"))?,
                    addr: buf.gpu_va,
                    size: u32::try_from(buf.size).unwrap_or(u32::MAX),
                });
            }
        }

        // Build QMD v2.1 with compiler-derived metadata
        let qmd_params = qmd::QmdParams {
            shader_va,
            grid: dims,
            workgroup: info.workgroup,
            gpr_count: info.gpr_count.max(4),
            shared_mem_bytes: info.shared_mem_bytes,
            barrier_count: info.barrier_count,
            cbufs,
        };
        let qmd_words = qmd::build_qmd_v21(&qmd_params);
        let qmd_bytes = u32_slice_as_bytes(&qmd_words);

        // Upload QMD to GPU memory
        let qmd_size = u64::try_from(qmd_bytes.len())
            .map_err(|_| DriverError::platform_overflow("QMD size fits in u64"))?;
        let qmd_handle = self.alloc(qmd_size, MemoryDomain::Gtt)?;
        self.upload(qmd_handle, 0, qmd_bytes)?;
        let qmd_va = self.buffers.get(&qmd_handle.0).map_or(0, |b| b.gpu_va);

        // Build push buffer: SET_OBJECT + caches + SEND_PCAS with QMD address
        let pb = pushbuf::PushBuf::compute_dispatch(
            pushbuf::class::VOLTA_COMPUTE_A,
            qmd_va,
            0xFF00_0000,
        );
        let pb_bytes = pb.as_bytes();

        // Upload push buffer to GPU memory
        let pb_size = u64::try_from(pb_bytes.len())
            .map_err(|_| DriverError::platform_overflow("pushbuf size fits in u64"))?;
        let pb_handle = self.alloc(pb_size, MemoryDomain::Gtt)?;
        self.upload(pb_handle, 0, pb_bytes)?;
        let pb_gem = self.buffers.get(&pb_handle.0).map_or(0, |b| b.gem_handle);

        // Collect all GEM handles for the BO list
        let mut bo_handles: Vec<u32> = Vec::with_capacity(buffers.len() + 3);
        if let Some(b) = self.buffers.get(&shader_handle.0) {
            bo_handles.push(b.gem_handle);
        }
        if let Some(b) = self.buffers.get(&qmd_handle.0) {
            bo_handles.push(b.gem_handle);
        }
        if let Some(b) = self.buffers.get(&pb_handle.0) {
            bo_handles.push(b.gem_handle);
        }
        for bh in buffers {
            if let Some(b) = self.buffers.get(&bh.0) {
                bo_handles.push(b.gem_handle);
            }
        }

        ioctl::pushbuf_submit(self.drm.fd(), self.channel, pb_gem, 0, pb_size, &bo_handles)?;

        // Track the QMD GEM handle for fence sync (the GPU reads QMD last)
        self.last_submit_gem = self.buffers.get(&qmd_handle.0).map(|b| b.gem_handle);

        self.free(pb_handle)?;
        self.free(qmd_handle)?;
        self.free(shader_handle)?;
        Ok(())
    }

    fn sync(&mut self) -> DriverResult<()> {
        // Wait for the last submitted GEM buffer to become idle via
        // DRM_NOUVEAU_GEM_CPU_PREP. If no dispatch has been issued,
        // sync is a no-op.
        if let Some(gem_handle) = self.last_submit_gem {
            ioctl::gem_cpu_prep(self.drm.fd(), gem_handle)?;
        }
        Ok(())
    }
}

impl Drop for NvDevice {
    fn drop(&mut self) {
        let handles: Vec<BufferHandle> = self.buffers.keys().map(|k| BufferHandle(*k)).collect();
        for h in handles {
            let _ = self.free(h);
        }
        let _ = ioctl::destroy_channel(self.drm.fd(), self.channel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qmd_construction() {
        let qmd = qmd::build_compute_qmd(0x1_0000_0000, DispatchDims::new(64, 1, 1), 256);
        assert_eq!(qmd[1], 64); // CTA_RASTER_WIDTH
        assert_eq!(qmd[2], 1); // CTA_RASTER_HEIGHT
    }

    #[test]
    fn nv_buffer_debug_format() {
        let buf = NvBuffer {
            gem_handle: 1,
            size: 4096,
            gpu_va: 0x1000,
            map_handle: 0x2000,
            domain: MemoryDomain::Vram,
        };
        let s = format!("{buf:?}");
        assert!(s.contains("gem_handle"));
    }
}
