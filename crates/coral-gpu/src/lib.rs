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

mod error;
mod preference;

pub use error::{GpuError, GpuResult};
pub use preference::DriverPreference;

use bytes::Bytes;
pub use coral_driver::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};
pub use coral_reef::{AmdArch, CompileOptions, GpuTarget, NvArch};

/// A compiled compute shader ready for dispatch.
///
/// Uses `bytes::Bytes` for the native binary to enable zero-copy sharing
/// across IPC boundaries and between threads.
#[derive(Debug, Clone)]
pub struct CompiledKernel {
    /// Native GPU binary (zero-copy shareable via `Bytes`).
    pub binary: Bytes,
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
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if the target is unsupported.
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
    /// Enumerates ALL `/dev/dri/renderD*` nodes and selects the best
    /// backend according to the [`DriverPreference`] order (read from
    /// `CORALREEF_DRIVER_PREFERENCE` env var, defaulting to sovereign:
    /// `nouveau` > `amdgpu` > `nvidia-drm`).
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if no suitable GPU is found.
    #[cfg(target_os = "linux")]
    pub fn auto() -> GpuResult<Self> {
        Self::auto_with_preference(&DriverPreference::from_env())
    }

    /// Auto-detect with an explicit driver preference order.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if no suitable GPU is found.
    #[cfg(target_os = "linux")]
    pub fn auto_with_preference(pref: &DriverPreference) -> GpuResult<Self> {
        use coral_driver::drm::enumerate_render_nodes;

        let nodes = enumerate_render_nodes();
        if nodes.is_empty() {
            return Err(GpuError::NoDevice("no DRM render nodes found".into()));
        }

        let available: Vec<&str> = nodes.iter().map(|n| n.driver.as_str()).collect();
        let selected = pref.select(&available);

        tracing::info!(
            available = ?available,
            preference = ?pref.order(),
            selected = ?selected,
            "GPU driver selection"
        );

        if let Some(driver) = selected {
            return Self::open_driver(driver);
        }

        Err(GpuError::NoDevice(
            format!(
                "no preferred driver found — available: [{}], preference: [{}]",
                available.join(", "),
                pref.order().join(", ")
            )
            .into(),
        ))
    }

    /// Open a specific driver backend by name.
    #[cfg(target_os = "linux")]
    fn open_driver(driver: &str) -> GpuResult<Self> {
        match driver {
            "amdgpu" => {
                let dev = coral_driver::amd::AmdDevice::open().map_err(GpuError::Driver)?;
                let target = GpuTarget::Amd(AmdArch::Rdna2);
                Self::with_device(target, Box::new(dev))
            }
            #[cfg(feature = "nvidia-drm")]
            "nvidia-drm" => {
                let dev = coral_driver::nv::NvDrmDevice::open().map_err(GpuError::Driver)?;
                let target = GpuTarget::Nvidia(NvArch::Sm86);
                Self::with_device(target, Box::new(dev))
            }
            #[cfg(feature = "nouveau")]
            "nouveau" => {
                let dev = coral_driver::nv::NvDevice::open().map_err(GpuError::Driver)?;
                let target = GpuTarget::Nvidia(NvArch::Sm70);
                Self::with_device(target, Box::new(dev))
            }
            other => Err(GpuError::NoDevice(
                format!("unsupported driver '{other}'").into(),
            )),
        }
    }

    /// Auto-detect all available GPUs and return contexts for each.
    ///
    /// Returns one [`GpuContext`] per supported GPU found on the system.
    /// Unsupported drivers are skipped without error.
    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn enumerate_all() -> Vec<GpuResult<Self>> {
        use coral_driver::drm::enumerate_render_nodes;

        enumerate_render_nodes()
            .iter()
            .filter_map(|info| {
                let result = Self::open_driver(&info.driver);
                match &result {
                    Err(GpuError::NoDevice(_)) => None,
                    _ => Some(result),
                }
            })
            .collect()
    }

    /// Fallback auto-detect for non-Linux (compile-only).
    #[cfg(not(target_os = "linux"))]
    pub fn auto() -> GpuResult<Self> {
        Self::new(GpuTarget::default())
    }

    /// Create a GPU context from a vendor/arch/driver descriptor.
    ///
    /// Used by the toadStool discovery integration: the primal layer
    /// discovers GPU devices via ecosystem IPC and passes descriptors
    /// to coral-gpu for context creation.
    ///
    /// `vendor` should be `"amd"` or `"nvidia"`.
    /// `arch` should be `"rdna2"`, `"sm86"`, etc. (or `None` for default).
    /// `driver` should be `"amdgpu"`, `"nvidia-drm"`, `"nouveau"`, etc.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if the vendor/driver is unsupported or
    /// the device cannot be opened.
    #[cfg(target_os = "linux")]
    pub fn from_descriptor(
        vendor: &str,
        arch: Option<&str>,
        driver: Option<&str>,
    ) -> GpuResult<Self> {
        match (vendor, driver) {
            ("amd", Some("amdgpu") | None) => {
                let target = match arch {
                    Some("rdna3") => GpuTarget::Amd(AmdArch::Rdna3),
                    Some("rdna4") => GpuTarget::Amd(AmdArch::Rdna4),
                    _ => GpuTarget::Amd(AmdArch::Rdna2),
                };
                let dev = coral_driver::amd::AmdDevice::open().map_err(GpuError::Driver)?;
                Self::with_device(target, Box::new(dev))
            }
            #[cfg(feature = "nvidia-drm")]
            ("nvidia", Some("nvidia-drm")) => {
                let target = match arch {
                    Some("sm89") => GpuTarget::Nvidia(NvArch::Sm89),
                    Some("sm80") => GpuTarget::Nvidia(NvArch::Sm80),
                    Some("sm75") => GpuTarget::Nvidia(NvArch::Sm75),
                    Some("sm70") => GpuTarget::Nvidia(NvArch::Sm70),
                    _ => GpuTarget::Nvidia(NvArch::Sm86),
                };
                let dev = coral_driver::nv::NvDrmDevice::open().map_err(GpuError::Driver)?;
                Self::with_device(target, Box::new(dev))
            }
            #[cfg(feature = "nouveau")]
            ("nvidia", Some("nouveau") | None) => {
                let target = match arch {
                    Some("sm86") => GpuTarget::Nvidia(NvArch::Sm86),
                    Some("sm80") => GpuTarget::Nvidia(NvArch::Sm80),
                    Some("sm75") => GpuTarget::Nvidia(NvArch::Sm75),
                    _ => GpuTarget::Nvidia(NvArch::Sm70),
                };
                let sm = match target {
                    GpuTarget::Nvidia(nv) => nv.sm(),
                    _ => 70,
                };
                let dev = coral_driver::nv::NvDevice::open_with_sm(sm).map_err(GpuError::Driver)?;
                Self::with_device(target, Box::new(dev))
            }
            _ => Err(GpuError::NoDevice(
                format!("unsupported vendor/driver: vendor={vendor}, driver={driver:?}").into(),
            )),
        }
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
            binary: Bytes::from(compiled.binary),
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
            binary: Bytes::from(binary),
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
        self.device
            .as_mut()
            .map_or(Err(GpuError::NoDeviceAttached), |d| Ok(d.as_mut()))
    }

    fn device_ref(&self) -> GpuResult<&dyn ComputeDevice> {
        self.device
            .as_ref()
            .map_or(Err(GpuError::NoDeviceAttached), |d| Ok(d.as_ref()))
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

pub(crate) fn hash_wgsl(wgsl: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in wgsl.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests;
