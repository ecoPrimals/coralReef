// SPDX-License-Identifier: AGPL-3.0-only
//! AMD GPU driver — amdgpu DRM backend.
//!
//! Provides compute shader dispatch via the amdgpu kernel driver:
//! - GEM buffer object management
//! - PM4 command buffer construction
//! - DRM command submission
//! - Fence synchronization

pub mod gem;
pub mod ioctl;
pub mod pm4;

use crate::drm::DrmDevice;
use crate::error::{DriverError, DriverResult};
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use std::collections::HashMap;

/// Default GPU fence timeout in nanoseconds (5 seconds).
///
/// Controls how long `sync()` waits for in-flight compute dispatches
/// to complete. Tuned for typical shader workloads; long-running kernels
/// may need a higher value via the future `FenceConfig` evolution.
const FENCE_TIMEOUT_NS: u64 = 5_000_000_000;

/// AMD GPU compute device.
pub struct AmdDevice {
    drm: DrmDevice,
    ctx_handle: u32,
    buffers: HashMap<u32, gem::GemBuffer>,
    next_handle: u32,
    last_fence: u64,
    /// Buffers allocated during dispatch that must survive until sync.
    inflight: Vec<BufferHandle>,
    /// HW IP ring to submit on (default: COMPUTE).
    ip_type: u32,
}

impl AmdDevice {
    /// Open the AMD GPU device.
    ///
    /// Probes `/dev/dri/renderD*` for an amdgpu driver and creates a
    /// GPU context for compute submission.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if no amdgpu render node is found or
    /// context creation fails.
    pub fn open() -> DriverResult<Self> {
        let drm = DrmDevice::open_by_driver("amdgpu")?;
        Self::open_from_drm(drm)
    }

    /// Open a specific AMD GPU device by render node path.
    ///
    /// Use this to target a specific GPU when multiple AMD cards are present
    /// (e.g. `/dev/dri/renderD128`).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the path cannot be opened or context
    /// creation fails.
    pub fn open_path(path: &str) -> DriverResult<Self> {
        let drm = DrmDevice::open(path)?;
        Self::open_from_drm(drm)
    }

    fn open_from_drm(drm: DrmDevice) -> DriverResult<Self> {
        let ctx_handle = ioctl::create_context(drm.fd())?;
        tracing::info!(path = %drm.path, ctx = ctx_handle, "AMD GPU context created");

        Ok(Self {
            drm,
            ctx_handle,
            buffers: HashMap::new(),
            next_handle: 1,
            last_fence: 0,
            inflight: Vec::new(),
            ip_type: ioctl::AMDGPU_HW_IP_COMPUTE,
        })
    }

    const fn alloc_handle(&mut self) -> u32 {
        let h = self.next_handle;
        self.next_handle += 1;
        h
    }

    /// Return the GPU virtual address for a buffer (for diagnostics).
    #[must_use]
    pub fn buffer_gpu_va(&self, handle: BufferHandle) -> Option<u64> {
        self.buffers.get(&handle.0).map(|g| g.gpu_va)
    }

    /// Switch submission ring. Use `ioctl::AMDGPU_HW_IP_GFX` or
    /// `ioctl::AMDGPU_HW_IP_COMPUTE`.
    pub fn set_ip_type(&mut self, ip_type: u32) {
        self.ip_type = ip_type;
    }
}

/// Reinterpret a `&[u32]` as `&[u8]` for buffer upload.
///
/// Uses native-endian byte order (matches GPU expectations on little-endian).
fn u32_slice_as_bytes(words: &[u32]) -> &[u8] {
    bytemuck::cast_slice(words)
}

impl ComputeDevice for AmdDevice {
    fn alloc(&mut self, size: u64, domain: MemoryDomain) -> DriverResult<BufferHandle> {
        let gem = gem::GemBuffer::create(self.drm.fd(), size, domain)?;
        let handle_id = self.alloc_handle();
        self.buffers.insert(handle_id, gem);
        Ok(BufferHandle(handle_id))
    }

