// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
#![deny(unsafe_code)]
#![warn(missing_docs)]
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
//! ```no_run
//! # fn main() -> Result<(), coral_gpu::GpuError> {
//! use coral_gpu::{GpuContext, GpuTarget};
//!
//! let mut ctx = GpuContext::auto()?;
//! let shader = ctx.compile_wgsl("@compute @workgroup_size(64) fn main() {}")?;
//! let mut buf = ctx.alloc(1024)?;
//! ctx.dispatch(&shader, &[buf], [16, 1, 1])?;
//! ctx.sync()?;
//! let _data = ctx.readback(buf, 1024)?;
//! # Ok(())
//! # }
//! ```

mod error;
mod preference;

pub use error::{GpuError, GpuResult};
pub use preference::DriverPreference;

use bytes::Bytes;
pub use coral_driver::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};
pub use coral_reef::{AmdArch, CompileOptions, FmaPolicy, GpuTarget, NvArch};

/// Default NVIDIA SM architecture for sysfs-based fallback detection.
///
/// SM 86 (Ampere, GA102) is the default because it covers RTX 3090/3080/3070
/// which are the most common sovereign compute GPUs.
const DEFAULT_NV_SM: u32 = 86;

/// Default NVIDIA SM for the nouveau sovereign path when sysfs detection fails.
const DEFAULT_NV_SM_NOUVEAU: u32 = 70;

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

/// Serializable kernel cache entry for `dispatch_binary` / cached dispatch.
///
/// Produced by [`CompiledKernel::to_cache_entry`], consumed by
/// [`CompiledKernel::from_cache_entry`]. Separates the binary from
/// metadata so that callers can cache across sessions without
/// recompilation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KernelCacheEntry {
    /// Native GPU binary.
    pub binary: Vec<u8>,
    /// Target identifier string (e.g. `"nvidia:sm86"`, `"amd:rdna2"`).
    pub target_id: String,
    /// GPR count.
    pub gpr_count: u32,
    /// Instruction count.
    pub instr_count: u32,
    /// Shared memory in bytes.
    pub shared_mem_bytes: u32,
    /// Barrier count.
    pub barrier_count: u32,
    /// Workgroup size `[x, y, z]`.
    pub workgroup: [u32; 3],
    /// Hash of the source WGSL.
    pub source_hash: u64,
}

impl CompiledKernel {
    /// Convert to a serializable cache entry for on-disk persistence.
    #[must_use]
    pub fn to_cache_entry(&self) -> KernelCacheEntry {
        KernelCacheEntry {
            binary: self.binary.to_vec(),
            target_id: format!("{}:{}", self.target.vendor(), self.target.arch_name()),
            gpr_count: self.gpr_count,
            instr_count: self.instr_count,
            shared_mem_bytes: self.shared_mem_bytes,
            barrier_count: self.barrier_count,
            workgroup: self.workgroup,
            source_hash: self.source_hash,
        }
    }

    /// Reconstruct from a cache entry. `target` must match the `target_id`
    /// in the entry — caller is responsible for validation.
    #[must_use]
    pub fn from_cache_entry(entry: &KernelCacheEntry, target: GpuTarget) -> Self {
        Self {
            binary: Bytes::from(entry.binary.clone()),
            source_hash: entry.source_hash,
            target,
            gpr_count: entry.gpr_count,
            instr_count: entry.instr_count,
            shared_mem_bytes: entry.shared_mem_bytes,
            barrier_count: entry.barrier_count,
            workgroup: entry.workgroup,
        }
    }
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
    /// Enumerates ALL `/dev/dri/renderD*` nodes (and VFIO groups if `vfio`
    /// feature is enabled) and selects the best backend according to the
    /// [`DriverPreference`] order (read from `CORALREEF_DRIVER_PREFERENCE`
    /// env var, defaulting to sovereign: `vfio` > `nouveau` > `amdgpu` > `nvidia-drm`).
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

        let mut available: Vec<String> = Vec::new();

