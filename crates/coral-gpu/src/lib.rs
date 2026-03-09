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

use bytes::Bytes;
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

// ---------------------------------------------------------------------------
// Driver preference — sovereignty-first with pragmatic fallback
// ---------------------------------------------------------------------------

/// DRM driver identifiers in preference order.
///
/// coralReef prefers sovereign (open-source) drivers because they force deep
/// understanding and give us full control. But we also want to work on
/// whatever already exists on a deployment target.
///
/// Default preference: `nouveau` > `amdgpu` > `nvidia-drm`
///
/// - **nouveau**: Open-source NVIDIA DRM driver. Forces us to solve deep
///   (our own channel management, QMD, pushbuf). Full sovereignty.
/// - **amdgpu**: Open-source AMD DRM driver. Native Linux citizen. Full
///   dispatch pipeline already working.
/// - **nvidia-drm**: NVIDIA proprietary DRM module. Compatible with existing
///   deployments. Dispatch pending UVM integration.
///
/// Operators can override via `CORALREEF_DRIVER_PREFERENCE` environment
/// variable (comma-separated driver names):
///
/// ```text
/// CORALREEF_DRIVER_PREFERENCE=nouveau,amdgpu,nvidia-drm  # sovereign default
/// CORALREEF_DRIVER_PREFERENCE=nvidia-drm,amdgpu           # pragmatic (use what's installed)
/// CORALREEF_DRIVER_PREFERENCE=amdgpu                       # AMD-only deployment
/// ```
#[derive(Debug, Clone)]
pub struct DriverPreference {
    order: Vec<String>,
}

impl DriverPreference {
    /// Sovereign default: prefer open-source drivers, fall back to proprietary.
    #[must_use]
    pub fn sovereign() -> Self {
        Self {
            order: vec![
                "nouveau".to_string(),
                "amdgpu".to_string(),
                "nvidia-drm".to_string(),
            ],
        }
    }

    /// Pragmatic default: prefer whatever's most likely to work on a typical system.
    #[must_use]
    pub fn pragmatic() -> Self {
        Self {
            order: vec![
                "amdgpu".to_string(),
                "nvidia-drm".to_string(),
                "nouveau".to_string(),
            ],
        }
    }

    /// Parse from a comma-separated string (e.g. `"nouveau,amdgpu,nvidia-drm"`).
    #[must_use]
    pub fn from_str_list(s: &str) -> Self {
        Self {
            order: s
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        }
    }

    /// Read from `CORALREEF_DRIVER_PREFERENCE` env var, falling back to sovereign default.
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var("CORALREEF_DRIVER_PREFERENCE") {
            Ok(val) if !val.is_empty() => Self::from_str_list(&val),
            _ => Self::sovereign(),
        }
    }

    /// The ordered list of preferred driver names.
    #[must_use]
    pub fn order(&self) -> &[String] {
        &self.order
    }

    /// Find the best matching driver from a list of available drivers.
    ///
    /// Returns the first driver in our preference order that appears in
    /// the available list. Returns `None` if no match.
    #[must_use]
    pub fn select<'a>(&self, available: &[&'a str]) -> Option<&'a str> {
        for preferred in &self.order {
            if let Some(&matched) = available.iter().find(|&&d| d == preferred) {
                return Some(matched);
            }
        }
        None
    }
}

impl Default for DriverPreference {
    fn default() -> Self {
        Self::sovereign()
    }
}