    fn free(&mut self, handle: BufferHandle) -> DriverResult<()> {
        let gem = self
            .buffers
            .remove(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        gem.close(self.drm.fd())
    }

    fn upload(&mut self, handle: BufferHandle, offset: u64, data: &[u8]) -> DriverResult<()> {
        let gem = self
            .buffers
            .get(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        gem.write(self.drm.fd(), offset, data)
    }

    fn readback(&self, handle: BufferHandle, offset: u64, len: usize) -> DriverResult<Vec<u8>> {
        let gem = self
            .buffers
            .get(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        gem.read(self.drm.fd(), offset, len)
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

        let shader_va = self.buffers.get(&shader_handle.0).map_or(0, |g| g.gpu_va);

        let buffer_vas: Vec<u64> = buffers
            .iter()
            .filter_map(|bh| self.buffers.get(&bh.0).map(|g| g.gpu_va))
            .collect();
        let pm4_words = pm4::build_compute_dispatch(shader_va, dims, info, &buffer_vas);

        let pm4_bytes = u32_slice_as_bytes(&pm4_words);
        let ib_size = u64::try_from(pm4_bytes.len())
            .map_err(|_| DriverError::platform_overflow("IB size fits in u64"))?;
        let ib_handle = self.alloc(ib_size, MemoryDomain::Gtt)?;
        self.upload(ib_handle, 0, pm4_bytes)?;
        let ib_va = self.buffers.get(&ib_handle.0).map_or(0, |g| g.gpu_va);
        let ib_bytes = u32::try_from(pm4_bytes.len())
            .map_err(|_| DriverError::platform_overflow("IB bytes fit in u32"))?;

        let mut gem_handles: Vec<u32> = Vec::with_capacity(buffers.len() + 2);
        if let Some(gem) = self.buffers.get(&shader_handle.0) {
            gem_handles.push(gem.gem_handle);
        }
        if let Some(gem) = self.buffers.get(&ib_handle.0) {
            gem_handles.push(gem.gem_handle);
        }
        for bh in buffers {
            if let Some(gem) = self.buffers.get(&bh.0) {
                gem_handles.push(gem.gem_handle);
            }
        }

        let bo_list = ioctl::create_bo_list(self.drm.fd(), &gem_handles)?;
        let submit_result =
            ioctl::submit_command_ip(self.drm.fd(), self.ctx_handle, bo_list, ib_va, ib_bytes, self.ip_type);
        let _ = ioctl::destroy_bo_list(self.drm.fd(), bo_list);

        self.last_fence = submit_result?;
        self.inflight.push(ib_handle);
        self.inflight.push(shader_handle);
        Ok(())
    }

    fn sync(&mut self) -> DriverResult<()> {
        if self.last_fence == 0 {
            return Ok(());
        }
        ioctl::sync_fence_ip(
            self.drm.fd(),
            self.ctx_handle,
            self.last_fence,
            FENCE_TIMEOUT_NS,
            self.ip_type,
        )?;
        let inflight = std::mem::take(&mut self.inflight);
        for handle in inflight {
            let _ = self.free(handle);
        }
        Ok(())
    }
}

impl Drop for AmdDevice {
    fn drop(&mut self) {
        let handles: Vec<BufferHandle> = self.buffers.keys().map(|k| BufferHandle(*k)).collect();
        for h in handles {
            let _ = self.free(h);
        }
        let _ = ioctl::destroy_context(self.drm.fd(), self.ctx_handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u32_slice_as_bytes_empty() {
        let words: &[u32] = &[];
        let bytes = u32_slice_as_bytes(words);
        assert!(bytes.is_empty());
    }

    #[test]
    fn u32_slice_as_bytes_single_u32_little_endian() {
        let words: &[u32] = &[0x1234_5678];
        let bytes = u32_slice_as_bytes(words);
        assert_eq!(bytes.len(), 4);
        assert_eq!(bytes[0], 0x78);
        assert_eq!(bytes[1], 0x56);
        assert_eq!(bytes[2], 0x34);
        assert_eq!(bytes[3], 0x12);
    }

    #[test]
    fn u32_slice_as_bytes_multi_word() {
        let words: &[u32] = &[0xDEAD_BEEF, 0xCAFE_BABE];
        let bytes = u32_slice_as_bytes(words);
        assert_eq!(bytes.len(), 8);
        assert_eq!(bytes[0..4], [0xEF, 0xBE, 0xAD, 0xDE]);
        assert_eq!(bytes[4..8], [0xBE, 0xBA, 0xFE, 0xCA]);
    }
}
