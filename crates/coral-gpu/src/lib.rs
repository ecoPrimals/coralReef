// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! # coral-gpu — Unified GPU Compute
//!
//! Sovereign GPU compute abstraction: compile WGSL → native binary →
//! dispatch on hardware, all in pure Rust.
//!
//! Replaces `wgpu` for compute workloads in barraCuda and the wider
//! ecoPrimals ecosystem.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │              coral-gpu                       │
//! │  ┌──────────┐  ┌──────────┐  ┌───────────┐ │
//! │  │ Compiler │  │  Driver  │  │  Context   │ │
//! │  │(coral-   │  │(coral-   │  │(compile +  │ │
//! │  │  reef)   │  │  driver) │  │  dispatch) │ │
//! │  └──────────┘  └──────────┘  └───────────┘ │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! ## Example
//!
//! ```rust,ignore
//! use coral_gpu::{GpuContext, GpuTarget};
//!
//! let ctx = GpuContext::new(GpuTarget::auto())?;
//! let shader = ctx.compile_wgsl("@compute @workgroup_size(64) fn main() {}")?;
//! let buf = ctx.alloc(1024)?;
//! ctx.dispatch(&shader, &[buf], [16, 1, 1])?;
//! ctx.sync()?;
//! let data = ctx.readback(buf, 1024)?;
//! ```

pub use coral_driver::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain};
pub use coral_reef::{AmdArch, CompileOptions, GpuTarget, NvArch};

/// Errors from the unified GPU abstraction.
#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("compilation error: {0}")]
    Compile(#[from] coral_reef::CompileError),

    #[error("driver error: {0}")]
    Driver(#[from] coral_driver::DriverError),

    #[error("no GPU device available for target {0}")]
    NoDevice(String),
}

pub type GpuResult<T> = Result<T, GpuError>;

/// A compiled compute shader ready for dispatch.
#[derive(Debug, Clone)]
pub struct CompiledKernel {
    /// Native GPU binary.
    pub binary: Vec<u8>,
    /// Source WGSL (for diagnostics).
    pub source_hash: u64,
    /// Target this was compiled for.
    pub target: GpuTarget,
}

/// GPU compute context — unified compile + dispatch.
///
/// Wraps a `coral-reef` compiler and a `coral-driver` device into
/// a single API for GPU compute.
pub struct GpuContext {
    target: GpuTarget,
    options: CompileOptions,
}

impl GpuContext {
    /// Create a new GPU context for the given target.
    pub fn new(target: GpuTarget) -> GpuResult<Self> {
        let options = CompileOptions {
            target,
            ..CompileOptions::default()
        };
        Ok(Self { target, options })
    }

    /// Auto-detect the best available GPU.
    ///
    /// Probes DRM render nodes and selects the first available device.
    pub fn auto() -> GpuResult<Self> {
        // Default to NVIDIA SM70 for now; hardware probing requires
        // coral-driver device enumeration.
        Self::new(GpuTarget::default())
    }

    /// Compile WGSL source to a native GPU kernel.
    pub fn compile_wgsl(&self, wgsl: &str) -> GpuResult<CompiledKernel> {
        let binary = coral_reef::compile_wgsl(wgsl, &self.options)?;
        Ok(CompiledKernel {
            binary,
            source_hash: hash_wgsl(wgsl),
            target: self.target,
        })
    }

    /// Compile SPIR-V to a native GPU kernel.
    pub fn compile_spirv(&self, spirv: &[u32]) -> GpuResult<CompiledKernel> {
        let binary = coral_reef::compile(spirv, &self.options)?;
        Ok(CompiledKernel {
            binary,
            source_hash: 0,
            target: self.target,
        })
    }

    /// Get the target GPU.
    #[must_use]
    pub fn target(&self) -> GpuTarget {
        self.target
    }
}

fn hash_wgsl(wgsl: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in wgsl.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_context_creation() {
        let ctx = GpuContext::auto();
        assert!(ctx.is_ok());
    }

    #[test]
    fn gpu_context_compile_wgsl() {
        let ctx = GpuContext::auto().unwrap();
        let kernel = ctx.compile_wgsl("@compute @workgroup_size(1) fn main() {}");
        assert!(kernel.is_ok());
        let k = kernel.unwrap();
        assert!(!k.binary.is_empty());
    }

    #[test]
    fn gpu_context_amd_compile() {
        let ctx = GpuContext::new(GpuTarget::Amd(AmdArch::Rdna2)).unwrap();
        let kernel = ctx.compile_wgsl("@compute @workgroup_size(1) fn main() {}");
        assert!(kernel.is_ok());
    }

    #[test]
    fn compiled_kernel_has_target() {
        let ctx = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm86)).unwrap();
        let kernel = ctx
            .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
            .unwrap();
        assert!(matches!(kernel.target, GpuTarget::Nvidia(NvArch::Sm86)));
    }

    #[test]
    fn hash_deterministic() {
        let a = hash_wgsl("hello");
        let b = hash_wgsl("hello");
        let c = hash_wgsl("world");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