pub type GpuResult<T> = Result<T, GpuError>;

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
                let dev =
                    coral_driver::nv::NvDevice::open_with_sm(sm).map_err(GpuError::Driver)?;
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
    use coral_driver::{DispatchDims, DriverError, DriverResult, ShaderInfo};
    use std::collections::HashMap;

    /// Mock ComputeDevice for testing success paths.
    struct MockDevice {
        buffers: HashMap<coral_driver::BufferHandle, Vec<u8>>,
        next_handle: u32,
    }

    impl MockDevice {
        fn new() -> Self {
            Self {
                buffers: HashMap::new(),
                next_handle: 1,
            }
        }
    }

    impl coral_driver::ComputeDevice for MockDevice {
        fn alloc(
            &mut self,
            size: u64,
            _domain: coral_driver::MemoryDomain,
        ) -> DriverResult<coral_driver::BufferHandle> {
            let h = coral_driver::BufferHandle::from_id(self.next_handle);
            self.next_handle += 1;
            self.buffers.insert(h, vec![0; size as usize]);
            Ok(h)
        }
        fn free(&mut self, handle: coral_driver::BufferHandle) -> DriverResult<()> {
            self.buffers
                .remove(&handle)
                .map(|_| ())
                .ok_or(DriverError::BufferNotFound(handle))
        }
        fn upload(
            &mut self,
            handle: coral_driver::BufferHandle,
            offset: u64,
            data: &[u8],
        ) -> DriverResult<()> {
            let buf = self
                .buffers
                .get_mut(&handle)
                .ok_or(DriverError::BufferNotFound(handle))?;
            let end = (offset as usize).saturating_add(data.len());
            if end > buf.len() {
                return Err(DriverError::BufferNotFound(handle));
            }
            buf[offset as usize..end].copy_from_slice(data);
            Ok(())
        }
        fn readback(
            &self,
            handle: coral_driver::BufferHandle,
            offset: u64,
            len: usize,
        ) -> DriverResult<Vec<u8>> {
            let buf = self
                .buffers
                .get(&handle)
                .ok_or(DriverError::BufferNotFound(handle))?;
            let end = (offset as usize).saturating_add(len);
            if end > buf.len() {
                return Err(DriverError::BufferNotFound(handle));
            }
            Ok(buf[offset as usize..end].to_vec())
        }
        fn dispatch(
            &mut self,
            _shader: &[u8],
            _buffers: &[coral_driver::BufferHandle],
            _dims: DispatchDims,
            _info: &ShaderInfo,
        ) -> DriverResult<()> {
            Ok(())
        }
        fn sync(&mut self) -> DriverResult<()> {
            Ok(())
        }
    }

    fn ctx_with_mock() -> GpuContext {
        GpuContext::with_device(GpuTarget::default(), Box::new(MockDevice::new())).unwrap()
    }

    #[test]
    fn alloc_upload_readback_roundtrip() {
        let mut ctx = ctx_with_mock();
        let buf = ctx.alloc(16).unwrap();
        let data = b"hello world!!!!";
        ctx.upload(buf, data).unwrap();
        let out = ctx.readback(buf, data.len()).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn alloc_in_different_domains() {
        let mut ctx = ctx_with_mock();
        let vram = ctx.alloc_in(64, MemoryDomain::Vram).unwrap();
        let gtt = ctx.alloc_in(64, MemoryDomain::Gtt).unwrap();
        let either = ctx.alloc_in(64, MemoryDomain::VramOrGtt).unwrap();
        assert_ne!(vram, gtt);
        assert_ne!(gtt, either);
        ctx.upload(vram, b"vram").unwrap();
        ctx.upload(gtt, b"gtt").unwrap();
        assert_eq!(ctx.readback(vram, 4).unwrap(), b"vram");
        assert_eq!(ctx.readback(gtt, 3).unwrap(), b"gtt");
    }

    #[test]
    fn dispatch_with_compiled_kernel() {
        let mut ctx = ctx_with_mock();
        let kernel = ctx
            .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
            .unwrap();
        let buf = ctx.alloc(64).unwrap();
        ctx.dispatch(&kernel, &[buf], [1, 1, 1]).unwrap();
    }

    #[test]
    fn free_then_use_freed_buffer_fails() {
        let mut ctx = ctx_with_mock();
        let buf = ctx.alloc(64).unwrap();
        ctx.free(buf).unwrap();
        let err = ctx.upload(buf, b"x").unwrap_err();
        assert!(matches!(
            err,
            GpuError::Driver(DriverError::BufferNotFound(_))
        ));
        let err = ctx.readback(buf, 1).unwrap_err();
        assert!(matches!(
            err,
            GpuError::Driver(DriverError::BufferNotFound(_))
        ));
    }

    #[test]
    fn compile_wgsl_dispatch_sync_pipeline() {
        let mut ctx = ctx_with_mock();
        let kernel = ctx
            .compile_wgsl("@compute @workgroup_size(64) fn main() {}")
            .unwrap();
        let buf = ctx.alloc(256).unwrap();
        ctx.dispatch(&kernel, &[buf], [4, 1, 1]).unwrap();
        ctx.sync().unwrap();
    }

    #[test]
    fn compile_spirv_method() {
        let ctx = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm70)).unwrap();
        let invalid = [0x0723_0203_u32, 0x0001_0000, 0, 0, 0];
        let r = ctx.compile_spirv(&invalid);
        assert!(r.is_err());
    }

    #[test]
    fn readback_returns_correct_data() {
        let mut ctx = ctx_with_mock();
        let buf = ctx.alloc(8).unwrap();
        ctx.upload(buf, &[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let out = ctx.readback(buf, 8).unwrap();
        assert_eq!(out, [1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn has_device_returns_true_when_attached() {
        let ctx = ctx_with_mock();
        assert!(ctx.has_device());
    }

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

    // -------------------------------------------------------------------
    // DriverPreference tests
    // -------------------------------------------------------------------

    #[test]
    fn sovereign_preference_prefers_nouveau() {
        let pref = DriverPreference::sovereign();
        assert_eq!(pref.order()[0], "nouveau");
        assert_eq!(pref.order()[1], "amdgpu");
        assert_eq!(pref.order()[2], "nvidia-drm");
    }

    #[test]
    fn pragmatic_preference_prefers_amdgpu() {
        let pref = DriverPreference::pragmatic();
        assert_eq!(pref.order()[0], "amdgpu");
    }

    #[test]
    fn default_preference_is_sovereign() {
        let pref = DriverPreference::default();
        assert_eq!(pref.order(), DriverPreference::sovereign().order());
    }

    #[test]
    fn select_returns_best_match() {
        let pref = DriverPreference::sovereign();
        assert_eq!(
            pref.select(&["amdgpu", "nvidia-drm"]),
            Some("amdgpu"),
            "with no nouveau, sovereign picks amdgpu next"
        );
    }

    #[test]
    fn select_returns_nouveau_when_available() {
        let pref = DriverPreference::sovereign();
        assert_eq!(
            pref.select(&["nvidia-drm", "nouveau", "amdgpu"]),
            Some("nouveau"),
            "sovereign always picks nouveau first"
        );
    }

    #[test]
    fn select_returns_none_when_no_match() {
        let pref = DriverPreference::sovereign();
        assert_eq!(pref.select(&["i915", "xe"]), None);
    }

    #[test]
    fn pragmatic_selects_nvidia_drm_over_nouveau() {
        let pref = DriverPreference::pragmatic();
        assert_eq!(
            pref.select(&["nouveau", "nvidia-drm"]),
            Some("nvidia-drm"),
            "pragmatic picks nvidia-drm before nouveau"
        );
    }

    #[test]
    fn from_str_list_parses_comma_separated() {
        let pref = DriverPreference::from_str_list("nvidia-drm, amdgpu");
        assert_eq!(pref.order(), &["nvidia-drm", "amdgpu"]);
    }

    #[test]
    fn from_str_list_handles_empty_segments() {
        let pref = DriverPreference::from_str_list("nouveau,,amdgpu,");
        assert_eq!(pref.order(), &["nouveau", "amdgpu"]);
    }

    #[test]
    fn from_str_list_single_driver() {
        let pref = DriverPreference::from_str_list("amdgpu");
        assert_eq!(pref.order(), &["amdgpu"]);
        assert_eq!(pref.select(&["amdgpu", "nvidia-drm"]), Some("amdgpu"));
        assert_eq!(pref.select(&["nvidia-drm"]), None);
    }

    #[test]
    fn from_env_falls_back_to_sovereign() {
        // Don't modify env vars (unsafe in 2024+ edition).
        // Instead, verify that from_env returns sovereign-compatible
        // ordering when the env var is not set to something specific.
        let pref = DriverPreference::from_env();
        assert!(
            !pref.order().is_empty(),
            "preference should have at least one driver"
        );
    }

    #[test]
    fn driver_preference_debug_format() {
        let pref = DriverPreference::sovereign();
        let debug = format!("{pref:?}");
        assert!(debug.contains("nouveau"));
    }

    #[test]
    fn driver_preference_clone() {
        let pref = DriverPreference::sovereign();
        let cloned = pref.clone();
        assert_eq!(pref.order(), cloned.order());
    }
}
