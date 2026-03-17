// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

use coral_driver::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};
use coral_reef::{AmdArch, CompileOptions, GpuTarget, NvArch};

use crate::error::GpuError;
use crate::fma::FmaCapability;
use crate::hash;
use crate::kernel::{CompiledKernel, KernelCacheEntry};
use crate::preference;
use crate::{GpuResult, driver};

/// GPU compute context — unified compile + dispatch.
///
/// Wraps a `coral-reef` compiler and a `coral-driver` device into
/// a single API for GPU compute.
pub struct GpuContext {
    pub(super) target: GpuTarget,
    pub(super) options: CompileOptions,
    pub(super) device: Option<Box<dyn ComputeDevice>>,
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
    /// [`DriverPreference`](crate::DriverPreference) order (read from `CORALREEF_DRIVER_PREFERENCE`
    /// env var, defaulting to sovereign: `vfio` > `nouveau` > `amdgpu` > `nvidia-drm`).
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if no suitable GPU is found.
    #[cfg(target_os = "linux")]
    pub fn auto() -> GpuResult<Self> {
        Self::auto_with_preference(&preference::DriverPreference::from_env())
    }

    /// Auto-detect with an explicit driver preference order.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if no suitable GPU is found.
    #[cfg(target_os = "linux")]
    pub fn auto_with_preference(pref: &preference::DriverPreference) -> GpuResult<Self> {
        use coral_driver::drm::enumerate_render_nodes;

        let mut available: Vec<String> = Vec::new();

        #[cfg(feature = "vfio")]
        if driver::discover_vfio_nvidia_bdf().is_some() {
            available.push(preference::DRIVER_VFIO.to_string());
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
            preference::DRIVER_VFIO => {
                let bdf = driver::discover_vfio_nvidia_bdf()
                    .ok_or_else(|| GpuError::NoDevice("no VFIO-bound NVIDIA GPU found".into()))?;
                let sm = driver::vfio_detect_sm(&bdf);
                let compute_class = driver::sm_to_compute_class(sm);
                let dev = coral_driver::nv::NvVfioComputeDevice::open(&bdf, sm, compute_class)
                    .map_err(GpuError::Driver)?;
                let target = GpuTarget::Nvidia(driver::sm_to_nvarch(sm));
                Self::with_device(target, Box::new(dev))
            }
            preference::DRIVER_AMDGPU => {
                let dev = coral_driver::amd::AmdDevice::open().map_err(GpuError::Driver)?;
                let target = GpuTarget::Amd(AmdArch::Rdna2);
                Self::with_device(target, Box::new(dev))
            }
            #[cfg(feature = "nvidia-drm")]
            preference::DRIVER_NVIDIA_DRM => {
                if coral_driver::nv::uvm::nvidia_uvm_available() {
                    let sm = driver::sm_from_sysfs_or(driver::DEFAULT_NV_SM);
                    match coral_driver::nv::NvUvmComputeDevice::open(0, sm) {
                        Ok(dev) => {
                            tracing::info!(sm, "nvidia-drm: UVM compute device opened");
                            let target = GpuTarget::Nvidia(driver::sm_to_nvarch(sm));
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
            preference::DRIVER_NOUVEAU => {
                let dev = coral_driver::nv::NvDevice::open().map_err(GpuError::Driver)?;
                let target = GpuTarget::Nvidia(driver::sm_to_nvarch(dev.sm_version()));
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
            preference::DRIVER_AMDGPU => {
                let dev =
                    coral_driver::amd::AmdDevice::open_path(path).map_err(GpuError::Driver)?;
                let target = GpuTarget::Amd(AmdArch::Rdna2);
                Self::with_device(target, Box::new(dev))
            }
            #[cfg(feature = "nvidia-drm")]
            preference::DRIVER_NVIDIA_DRM => {
                let dev =
                    coral_driver::nv::NvDrmDevice::open_path(path).map_err(GpuError::Driver)?;
                let target = driver::sm_target_from_sysfs(path);
                Self::with_device(target, Box::new(dev))
            }
            #[cfg(feature = "nouveau")]
            preference::DRIVER_NOUVEAU => {
                let sm = driver::sm_from_sysfs(path);
                let dev =
                    coral_driver::nv::NvDevice::open_path(path, sm).map_err(GpuError::Driver)?;
                let target = GpuTarget::Nvidia(driver::sm_to_nvarch(sm));
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
            ("amd", Some(preference::DRIVER_AMDGPU) | None) => {
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
            ("nvidia", Some(preference::DRIVER_NVIDIA_DRM)) => {
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
            ("nvidia", Some(preference::DRIVER_NOUVEAU) | None) => {
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
            ("nvidia", Some(preference::DRIVER_VFIO)) => {
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
                let compute_class = driver::sm_to_compute_class(sm);
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
        let sm = driver::vfio_detect_sm(bdf);
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
        let compute_class = driver::sm_to_compute_class(sm);
        let dev = coral_driver::nv::NvVfioComputeDevice::open(bdf, sm, compute_class)
            .map_err(GpuError::Driver)?;
        let target = GpuTarget::Nvidia(driver::sm_to_nvarch(sm));
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
            binary: bytes::Bytes::from(compiled.binary),
            source_hash: hash::hash_wgsl(wgsl),
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
            binary: bytes::Bytes::from(binary),
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
