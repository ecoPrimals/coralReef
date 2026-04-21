// SPDX-License-Identifier: AGPL-3.0-or-later
//! Intel GPU driver skeleton — i915/xe DRM backend.
//!
//! Scaffolds the `ComputeDevice` implementation for Intel discrete GPUs
//! (Arc Alchemist / Battlemage) via the i915 or xe kernel driver.
//!
//! # Status
//!
//! **Skeleton** — compile-gates behind `feature = "intel"`. Buffer allocation
//! and dispatch return stub errors. The generation profile and
//! `HardwareCapabilities` are wired so the rest of the ecosystem can
//! route to Intel GPUs without feature-flag changes.
//!
//! # Future work
//!
//! - GEM buffer management via i915/xe ioctls
//! - EU thread dispatch command encoding
//! - SPIR-V / L0 shader loading
//! - Fence synchronization via DRM syncobj

pub mod generation;
pub mod ioctl;

use crate::error::{DriverError, DriverResult};
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use std::collections::HashMap;

/// Intel GPU compute device.
pub struct IntelDevice {
    caps: crate::HardwareCapabilities,
    buffers: HashMap<u32, IntelBuffer>,
    next_handle: u32,
}

struct IntelBuffer {
    data: Vec<u8>,
}

impl std::fmt::Debug for IntelDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntelDevice")
            .field("caps", &self.caps)
            .finish()
    }
}

impl IntelDevice {
    /// Create an Intel compute device for a given generation.
    ///
    /// Currently a host-memory stub — no actual GPU access. Useful for
    /// testing the vendor-agnostic dispatch pipeline with Intel capabilities.
    #[must_use]
    pub fn stub(gfx_ver: u8) -> Self {
        let profile = generation::profile_for_gen(gfx_ver);
        Self {
            caps: profile.to_capabilities(),
            buffers: HashMap::new(),
            next_handle: 1,
        }
    }

    fn alloc_handle(&mut self) -> u32 {
        let h = self.next_handle;
        self.next_handle += 1;
        h
    }
}

impl ComputeDevice for IntelDevice {
    fn alloc(&mut self, size: u64, _domain: MemoryDomain) -> DriverResult<BufferHandle> {
        let h = self.alloc_handle();
        self.buffers.insert(
            h,
            IntelBuffer {
                data: vec![0u8; size as usize],
            },
        );
        Ok(BufferHandle(h))
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
        let start = offset as usize;
        let end = start + data.len();
        if end > buf.data.len() {
            return Err(DriverError::SubmitFailed("upload out of bounds".into()));
        }
        buf.data[start..end].copy_from_slice(data);
        Ok(())
    }

    fn readback(&self, handle: BufferHandle, offset: u64, len: usize) -> DriverResult<Vec<u8>> {
        let buf = self
            .buffers
            .get(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        let start = offset as usize;
        let end = start + len;
        if end > buf.data.len() {
            return Err(DriverError::SubmitFailed("readback out of bounds".into()));
        }
        Ok(buf.data[start..end].to_vec())
    }

    fn dispatch(
        &mut self,
        _shader: &[u8],
        _buffers: &[BufferHandle],
        _dims: DispatchDims,
        _info: &ShaderInfo,
    ) -> DriverResult<()> {
        Err(DriverError::DispatchFailed(
            "Intel GPU dispatch not yet implemented — skeleton driver".into(),
        ))
    }

    fn sync(&mut self) -> DriverResult<()> {
        Ok(())
    }

    fn capabilities(&self) -> &crate::HardwareCapabilities {
        &self.caps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_device_alloc_upload_readback() {
        let mut dev = IntelDevice::stub(12);
        let buf = dev.alloc(64, MemoryDomain::Gtt).unwrap();
        dev.upload(buf, 0, &[1, 2, 3, 4]).unwrap();
        let data = dev.readback(buf, 0, 4).unwrap();
        assert_eq!(data, [1, 2, 3, 4]);
    }

    #[test]
    fn stub_dispatch_returns_error() {
        let mut dev = IntelDevice::stub(12);
        let info = ShaderInfo::default();
        let result = dev.dispatch(&[], &[], DispatchDims::linear(1), &info);
        assert!(result.is_err());
    }

    #[test]
    fn capabilities_are_intel() {
        let dev = IntelDevice::stub(12);
        assert_eq!(dev.capabilities().vendor, crate::hardware::Vendor::Intel);
    }
}
