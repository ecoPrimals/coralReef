// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
#![warn(missing_docs)]
//! # coral-driver — Sovereign GPU Dispatch
//!
//! Pure Rust userspace GPU driver for compute shader dispatch via Linux DRM.
//! No `*-sys` crates, no FFI — all ioctl structures defined internally.
//!
//! ## Supported backends
//!
//! All backends compile by default. Runtime selection via `DriverPreference`.
//!
//! - **AMD**: `amdgpu` DRM driver — GEM buffers, PM4 command streams, CS submit, fence sync
//! - **NVIDIA (nouveau)**: `nouveau` DRM driver — sovereign path (our channel, QMD, pushbuf)
//! - **NVIDIA (proprietary)**: `nvidia-drm` — compatibility path (probe + pending UVM dispatch)
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────┐
//! │  ComputeDevice trait │  ← vendor-agnostic API
//! ├──────────────────────┤
//! │  AmdDevice           │  ← amdgpu DRM backend
//! │  NvDevice            │  ← nouveau DRM backend
//! └──────────────────────┘
//!          │
//!     ioctl::drm       ← pure Rust ioctl wrappers
//!          │
//!     /dev/dri/renderD* ← Linux DRM subsystem
//! ```

pub mod error;

#[cfg(target_os = "linux")]
pub mod drm;

#[cfg(target_os = "linux")]
pub mod amd;

#[cfg(target_os = "linux")]
pub mod nv;

pub mod gsp;

pub use error::{DriverError, DriverResult};

/// An opaque GPU buffer handle.
///
/// Handles are created by [`ComputeDevice::alloc`] and consumed by other
/// device operations. The raw ID is not exposed — callers cannot forge
/// handles, ensuring the driver owns the validity invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferHandle(pub(crate) u32);

impl BufferHandle {
    /// Create a handle from a raw ID. For mock devices; enable `test-utils` feature.
    #[cfg(feature = "test-utils")]
    #[must_use]
    pub const fn from_id(id: u32) -> Self {
        Self(id)
    }
}

/// GPU memory domain for buffer placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryDomain {
    /// Device-local VRAM (fastest for GPU access).
    Vram,
    /// Host-visible system memory (CPU-accessible).
    Gtt,
    /// Either VRAM or GTT (driver picks based on size/pressure).
    VramOrGtt,
}

/// Compute dispatch dimensions.
#[derive(Debug, Clone, Copy)]
pub struct DispatchDims {
    /// Number of workgroups in the X dimension.
    pub x: u32,
    /// Number of workgroups in the Y dimension.
    pub y: u32,
    /// Number of workgroups in the Z dimension.
    pub z: u32,
}

/// Compiler-derived metadata passed to the driver for QMD construction.
///
/// Without this, the driver must guess register counts and shared memory
/// sizing, leading to incorrect hardware configuration.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShaderInfo {
    /// General-purpose register count (from compiler RA).
    pub gpr_count: u32,
    /// Shared memory in bytes (from shader analysis).
    pub shared_mem_bytes: u32,
    /// Barrier count used by the shader.
    pub barrier_count: u32,
    /// Workgroup size (threads per CTA), from `@workgroup_size`.
    pub workgroup: [u32; 3],
}

impl DispatchDims {
    /// Create dispatch dimensions for a 3D grid.
    #[must_use]
    pub const fn new(x: u32, y: u32, z: u32) -> Self {
        Self { x, y, z }
    }

    /// Create dispatch dimensions for a 1D linear grid (n workgroups in X, 1 in Y and Z).
    #[must_use]
    pub const fn linear(n: u32) -> Self {
        Self { x: n, y: 1, z: 1 }
    }
}

/// Vendor-agnostic GPU compute device.
///
/// Implementations provide the full lifecycle: open device, allocate
/// buffers, upload shader binary, dispatch workgroups, synchronize,
/// and read back results.
pub trait ComputeDevice: Send + Sync {
    /// Allocate a GPU buffer.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if allocation fails (OOM, invalid domain).
    fn alloc(&mut self, size: u64, domain: MemoryDomain) -> DriverResult<BufferHandle>;

    /// Free a GPU buffer.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the handle is invalid or the free ioctl fails.
    fn free(&mut self, handle: BufferHandle) -> DriverResult<()>;

    /// Upload data from host to a GPU buffer.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the buffer handle is invalid or the
    /// write exceeds the buffer bounds.
    fn upload(&mut self, handle: BufferHandle, offset: u64, data: &[u8]) -> DriverResult<()>;

    /// Read data from a GPU buffer to host.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the buffer handle is invalid or the
    /// read exceeds the buffer bounds.
    fn readback(&self, handle: BufferHandle, offset: u64, len: usize) -> DriverResult<Vec<u8>>;

    /// Dispatch a compute shader.
    ///
    /// `shader` is the compiled binary (from `coral-reef`).
    /// `buffers` are the buffer handles bound as shader resources.
    /// `dims` are the workgroup dispatch dimensions (grid size in CTAs).
    /// `info` is compiler-derived metadata (GPR count, shared memory, etc.)
    /// used for QMD construction.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if shader upload, command construction, or
    /// submission fails.
    fn dispatch(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
    ) -> DriverResult<()>;

    /// Wait for all submitted work to complete.
    ///
    /// May free in-flight temporary buffers after the fence signals.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the fence wait fails or times out.
    fn sync(&mut self) -> DriverResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_handle_equality() {
        assert_eq!(BufferHandle(1), BufferHandle(1));
        assert_ne!(BufferHandle(1), BufferHandle(2));
    }

    #[test]
    fn buffer_handle_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(BufferHandle(1));
        set.insert(BufferHandle(2));
        assert!(set.contains(&BufferHandle(1)));
        assert!(!set.contains(&BufferHandle(99)));
    }

    #[test]
    fn dispatch_dims_new() {
        let d = DispatchDims::new(8, 4, 2);
        assert_eq!(d.x, 8);
        assert_eq!(d.y, 4);
        assert_eq!(d.z, 2);
    }

    #[test]
    fn dispatch_dims_linear() {
        let d = DispatchDims::linear(256);
        assert_eq!(d.x, 256);
        assert_eq!(d.y, 1);
        assert_eq!(d.z, 1);
    }

    #[test]
    fn dispatch_dims_debug_format() {
        let d = DispatchDims::new(1, 1, 1);
        let debug = format!("{d:?}");
        assert!(debug.contains("DispatchDims"));
    }

    #[test]
    fn memory_domain_equality() {
        assert_eq!(MemoryDomain::Vram, MemoryDomain::Vram);
        assert_ne!(MemoryDomain::Vram, MemoryDomain::Gtt);
        assert_ne!(MemoryDomain::Gtt, MemoryDomain::VramOrGtt);
    }

    #[test]
    fn memory_domain_debug_format() {
        let domains = [
            MemoryDomain::Vram,
            MemoryDomain::Gtt,
            MemoryDomain::VramOrGtt,
        ];
        for d in domains {
            let debug = format!("{d:?}");
            assert!(!debug.is_empty());
        }
    }
}
