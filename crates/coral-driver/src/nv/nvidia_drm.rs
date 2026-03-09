// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA proprietary DRM driver backend (`nvidia-drm`).
//!
//! Provides device probing and identification via the nvidia-drm
//! kernel module's DRM render node (`/dev/dri/renderD*`).
//!
//! ## Current capabilities
//!
//! - Device probe + driver identification via `DRM_IOCTL_VERSION`
//! - Render node enumeration alongside other DRM devices
//! - SM86-specific compilation target in `GpuContext::auto()`
//!
//! ## Compute dispatch path
//!
//! The nvidia-drm render node does not support DRM GEM allocation
//! or dumb buffers. Compute dispatch on the proprietary nvidia driver
//! requires the NVIDIA UVM (Unified Virtual Memory) interface:
//!
//! - `/dev/nvidia0` — device management, context creation
//! - `/dev/nvidia-uvm` — GPU virtual memory, buffer allocation
//! - NVIDIA RM (Resource Manager) ioctls — channel submission
//!
//! See [`super::uvm`] for the UVM ioctl definitions and device
//! infrastructure. Compiled SM86 SASS binaries are target-identical
//! regardless of the host driver (nouveau vs nvidia). The compilation
//! pipeline is ready; the dispatch path needs UVM integration testing
//! on a system with the proprietary driver loaded.
//!
//! Feature-gated behind `--features nvidia-drm`.

use crate::drm::DrmDevice;
use crate::error::{DriverError, DriverResult};
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

/// NVIDIA GPU device via the proprietary nvidia-drm DRM module.
///
/// Currently provides device probing and identification. Buffer
/// management and compute dispatch require NVIDIA UVM integration
/// (tracked for future evolution).
pub struct NvDrmDevice {
    drm: DrmDevice,
}

impl NvDrmDevice {
    /// Open the NVIDIA GPU device via the nvidia-drm proprietary DRM module.
    ///
    /// Requires the `nvidia-drm` feature. Finds the first render node with
    /// driver name `"nvidia-drm"`.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if no nvidia-drm render node is found.
    #[cfg(feature = "nvidia-drm")]
    pub fn open() -> DriverResult<Self> {
        let drm = DrmDevice::open_by_driver("nvidia-drm")?;
        tracing::info!(path = %drm.path, "NVIDIA proprietary DRM device opened");
        Ok(Self { drm })
    }

    /// Returns the DRM render node path (e.g. `/dev/dri/renderD129`).
    #[must_use]
    pub fn path(&self) -> &str {
        &self.drm.path
    }

    /// Query the DRM driver name (should be `"nvidia-drm"`).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the version ioctl fails.
    pub fn driver_name(&self) -> DriverResult<String> {
        self.drm.driver_name()
    }
}

impl ComputeDevice for NvDrmDevice {
    fn alloc(&mut self, _size: u64, _domain: MemoryDomain) -> DriverResult<BufferHandle> {
        Err(DriverError::SubmitFailed(
            "nvidia-drm buffer allocation requires UVM integration (not yet implemented)".into(),
        ))
    }

    fn free(&mut self, _handle: BufferHandle) -> DriverResult<()> {
        Err(DriverError::SubmitFailed(
            "nvidia-drm buffer free requires UVM integration (not yet implemented)".into(),
        ))
    }

    fn upload(&mut self, _handle: BufferHandle, _offset: u64, _data: &[u8]) -> DriverResult<()> {
        Err(DriverError::SubmitFailed(
            "nvidia-drm upload requires UVM integration (not yet implemented)".into(),
        ))
    }

    fn readback(&self, _handle: BufferHandle, _offset: u64, _len: usize) -> DriverResult<Vec<u8>> {
        Err(DriverError::SubmitFailed(
            "nvidia-drm readback requires UVM integration (not yet implemented)".into(),
        ))
    }

    fn dispatch(
        &mut self,
        _shader: &[u8],
        _buffers: &[BufferHandle],
        _dims: DispatchDims,
        _info: &ShaderInfo,
    ) -> DriverResult<()> {
        Err(DriverError::SubmitFailed(
            "nvidia-drm compute dispatch requires UVM integration (not yet implemented)".into(),
        ))
    }

    fn sync(&mut self) -> DriverResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn module_compiles() {
        // Existence test — nvidia_drm module is correctly feature-gated.
    }
}
