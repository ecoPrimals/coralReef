// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA GPU driver — nouveau DRM backend.
//!
//! Provides compute shader dispatch via the nouveau kernel driver:
//! - GEM buffer object management
//! - QMD (Queue Management Descriptor) construction
//! - Pushbuf command submission
//! - Fence synchronization

pub mod ioctl;
pub mod qmd;

use crate::drm::DrmDevice;
use crate::error::{DriverError, DriverResult};
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain};

use std::collections::HashMap;

/// NVIDIA GPU compute device via nouveau.
pub struct NvDevice {
    drm: DrmDevice,
    channel: u32,
    buffers: HashMap<u32, NvBuffer>,
    next_handle: u32,
}

/// A nouveau GEM buffer.
#[derive(Debug)]
pub struct NvBuffer {
    pub gem_handle: u32,
    pub size: u64,
    pub gpu_va: u64,
    pub domain: MemoryDomain,
}

impl NvDevice {
    /// Open the NVIDIA GPU device via nouveau.
    pub fn open() -> DriverResult<Self> {
        let drm = DrmDevice::open_default()?;
        let driver = drm.driver_name()?;
        if driver != "nouveau" {
            return Err(DriverError::DeviceNotFound(format!(
                "expected nouveau driver, found '{driver}'"
            )));
        }

        let channel = ioctl::create_channel(drm.fd())?;
        tracing::info!(driver = %driver, channel, "NVIDIA nouveau channel created");

        Ok(Self {
            drm,
            channel,
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

impl ComputeDevice for NvDevice {
    fn alloc(&mut self, size: u64, domain: MemoryDomain) -> DriverResult<BufferHandle> {
        let gem_handle = ioctl::gem_new(self.drm.fd(), size, domain)?;
        let gpu_va = 0x0002_0000_0000_u64 + u64::from(gem_handle) * 0x1000_0000;

        let handle_id = self.alloc_handle();
        self.buffers.insert(handle_id, NvBuffer {
            gem_handle,
            size,
            gpu_va,
            domain,
        });
        Ok(BufferHandle(handle_id))
    }

    fn free(&mut self, handle: BufferHandle) -> DriverResult<()> {
        let _buf = self.buffers.remove(&handle.0).ok_or(DriverError::BufferNotFound(handle))?;
        // DRM_IOCTL_GEM_CLOSE
        tracing::debug!(handle = _buf.gem_handle, "nouveau GEM close (scaffold)");
        Ok(())
    }

    fn upload(&mut self, _handle: BufferHandle, _offset: u64, _data: &[u8]) -> DriverResult<()> {
        Err(DriverError::Unsupported("nouveau upload not yet implemented".into()))
    }

    fn readback(&self, _handle: BufferHandle, _offset: u64, _len: usize) -> DriverResult<Vec<u8>> {
        Err(DriverError::Unsupported("nouveau readback not yet implemented".into()))
    }

    fn dispatch(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
    ) -> DriverResult<()> {
        let _qmd = qmd::build_compute_qmd(
            0, // shader VA
            dims,
            shader.len() as u32,
        );

        tracing::debug!(
            channel = self.channel,
            buffers = buffers.len(),
            dims_x = dims.x,
            "nouveau compute dispatch (scaffold)"
        );
        Ok(())
    }

    fn sync(&self) -> DriverResult<()> {
        tracing::debug!(channel = self.channel, "nouveau fence sync (scaffold)");
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
