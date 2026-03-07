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
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain};

use std::collections::HashMap;

/// AMD GPU compute device.
pub struct AmdDevice {
    drm: DrmDevice,
    ctx_handle: u32,
    buffers: HashMap<u32, gem::GemBuffer>,
    next_handle: u32,
}

impl AmdDevice {
    /// Open the AMD GPU device.
    ///
    /// Probes `/dev/dri/renderD*` for an amdgpu driver and creates a
    /// GPU context for compute submission.
    pub fn open() -> DriverResult<Self> {
        let drm = DrmDevice::open_default()?;
        let driver = drm.driver_name()?;
        if driver != "amdgpu" {
            return Err(DriverError::DeviceNotFound(format!(
                "expected amdgpu driver, found '{driver}'"
            )));
        }

        let ctx_handle = ioctl::create_context(drm.fd())?;
        tracing::info!(driver = %driver, ctx = ctx_handle, "AMD GPU context created");

        Ok(Self {
            drm,
            ctx_handle,
            buffers: HashMap::new(),
            next_handle: 1,
        })
    }

    fn alloc_handle(&mut self) -> u32 {
        let h = self.next_handle;
        self.next_handle += 1;
        h
    }
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
    ) -> DriverResult<()> {
        let shader_handle = self.alloc(shader.len() as u64, MemoryDomain::Gtt)?;
        self.upload(shader_handle, 0, shader)?;

        let mut gem_handles: Vec<u32> = Vec::with_capacity(buffers.len() + 1);
        if let Some(gem) = self.buffers.get(&shader_handle.0) {
            gem_handles.push(gem.gem_handle);
        }
        for bh in buffers {
            if let Some(gem) = self.buffers.get(&bh.0) {
                gem_handles.push(gem.gem_handle);
            }
        }

        let pm4_words = pm4::build_compute_dispatch(
            self.buffers
                .get(&shader_handle.0)
                .map(|g| g.gpu_va)
                .unwrap_or(0),
            dims,
        );

        ioctl::submit_command(self.drm.fd(), self.ctx_handle, &gem_handles, &pm4_words)?;

        self.free(shader_handle)?;
        Ok(())
    }

    fn sync(&self) -> DriverResult<()> {
        ioctl::sync_fence(self.drm.fd(), self.ctx_handle)
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