        #[cfg(feature = "vfio")]
        if discover_vfio_nvidia_bdf().is_some() {
            available.push("vfio".to_string());
        }

        let nodes = enumerate_render_nodes();
        for node in &nodes {
            if !available.iter().any(|a| a == &node.driver) {
                available.push(node.driver.clone());
            }
        }

        if available.is_empty() {
            return Err(GpuError::NoDevice("no GPU devices found".into()));
        }

        let available_refs: Vec<&str> = available.iter().map(String::as_str).collect();
        let selected = pref.select(&available_refs);

        tracing::info!(
            available = ?available_refs,
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

    /// Open a specific driver backend by name (first matching render node).
    #[cfg(target_os = "linux")]
    fn open_driver(driver: &str) -> GpuResult<Self> {
        match driver {
            #[cfg(feature = "vfio")]
            "vfio" => {
                let bdf = discover_vfio_nvidia_bdf()
                    .ok_or_else(|| GpuError::NoDevice("no VFIO-bound NVIDIA GPU found".into()))?;
                let sm = vfio_detect_sm(&bdf);
                let compute_class = sm_to_compute_class(sm);
                let dev = coral_driver::nv::NvVfioComputeDevice::open(&bdf, sm, compute_class)
                    .map_err(GpuError::Driver)?;
                let target = GpuTarget::Nvidia(sm_to_nvarch(sm));
                Self::with_device(target, Box::new(dev))
            }
            "amdgpu" => {
                let dev = coral_driver::amd::AmdDevice::open().map_err(GpuError::Driver)?;
                let target = GpuTarget::Amd(AmdArch::Rdna2);
                Self::with_device(target, Box::new(dev))
            }
            #[cfg(feature = "nvidia-drm")]
            "nvidia-drm" => {
                if coral_driver::nv::uvm::nvidia_uvm_available() {
                    let sm = sm_from_sysfs_or(DEFAULT_NV_SM);
                    match coral_driver::nv::NvUvmComputeDevice::open(0, sm) {
                        Ok(dev) => {
                            tracing::info!(sm, "nvidia-drm: UVM compute device opened");
                            let target = GpuTarget::Nvidia(sm_to_nvarch(sm));
                            return Self::with_device(target, Box::new(dev));
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                "nvidia-drm: UVM init failed, falling back to DRM-only"
                            );
                        }
                    }
                }
                let dev = coral_driver::nv::NvDrmDevice::open().map_err(GpuError::Driver)?;
                let target = GpuTarget::Nvidia(NvArch::Sm86);
                Self::with_device(target, Box::new(dev))
            }
            #[cfg(feature = "nouveau")]
            "nouveau" => {
                let dev = coral_driver::nv::NvDevice::open().map_err(GpuError::Driver)?;
                let target = GpuTarget::Nvidia(sm_to_nvarch(dev.sm_version()));
                Self::with_device(target, Box::new(dev))
            }
            other => Err(GpuError::NoDevice(
                format!("unsupported driver '{other}'").into(),
            )),
        }
    }

    /// Open a driver backend by name, targeting a specific render node path.
    #[cfg(target_os = "linux")]
    fn open_driver_at_path(driver: &str, path: &str) -> GpuResult<Self> {
        match driver {
            "amdgpu" => {
                let dev =
                    coral_driver::amd::AmdDevice::open_path(path).map_err(GpuError::Driver)?;
                let target = GpuTarget::Amd(AmdArch::Rdna2);
                Self::with_device(target, Box::new(dev))
            }
            #[cfg(feature = "nvidia-drm")]
            "nvidia-drm" => {
                let dev =
                    coral_driver::nv::NvDrmDevice::open_path(path).map_err(GpuError::Driver)?;
                let target = sm_target_from_sysfs(path);
                Self::with_device(target, Box::new(dev))
            }
            #[cfg(feature = "nouveau")]
            "nouveau" => {
                let sm = sm_from_sysfs(path);
                let dev =
                    coral_driver::nv::NvDevice::open_path(path, sm).map_err(GpuError::Driver)?;
                let target = GpuTarget::Nvidia(sm_to_nvarch(sm));
                Self::with_device(target, Box::new(dev))
            }
            other => Err(GpuError::NoDevice(
                format!("unsupported driver '{other}'").into(),
            )),
        }
    }

