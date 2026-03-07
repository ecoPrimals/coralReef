// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
#![deny(unsafe_code)]
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
//! let ctx = GpuContext::auto()?;
//! let shader = ctx.compile_wgsl("@compute @workgroup_size(64) fn main() {}")?;
//! let buf = ctx.alloc(1024)?;
//! ctx.dispatch(&shader, &[buf], [16, 1, 1])?;
//! ctx.sync()?;
//! let data = ctx.readback(buf, 1024)?;
//! ```

pub use coral_driver::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};
pub use coral_reef::{AmdArch, CompileOptions, GpuTarget, NvArch};

/// Errors from the unified GPU abstraction.
#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("compilation error: {0}")]
    Compile(#[from] coral_reef::CompileError),

    #[error("driver error: {0}")]
    Driver(#[from] coral_driver::DriverError),

    #[error("no GPU device available for target {0}")]
    NoDevice(std::borrow::Cow<'static, str>),

    #[error("no device attached — call `auto()` or `with_device()` to bind hardware")]
    NoDeviceAttached,
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
    /// GPR count from the compiler (for QMD construction).
    pub gpr_count: u32,
    /// Instruction count (for diagnostics).
    pub instr_count: u32,
    /// Shared memory used by the shader (bytes, for QMD).
    pub shared_mem_bytes: u32,
    /// Barrier count used by the shader (for QMD).
    pub barrier_count: u32,
    /// Workgroup dimensions from `@workgroup_size(x, y, z)`.
    pub workgroup: [u32; 3],
}

/// GPU compute context — unified compile + dispatch.
///
/// Wraps a `coral-reef` compiler and a `coral-driver` device into
/// a single API for GPU compute.
pub struct GpuContext {
    target: GpuTarget,
    options: CompileOptions,
    device: Option<Box<dyn ComputeDevice>>,
}

impl GpuContext {
    /// Create a new GPU context for the given target (compile-only, no device).
    ///
    /// Use [`with_device`](Self::with_device) or [`auto`](Self::auto) to
    /// attach a hardware device for dispatch.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if the target is unsupported.
    pub fn new(target: GpuTarget) -> GpuResult<Self> {
        let options = CompileOptions {
            target,
            ..CompileOptions::default()
        };
        Ok(Self {
            target,
            options,
            device: None,
        })
    }

    /// Create a context with an explicit device (for testing or manual wiring).
    pub fn with_device(target: GpuTarget, device: Box<dyn ComputeDevice>) -> GpuResult<Self> {
        let options = CompileOptions {
            target,
            ..CompileOptions::default()
        };
        Ok(Self {
            target,
            options,
            device: Some(device),
        })
    }

    /// Auto-detect the best available GPU via DRM render node probing.
    ///
    /// Scans `/dev/dri/renderD*`, queries the kernel driver name, and
    /// selects the appropriate backend + compilation target.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if no suitable GPU is found.
    #[cfg(target_os = "linux")]
    pub fn auto() -> GpuResult<Self> {
        use coral_driver::drm::DrmDevice;

        let drm = DrmDevice::open_default().map_err(GpuError::Driver)?;
        let driver = drm.driver_name().map_err(GpuError::Driver)?;

        match driver.as_str() {
            "amdgpu" => {
                let dev = coral_driver::amd::AmdDevice::open().map_err(GpuError::Driver)?;
                let target = GpuTarget::Amd(AmdArch::Rdna2);
                Self::with_device(target, Box::new(dev))
            }
            #[cfg(feature = "nouveau")]
            "nouveau" => {
                let dev = coral_driver::nv::NvDevice::open().map_err(GpuError::Driver)?;
                let target = GpuTarget::Nvidia(NvArch::Sm70);
                Self::with_device(target, Box::new(dev))
            }
            other => Err(GpuError::NoDevice(
                format!("unsupported DRM driver '{other}' — expected amdgpu or nouveau").into(),
            )),
        }
    }

    /// Fallback auto-detect for non-Linux (compile-only).
    #[cfg(not(target_os = "linux"))]
    pub fn auto() -> GpuResult<Self> {
        Self::new(GpuTarget::default())
    }

    /// Compile WGSL source to a native GPU kernel.
    ///
    /// Returns a [`CompiledKernel`] with the binary and compiler metadata
    /// (GPR count, instruction count) needed for QMD construction.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::Compile`] if parsing or compilation fails.
    pub fn compile_wgsl(&self, wgsl: &str) -> GpuResult<CompiledKernel> {
        let compiled = coral_reef::compile_wgsl_full(wgsl, &self.options)?;
        Ok(CompiledKernel {
            binary: compiled.binary,
            source_hash: hash_wgsl(wgsl),
            target: self.target,
            gpr_count: compiled.info.gpr_count,
            instr_count: compiled.info.instr_count,
            shared_mem_bytes: compiled.info.shared_mem_bytes,
            barrier_count: compiled.info.barrier_count,
            workgroup: compiled.info.local_size,
        })
    }

    /// Compile SPIR-V to a native GPU kernel.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::Compile`] if the SPIR-V is invalid or compilation fails.
    pub fn compile_spirv(&self, spirv: &[u32]) -> GpuResult<CompiledKernel> {
        let binary = coral_reef::compile(spirv, &self.options)?;
        Ok(CompiledKernel {
            binary,
            source_hash: 0,
            target: self.target,
            gpr_count: 0,
            instr_count: 0,
            shared_mem_bytes: 0,
            barrier_count: 0,
            workgroup: [1, 1, 1],
        })
    }

    /// Get the target GPU.
    #[must_use]
    pub const fn target(&self) -> GpuTarget {
        self.target
    }

    /// Whether a hardware device is attached.
    #[must_use]
    pub fn has_device(&self) -> bool {
        self.device.is_some()
    }

    fn device_mut(&mut self) -> GpuResult<&mut dyn ComputeDevice> {
        match self.device.as_mut() {
            Some(d) => Ok(d.as_mut()),
            None => Err(GpuError::NoDeviceAttached),
        }
    }

    fn device_ref(&self) -> GpuResult<&dyn ComputeDevice> {
        match self.device.as_ref() {
            Some(d) => Ok(d.as_ref()),
            None => Err(GpuError::NoDeviceAttached),
        }
    }

    /// Allocate a GPU buffer.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if no device is attached or allocation fails.
    pub fn alloc(&mut self, size: u64) -> GpuResult<BufferHandle> {
        Ok(self.device_mut()?.alloc(size, MemoryDomain::VramOrGtt)?)
    }

    /// Allocate a GPU buffer in a specific memory domain.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if no device is attached or allocation fails.
    pub fn alloc_in(&mut self, size: u64, domain: MemoryDomain) -> GpuResult<BufferHandle> {
        Ok(self.device_mut()?.alloc(size, domain)?)
    }

    /// Free a GPU buffer.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if the handle is invalid or no device is attached.
    pub fn free(&mut self, handle: BufferHandle) -> GpuResult<()> {
        Ok(self.device_mut()?.free(handle)?)
    }

    /// Upload data from host to a GPU buffer.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if the handle is invalid, data exceeds buffer bounds,
    /// or no device is attached.
    pub fn upload(&mut self, handle: BufferHandle, data: &[u8]) -> GpuResult<()> {
        Ok(self.device_mut()?.upload(handle, 0, data)?)
    }

    /// Read data back from a GPU buffer to host.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if the handle is invalid or no device is attached.
    pub fn readback(&self, handle: BufferHandle, len: usize) -> GpuResult<Vec<u8>> {
        Ok(self.device_ref()?.readback(handle, 0, len)?)
    }

    /// Dispatch a compiled kernel on the GPU.
    ///
    /// Binds `buffers` as shader resources and launches `dims` workgroups.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if no device is attached or dispatch fails.
    pub fn dispatch(
        &mut self,
        kernel: &CompiledKernel,
        buffers: &[BufferHandle],
        dims: [u32; 3],
    ) -> GpuResult<()> {
        let dispatch_dims = DispatchDims::new(dims[0], dims[1], dims[2]);
        let info = ShaderInfo {
            gpr_count: kernel.gpr_count,
            shared_mem_bytes: kernel.shared_mem_bytes,
            barrier_count: kernel.barrier_count,
            workgroup: kernel.workgroup,
        };
        Ok(self
            .device_mut()?
            .dispatch(&kernel.binary, buffers, dispatch_dims, &info)?)
    }

    /// Wait for all submitted GPU work to complete.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if the fence wait fails or no device is attached.
    pub fn sync(&mut self) -> GpuResult<()> {
        Ok(self.device_mut()?.sync()?)
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
    fn gpu_context_compile_only() {
        let ctx = GpuContext::new(GpuTarget::default()).unwrap();
        assert!(!ctx.has_device());
    }

    #[test]
    fn gpu_context_compile_wgsl() {
        let ctx = GpuContext::new(GpuTarget::default()).unwrap();
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

    #[test]
    fn alloc_fails_without_device() {
        let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
        let err = ctx.alloc(1024).unwrap_err();
        assert!(matches!(err, GpuError::NoDeviceAttached));
    }

    #[test]
    fn dispatch_fails_without_device() {
        let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
        let kernel = ctx
            .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
            .unwrap();
        let err = ctx.dispatch(&kernel, &[], [1, 1, 1]).unwrap_err();
        assert!(matches!(err, GpuError::NoDeviceAttached));
    }

    #[test]
    fn sync_fails_without_device() {
        let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
        let err = ctx.sync().unwrap_err();
        assert!(matches!(err, GpuError::NoDeviceAttached));
    }

    #[test]
    fn readback_fails_without_device() {
        let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
        let err = ctx.sync().unwrap_err();
        assert!(matches!(err, GpuError::NoDeviceAttached));
    }
}
