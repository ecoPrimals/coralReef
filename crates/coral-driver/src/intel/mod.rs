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
    /// Currently a host-memory emulation — no actual GPU access. Useful for
    /// testing the vendor-agnostic dispatch pipeline with Intel capabilities.
    /// Buffer alloc/upload/readback works in host memory; dispatch builds a
    /// real command batch but returns an error (DRM exec not yet wired).
    #[must_use]
    pub fn host_emulated(gfx_ver: u8) -> Self {
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
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
    ) -> DriverResult<()> {
        let batch = build_compute_batch(shader, buffers, dims, info, &self.buffers);
        tracing::debug!(
            batch_dwords = batch.len(),
            shader_size = shader.len(),
            grid = ?[dims.x, dims.y, dims.z],
            workgroup = ?info.workgroup,
            "Intel dispatch: built batch buffer ({} DWORDs) — \
             requires DRM GEM exec submission to run on hardware",
            batch.len(),
        );
        Err(DriverError::DispatchFailed(
            format!(
                "Intel GPU dispatch: batch built ({} DWORDs) but DRM exec \
                 submission not yet wired — requires i915 GEM_EXECBUFFER2 \
                 or xe XE_EXEC with real GEM BOs",
                batch.len(),
            )
            .into(),
        ))
    }

    fn sync(&mut self) -> DriverResult<()> {
        Ok(())
    }

    fn capabilities(&self) -> &crate::HardwareCapabilities {
        &self.caps
    }
}

/// Build a compute batch buffer for Intel EU dispatch.
///
/// Encodes GPGPU_WALKER + PIPE_CONTROL + MI_BATCH_BUFFER_END into a
/// command stream. On real hardware, this batch would be written into a
/// GEM BO and submitted via i915 `GEM_EXECBUFFER2` or xe `XE_EXEC`.
///
/// Returns the raw DW stream. The shader kernel address and buffer binding
/// table would need to be configured via INTERFACE_DESCRIPTOR_DATA (IDD)
/// loaded before the GPGPU_WALKER command.
fn build_compute_batch(
    _shader: &[u8],
    _buffers: &[BufferHandle],
    dims: DispatchDims,
    info: &ShaderInfo,
    _buffer_map: &HashMap<u32, IntelBuffer>,
) -> Vec<u32> {
    let group_count = [dims.x, dims.y, dims.z];
    let local_size = [
        info.workgroup[0].max(1),
        info.workgroup[1].max(1),
        info.workgroup[2].max(1),
    ];

    let mut batch = Vec::with_capacity(32);

    // GPGPU_WALKER — dispatch compute threads.
    // IDD offset 0: in a real dispatch, the IDD would be pre-loaded via
    // MEDIA_INTERFACE_DESCRIPTOR_LOAD pointing to the kernel binary and
    // binding table in GPU memory.
    let walker = ioctl::compute_cmd::encode_gpgpu_walker(0, group_count, local_size);
    batch.extend_from_slice(&walker);

    // PIPE_CONTROL — flush caches and write a fence value.
    // Fence address 0 is a placeholder; real dispatch would allocate a
    // GEM BO for the fence and use its GPU VA here.
    let fence = ioctl::compute_cmd::encode_pipe_control_fence(0, 1);
    batch.extend_from_slice(&fence);

    // MI_BATCH_BUFFER_END — terminate the batch.
    batch.push(ioctl::compute_cmd::MI_BATCH_BUFFER_END);

    batch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_device_alloc_upload_readback() {
        let mut dev = IntelDevice::host_emulated(12);
        let buf = dev.alloc(64, MemoryDomain::Gtt).unwrap();
        dev.upload(buf, 0, &[1, 2, 3, 4]).unwrap();
        let data = dev.readback(buf, 0, 4).unwrap();
        assert_eq!(data, [1, 2, 3, 4]);
    }

    #[test]
    fn stub_dispatch_returns_error() {
        let mut dev = IntelDevice::host_emulated(12);
        let info = ShaderInfo::default();
        let result = dev.dispatch(&[], &[], DispatchDims::linear(1), &info);
        assert!(result.is_err());
    }

    #[test]
    fn capabilities_are_intel() {
        let dev = IntelDevice::host_emulated(12);
        assert_eq!(dev.capabilities().vendor, crate::hardware::Vendor::Intel);
    }

    #[test]
    fn dispatch_builds_batch_before_failing() {
        let mut dev = IntelDevice::host_emulated(12);
        let info = ShaderInfo {
            workgroup: [64, 1, 1],
            ..ShaderInfo::default()
        };
        let result = dev.dispatch(&[0u8; 32], &[], DispatchDims::linear(4), &info);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("DWORDs"),
            "error should mention batch size: {err}"
        );
    }

    #[test]
    fn build_compute_batch_structure() {
        let dims = DispatchDims { x: 4, y: 2, z: 1 };
        let info = ShaderInfo {
            workgroup: [32, 1, 1],
            ..ShaderInfo::default()
        };
        let batch = build_compute_batch(&[], &[], dims, &info, &HashMap::new());
        // GPGPU_WALKER (15) + PIPE_CONTROL (6) + MI_BATCH_BUFFER_END (1) = 22
        assert_eq!(batch.len(), 22);
        assert_eq!(batch[0] >> 16, ioctl::compute_cmd::GPGPU_WALKER_OPCODE);
        assert_eq!(
            *batch.last().unwrap(),
            ioctl::compute_cmd::MI_BATCH_BUFFER_END
        );
    }
}
