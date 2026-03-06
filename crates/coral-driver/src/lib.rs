// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! # coral-driver — Sovereign GPU Dispatch
//!
//! Pure Rust userspace GPU driver for compute shader dispatch via Linux DRM.
//! No `*-sys` crates, no FFI — all ioctl structures defined internally.
//!
//! ## Supported backends
//!
//! - **AMD**: `amdgpu` DRM driver — GEM buffers, PM4 command streams, SDMA
//! - **NVIDIA**: `nouveau` DRM driver — pushbuf submission, QMD (planned)
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────┐
//! │  ComputeDevice trait │  ← vendor-agnostic API
//! ├──────────────────────┤
//! │  AmdDevice           │  ← amdgpu DRM backend
//! │  NvDevice (planned)  │  ← nouveau DRM backend
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

pub use error::{DriverError, DriverResult};

/// A GPU buffer handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferHandle(pub u32);

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
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

impl DispatchDims {
    #[must_use]
    pub fn new(x: u32, y: u32, z: u32) -> Self {
        Self { x, y, z }
    }

    #[must_use]
    pub fn linear(n: u32) -> Self {
        Self { x: n, y: 1, z: 1 }
    }
}

/// Vendor-agnostic GPU compute device.
///
/// Implementations provide the full lifecycle: open device, allocate
/// buffers, upload shader binary, dispatch workgroups, synchronize,
/// and read back results.
pub trait ComputeDevice {
    /// Allocate a GPU buffer.
    fn alloc(&mut self, size: u64, domain: MemoryDomain) -> DriverResult<BufferHandle>;

    /// Free a GPU buffer.
    fn free(&mut self, handle: BufferHandle) -> DriverResult<()>;

    /// Upload data from host to a GPU buffer.
    fn upload(&mut self, handle: BufferHandle, offset: u64, data: &[u8]) -> DriverResult<()>;

    /// Read data from a GPU buffer to host.
    fn readback(&self, handle: BufferHandle, offset: u64, len: usize) -> DriverResult<Vec<u8>>;

    /// Dispatch a compute shader.
    ///
    /// `shader` is the compiled binary (from `coral-reef`).
    /// `buffers` are the buffer handles bound as shader resources.
    /// `dims` are the workgroup dispatch dimensions.
    fn dispatch(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
    ) -> DriverResult<()>;

    /// Wait for all submitted work to complete.
    fn sync(&self) -> DriverResult<()>;
}