    /// Auto-detect all available GPUs and return contexts for each.
    ///
    /// Returns one [`GpuContext`] per supported render node found on the system.
    /// Each render node is opened by its specific path, so multiple GPUs with
    /// the same driver (e.g. 4x RTX 3050) produce distinct contexts targeting
    /// distinct physical devices.
    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn enumerate_all() -> Vec<GpuResult<Self>> {
        use coral_driver::drm::enumerate_render_nodes;

        enumerate_render_nodes()
            .iter()
            .filter_map(|info| {
                let result = Self::open_driver_at_path(&info.driver, &info.path);
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
        Self::from_descriptor_with_path(vendor, arch, driver, None)
    }

    /// Create a GPU context from a descriptor, optionally targeting a specific
    /// render node path.
    ///
    /// When `render_node` is `Some`, opens that specific device. When `None`,
    /// falls back to the first matching driver.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if the vendor/driver is unsupported or the device
    /// cannot be opened.
    #[cfg(target_os = "linux")]
    pub fn from_descriptor_with_path(
        vendor: &str,
        arch: Option<&str>,
        driver: Option<&str>,
        render_node: Option<&str>,
    ) -> GpuResult<Self> {
        match (vendor, driver) {
            ("amd", Some("amdgpu") | None) => {
                let target = match arch {
                    Some("rdna3") => GpuTarget::Amd(AmdArch::Rdna3),
                    Some("rdna4") => GpuTarget::Amd(AmdArch::Rdna4),
                    _ => GpuTarget::Amd(AmdArch::Rdna2),
                };
                let dev = render_node
                    .map_or_else(
                        coral_driver::amd::AmdDevice::open,
                        coral_driver::amd::AmdDevice::open_path,
                    )
                    .map_err(GpuError::Driver)?;
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
                let dev = render_node
                    .map_or_else(
                        coral_driver::nv::NvDrmDevice::open,
                        coral_driver::nv::NvDrmDevice::open_path,
                    )
                    .map_err(GpuError::Driver)?;
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
                let dev = render_node
                    .map_or_else(
                        || coral_driver::nv::NvDevice::open_with_sm(sm),
                        |path| coral_driver::nv::NvDevice::open_path(path, sm),
                    )
                    .map_err(GpuError::Driver)?;
                Self::with_device(target, Box::new(dev))
            }
            #[cfg(feature = "vfio")]
            ("nvidia", Some("vfio")) => {
                let bdf = render_node.ok_or_else(|| {
                    GpuError::NoDevice("VFIO requires a BDF address as render_node".into())
                })?;
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
                let compute_class = sm_to_compute_class(sm);
                let dev = coral_driver::nv::NvVfioComputeDevice::open(bdf, sm, compute_class)
                    .map_err(GpuError::Driver)?;
                Self::with_device(target, Box::new(dev))
            }
            _ => Err(GpuError::NoDevice(
                format!("unsupported vendor/driver: vendor={vendor}, driver={driver:?}").into(),
            )),
        }
    }

    /// Open a VFIO-bound NVIDIA GPU by PCI Bus:Device.Function address.
    ///
    /// This is the primary entry point for sovereign VFIO compute. The SM
    /// version is auto-detected from sysfs; use [`from_vfio_with_sm`](Self::from_vfio_with_sm)
    /// for an explicit override.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # fn main() -> Result<(), coral_gpu::GpuError> {
    /// let ctx = coral_gpu::GpuContext::from_vfio("0000:01:00.0")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if VFIO open, BAR0 mapping, or DMA setup fails.
    #[cfg(all(target_os = "linux", feature = "vfio"))]
    pub fn from_vfio(bdf: &str) -> GpuResult<Self> {
        let sm = vfio_detect_sm(bdf);
        Self::from_vfio_with_sm(bdf, sm)
    }

    /// Open a VFIO-bound NVIDIA GPU with an explicit SM version.
    ///
    /// Use this when sysfs detection is unavailable or you need to target
    /// a specific SM architecture (e.g. testing SM 70 paths on newer hardware).
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if VFIO open, BAR0 mapping, or DMA setup fails.
    #[cfg(all(target_os = "linux", feature = "vfio"))]
    pub fn from_vfio_with_sm(bdf: &str, sm: u32) -> GpuResult<Self> {
        let compute_class = sm_to_compute_class(sm);
        let dev = coral_driver::nv::NvVfioComputeDevice::open(bdf, sm, compute_class)
            .map_err(GpuError::Driver)?;
        let target = GpuTarget::Nvidia(sm_to_nvarch(sm));
        Self::with_device(target, Box::new(dev))
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

    /// Query FMA hardware capabilities for this context's target.
    #[must_use]
    pub fn fma_capability(&self) -> FmaCapability {
        FmaCapability::for_target(self.target)
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

    /// Dispatch a pre-compiled native binary from a [`KernelCacheEntry`].
    ///
    /// This is the `dispatch_binary` entry point for cached kernel dispatch:
    /// the binary was compiled once (via `compile_wgsl`) and can be reused
    /// across sessions without recompilation.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if no device is attached or dispatch fails.
    pub fn dispatch_precompiled(
        &mut self,
        entry: &KernelCacheEntry,
        buffers: &[BufferHandle],
        dims: [u32; 3],
    ) -> GpuResult<()> {
        let dispatch_dims = DispatchDims::new(dims[0], dims[1], dims[2]);
        let info = ShaderInfo {
            gpr_count: entry.gpr_count,
            shared_mem_bytes: entry.shared_mem_bytes,
            barrier_count: entry.barrier_count,
            workgroup: entry.workgroup,
        };
        Ok(self
            .device_mut()?
            .dispatch(&entry.binary, buffers, dispatch_dims, &info)?)
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

/// FMA (fused multiply-add) hardware capability for a GPU target.
///
/// Reports whether the hardware supports FMA, and what precision behavior
/// to expect. Springs use this to decide between `FmaPolicy::Fused` (fast)
/// and `FmaPolicy::Separate` (bit-exact CPU parity).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FmaCapability {
    /// Hardware supports f32 FMA.
    pub f32_fma: bool,
    /// Hardware supports f64 FMA (DFMA).
    pub f64_fma: bool,
    /// Recommended FMA policy for numerical precision.
    pub recommended_policy: FmaPolicy,
    /// FMA throughput relative to separate mul+add (1.0 = same speed).
    /// Values > 1.0 mean FMA is faster than separate operations.
    pub f32_fma_throughput_ratio: f32,
}

impl FmaCapability {
    /// Query FMA capabilities for a given GPU target.
    ///
    /// Derives from architecture specifications — does not require
    /// a live device connection.
    #[must_use]
    pub fn for_target(target: GpuTarget) -> Self {
        match target {
            GpuTarget::Nvidia(nv) => Self::nvidia(nv),
            GpuTarget::Amd(amd) => Self::amd(amd),
            _ => Self {
                f32_fma: true,
                f64_fma: false,
                recommended_policy: FmaPolicy::Auto,
                f32_fma_throughput_ratio: 1.0,
            },
        }
    }

    fn nvidia(nv: NvArch) -> Self {
        Self {
            f32_fma: true,
            f64_fma: nv.has_dfma(),
            recommended_policy: FmaPolicy::Auto,
            // NVIDIA FMA is on the same pipeline as separate mul+add
            f32_fma_throughput_ratio: 2.0,
        }
    }

    fn amd(amd: AmdArch) -> Self {
        Self {
            f32_fma: true,
            f64_fma: amd.has_native_f64(),
            recommended_policy: FmaPolicy::Auto,
            // AMD RDNA: v_fma_f32 is VOP3 (1 cycle), same as v_mul + v_add
            f32_fma_throughput_ratio: 2.0,
        }
    }
}

/// PCIe topology information for multi-GPU device grouping.
///
/// Used by `shader.compile.wgsl.multi` to communicate device affinity.
/// Devices on the same PCIe switch have lower inter-device latency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcieDeviceInfo {
    /// Render node path (e.g. `/dev/dri/renderD128`).
    pub render_node: String,
    /// PCIe bus address (e.g. `0000:01:00.0`).
    pub pcie_address: Option<String>,
    /// PCIe switch group (devices sharing a switch get the same ID).
    pub switch_group: Option<u32>,
    /// GPU target architecture.
    pub target: GpuTarget,
}

/// Probe PCIe topology for all available GPU render nodes.
///
/// Reads sysfs to discover render nodes, their PCIe addresses, and
/// groups them by shared PCIe switch (based on common bus prefix).
#[cfg(target_os = "linux")]
#[must_use]
pub fn probe_pcie_topology() -> Vec<PcieDeviceInfo> {
    let dri_path = std::path::Path::new("/dev/dri");
    let mut devices = Vec::new();

    let entries = match std::fs::read_dir(dri_path) {
        Ok(e) => e,
        Err(_) => return devices,
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("renderD") {
            continue;
        }

        let render_path = format!("/dev/dri/{name_str}");
        let sysfs_device = format!("/sys/class/drm/{name_str}/device");

        let pcie_address = std::fs::read_link(&sysfs_device)
            .ok()
            .and_then(|link| link.file_name().map(|n| n.to_string_lossy().into_owned()));

        let vendor = std::fs::read_to_string(format!("{sysfs_device}/vendor"))
            .ok()
            .and_then(|v| u16::from_str_radix(v.trim().trim_start_matches("0x"), 16).ok());

        let target = match vendor {
            Some(coral_driver::nv::identity::PCI_VENDOR_NVIDIA) => {
                let sm = sm_from_sysfs(&render_path);
                GpuTarget::Nvidia(sm_to_nvarch(sm))
            }
            Some(coral_driver::nv::identity::PCI_VENDOR_AMD) => GpuTarget::Amd(AmdArch::Rdna2),
            _ => continue,
        };

        devices.push(PcieDeviceInfo {
            render_node: render_path,
            pcie_address,
            switch_group: None,
            target,
        });
    }

    assign_switch_groups(&mut devices);
    devices
}

/// Group devices by shared PCIe switch based on bus address prefix.
#[cfg(target_os = "linux")]
fn assign_switch_groups(devices: &mut [PcieDeviceInfo]) {
    let mut group_map: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut next_group = 0u32;

    for device in devices.iter_mut() {
        if let Some(ref addr) = device.pcie_address {
            let prefix = addr.split(':').take(2).collect::<Vec<_>>().join(":");
            let group = *group_map.entry(prefix).or_insert_with(|| {
                let g = next_group;
                next_group += 1;
                g
            });
            device.switch_group = Some(group);
        }
    }
}

/// Map an SM version number to the corresponding `NvArch`.
#[cfg(target_os = "linux")]
const fn sm_to_nvarch(sm: u32) -> NvArch {
    match sm {
        75 => NvArch::Sm75,
        80 => NvArch::Sm80,
        86 => NvArch::Sm86,
        89 => NvArch::Sm89,
        _ => NvArch::Sm70,
    }
}

/// Detect the NVIDIA SM version from any available render node.
/// Falls back to the provided default if detection fails.
#[cfg(all(target_os = "linux", feature = "nvidia-drm"))]
fn sm_from_sysfs_or(default: u32) -> u32 {
    use coral_driver::drm::enumerate_render_nodes;
    for node in enumerate_render_nodes() {
        if node.driver == "nvidia-drm" {
            return coral_driver::nv::ioctl::probe_gpu_identity(&node.path)
                .and_then(|id| id.nvidia_sm())
                .unwrap_or(default);
        }
    }
    default
}

/// Detect the NVIDIA SM version from sysfs for a render node path.
/// Falls back to `DEFAULT_NV_SM_NOUVEAU` if detection fails.
#[cfg(target_os = "linux")]
fn sm_from_sysfs(path: &str) -> u32 {
    coral_driver::nv::ioctl::probe_gpu_identity(path)
        .and_then(|id| id.nvidia_sm())
        .unwrap_or(DEFAULT_NV_SM_NOUVEAU)
}

/// Detect the GPU target from sysfs for an nvidia-drm render node.
/// Falls back to `DEFAULT_NV_SM` if detection fails.
#[cfg(all(target_os = "linux", feature = "nvidia-drm"))]
fn sm_target_from_sysfs(path: &str) -> GpuTarget {
    let sm = coral_driver::nv::ioctl::probe_gpu_identity(path)
        .and_then(|id| id.nvidia_sm())
        .unwrap_or(DEFAULT_NV_SM);
    GpuTarget::Nvidia(sm_to_nvarch(sm))
}

/// Map an SM version to the NVIDIA compute class constant.
#[cfg(all(target_os = "linux", feature = "vfio"))]
const fn sm_to_compute_class(sm: u32) -> u32 {
    match sm {
        70..=74 => coral_driver::nv::pushbuf::class::VOLTA_COMPUTE_A,
        75..=79 => coral_driver::nv::pushbuf::class::TURING_COMPUTE_A,
        _ => coral_driver::nv::pushbuf::class::AMPERE_COMPUTE_A,
    }
}

/// Discover a VFIO-bound NVIDIA GPU by scanning sysfs for `vfio-pci` bindings.
///
/// Returns the first BDF address of an NVIDIA GPU bound to `vfio-pci`, or `None`.
#[cfg(all(target_os = "linux", feature = "vfio"))]
fn discover_vfio_nvidia_bdf() -> Option<String> {
    let vfio_dir = std::path::Path::new("/sys/bus/pci/drivers/vfio-pci");
    let entries = std::fs::read_dir(vfio_dir).ok()?;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let bdf = name.to_string_lossy();
        if !bdf.contains(':') {
            continue;
        }

        let vendor_path = format!("/sys/bus/pci/devices/{bdf}/vendor");
        if let Ok(vendor_str) = std::fs::read_to_string(&vendor_path) {
            let vendor_str = vendor_str.trim().trim_start_matches("0x");
            if let Ok(vendor) = u16::from_str_radix(vendor_str, 16)
                && vendor == coral_driver::nv::identity::PCI_VENDOR_NVIDIA
            {
                tracing::info!(bdf = %bdf, "discovered VFIO-bound NVIDIA GPU");
                return Some(bdf.into_owned());
            }
        }
    }
    None
}

/// Detect SM version for a VFIO-bound GPU from sysfs device ID.
#[cfg(all(target_os = "linux", feature = "vfio"))]
fn vfio_detect_sm(bdf: &str) -> u32 {
    let device_path = format!("/sys/bus/pci/devices/{bdf}/device");
    let device_id = std::fs::read_to_string(&device_path)
        .ok()
        .and_then(|s| u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok());

    match device_id {
        Some(0x1D81) => 70,                            // Titan V
        Some(0x1E00..=0x1E8F) => 75,                   // Turing (TU10x)
        Some(0x2200..=0x2203 | 0x2207..=0x22FF) => 80, // GA100
        Some(0x2204..=0x2206) => 86,                   // GA102 (RTX 3090/3080)
        Some(0x2300..=0x23FF) => 86,                   // GA10x
        Some(0x2400..=0x26FF) => 89,                   // Ada Lovelace
        _ => DEFAULT_NV_SM,
    }
}

/// FNV-1a 64-bit offset basis.
const FNV1A_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const FNV1A_PRIME: u64 = 0x0100_0000_01b3;

/// Compute a fast non-cryptographic hash of WGSL source (FNV-1a 64-bit).
pub(crate) fn hash_wgsl(wgsl: &str) -> u64 {
    let mut hash = FNV1A_OFFSET_BASIS;
    for byte in wgsl.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV1A_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests;
