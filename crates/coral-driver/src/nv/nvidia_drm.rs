// SPDX-License-Identifier: AGPL-3.0-or-later
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
use crate::nv::identity::probe_gpu_identity;
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use super::uvm_compute::NvUvmComputeDevice;

/// Derive the NVIDIA device minor (gpu_index) from a DRM render node path.
///
/// Resolves `/dev/dri/renderD*` → sysfs BDF → `/proc/driver/nvidia/gpus/<BDF>/information`
/// → `Device Minor` field. Falls back to 0 if any lookup fails.
fn nvidia_gpu_index_from_render_node(path: &str) -> u32 {
    let node_name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("renderD128");

    let sysfs_device = format!("/sys/class/drm/{node_name}/device");
    let bdf = match std::fs::read_link(&sysfs_device) {
        Ok(link) => link
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string(),
        Err(_) => return 0,
    };

    if bdf.is_empty() {
        return 0;
    }

    let info_path = format!("/proc/driver/nvidia/gpus/{bdf}/information");
    let info = match std::fs::read_to_string(&info_path) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    for line in info.lines() {
        if let Some(val) = line.strip_prefix("Device Minor:")
            && let Ok(minor) = val.trim().parse::<u32>()
        {
            return minor;
        }
    }
    0
}

/// NVIDIA GPU device via the proprietary nvidia-drm DRM module.
///
/// Provides device probing via DRM render nodes and delegates compute
/// dispatch to [`NvUvmComputeDevice`] for actual GPU work via the
/// proprietary RM + UVM pipeline.
pub struct NvDrmDevice {
    drm: DrmDevice,
    compute: Option<NvUvmComputeDevice>,
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

        let sm = probe_gpu_identity(&drm.path)
            .and_then(|id| id.nvidia_sm())
            .unwrap_or(86);

        let compute = match NvUvmComputeDevice::open(0, sm) {
            Ok(dev) => {
                tracing::info!(sm, "UVM compute device initialized for nvidia-drm");
                Some(dev)
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    sm,
                    "UVM compute device failed to open — dispatch will fail"
                );
                None
            }
        };

        Ok(Self { drm, compute })
    }

    /// Open a specific nvidia-drm device by render node path.
    ///
    /// Use this to target a specific GPU when multiple NVIDIA cards are
    /// present (e.g. `/dev/dri/renderD129`).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the path cannot be opened.
    #[cfg(feature = "nvidia-drm")]
    pub fn open_path(path: &str) -> DriverResult<Self> {
        let drm = DrmDevice::open(path)?;
        tracing::info!(path = %drm.path, "NVIDIA proprietary DRM device opened (by path)");

        let sm = probe_gpu_identity(path)
            .and_then(|id| id.nvidia_sm())
            .unwrap_or(86);

        let gpu_index = nvidia_gpu_index_from_render_node(path);
        tracing::info!(path, gpu_index, sm, "resolved NVIDIA device minor for UVM");

        let compute = match NvUvmComputeDevice::open(gpu_index, sm) {
            Ok(dev) => Some(dev),
            Err(e) => {
                tracing::error!(
                    error = %e,
                    path,
                    gpu_index,
                    "UVM compute device failed for nvidia-drm"
                );
                None
            }
        };

        Ok(Self { drm, compute })
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

impl NvDrmDevice {
    fn compute_mut(&mut self) -> DriverResult<&mut NvUvmComputeDevice> {
        self.compute
            .as_mut()
            .ok_or_else(|| DriverError::DeviceNotFound("UVM compute backend not available".into()))
    }

    fn compute_ref(&self) -> DriverResult<&NvUvmComputeDevice> {
        self.compute
            .as_ref()
            .ok_or_else(|| DriverError::DeviceNotFound("UVM compute backend not available".into()))
    }
}

impl ComputeDevice for NvDrmDevice {
    fn alloc(&mut self, size: u64, domain: MemoryDomain) -> DriverResult<BufferHandle> {
        self.compute_mut()?.alloc(size, domain)
    }

    fn free(&mut self, handle: BufferHandle) -> DriverResult<()> {
        self.compute_mut()?.free(handle)
    }

    fn upload(&mut self, handle: BufferHandle, offset: u64, data: &[u8]) -> DriverResult<()> {
        self.compute_mut()?.upload(handle, offset, data)
    }

    fn readback(&self, handle: BufferHandle, offset: u64, len: usize) -> DriverResult<Vec<u8>> {
        self.compute_ref()?.readback(handle, offset, len)
    }

    fn dispatch(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
    ) -> DriverResult<()> {
        self.compute_mut()?.dispatch(shader, buffers, dims, info)
    }

    fn sync(&mut self) -> DriverResult<()> {
        self.compute
            .as_mut()
            .map_or(Ok(()), NvUvmComputeDevice::sync)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn module_compiles() {
        // Existence test — nvidia_drm module is correctly feature-gated.
    }
}
