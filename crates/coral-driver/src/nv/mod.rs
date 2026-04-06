// SPDX-License-Identifier: AGPL-3.0-or-later
//! NVIDIA GPU driver — nouveau (sovereign) and nvidia-drm (compatible) backends.
//!
//! coralReef prefers nouveau because it forces deep sovereignty: we own
//! every ioctl, every channel allocation, every QMD word. But we also
//! support nvidia-drm for pragmatic compatibility with existing deployments.
//!
//! Both backends compile by default. Runtime selection happens via
//! `DriverPreference` in coral-gpu.
//!
//! - **nouveau** (open-source): GEM buffers, pushbuf command submission,
//!   QMD dispatch, fence sync. The sovereign path.
//! - **nvidia-drm** (proprietary): DRM render node access, device probing.
//!   Compute dispatch pending UVM integration. The compatibility path.

pub mod bar0;
pub mod identity;
pub mod ioctl;
pub mod kepler_falcon;
pub mod pushbuf;
pub mod qmd;

#[cfg(feature = "nouveau")]
mod probe;

#[cfg(feature = "nvidia-drm")]
pub mod nvidia_drm;
#[cfg(feature = "nvidia-drm")]
pub use nvidia_drm::NvDrmDevice;

#[cfg(feature = "nvidia-drm")]
pub mod uvm;
#[cfg(feature = "nvidia-drm")]
pub mod uvm_compute;
#[cfg(feature = "nvidia-drm")]
pub use uvm_compute::NvUvmComputeDevice;

#[cfg(feature = "vfio")]
pub mod vfio_compute;
#[cfg(feature = "vfio")]
pub use vfio_compute::{GrEngineStatus, NvVfioComputeDevice, RawVfioDevice};

use crate::drm::DrmDevice;
use crate::error::{DriverError, DriverResult};
use crate::gsp::{self, GrFirmwareBlobs, GrInitSequence};
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use std::collections::HashMap;

/// Kernel-managed VA region base passed to `VM_INIT`.
///
/// `VM_INIT` reserves `[kernel_managed_addr, kernel_managed_addr + size)` for
/// kernel use (page tables, internal objects). Userspace must allocate VA
/// addresses OUTSIDE this range.
pub const NV_KERNEL_MANAGED_ADDR: u64 = 0x80_0000_0000;

/// Userspace VA heap start — below the kernel-managed region.
///
/// Userspace maps GEM buffers here and grows upward. Must stay below
/// `NV_KERNEL_MANAGED_ADDR`. 4 GiB base avoids low-address collisions.
pub const NV_USER_VA_START: u64 = 0x1_0000_0000;

/// GPU page size (4 KiB) — standard for NVIDIA and AMD discrete GPUs.
const GPU_PAGE_SIZE: u64 = 4096;

/// GPU page mask for alignment — `GPU_PAGE_SIZE - 1`.
const GPU_PAGE_MASK: u64 = GPU_PAGE_SIZE - 1;

/// Local memory window address for Volta+ (SM >= 70).
///
/// The shader local memory window tells the GPU where to map per-thread
/// scratch space. Volta uses a 64-bit address space with the window
/// high in virtual memory.
const LOCAL_MEM_WINDOW_VOLTA: u64 = 0xFF00_0000_0000_0000;

/// Local memory window address for pre-Volta (SM < 70).
const LOCAL_MEM_WINDOW_LEGACY: u64 = 0xFF00_0000;

/// NVIDIA GPU compute device via nouveau.
///
/// Supports two dispatch paths:
/// - **New UAPI** (kernel 6.6+): `VM_INIT` → `CHANNEL_ALLOC` → `VM_BIND` → `EXEC`.
///   Required for Volta+ on kernel 6.17+. Uses explicit VA management.
/// - **Legacy UAPI**: `CHANNEL_ALLOC` → `GEM_PUSHBUF`. Works on older kernels
///   where `VM_INIT` is not available.
///
/// The device auto-detects which path to use at open time.
pub struct NvDevice {
    drm: DrmDevice,
    channel: u32,
    compute_class: u32,
    /// Detected SM architecture version (e.g. 70 for Volta, 86 for Ampere).
    sm_version: u32,
    /// Whether the new UAPI (`VM_INIT`/`VM_BIND`/`EXEC`) is active.
    new_uapi: bool,
    /// Next GPU virtual address to allocate (new UAPI only).
    /// Starts at `NV_KERNEL_MANAGED_ADDR` and grows upward.
    next_va: u64,
    buffers: HashMap<u32, NvBuffer>,
    next_handle: u32,
    /// GEM handle of the last submitted pushbuf (for fence sync, legacy UAPI).
    last_submit_gem: Option<u32>,
    /// DRM syncobj handle for new UAPI completion signaling.
    exec_syncobj: Option<u32>,
    /// Temp buffers allocated during dispatch that must survive until sync.
    inflight: Vec<BufferHandle>,
}

/// A nouveau GEM buffer with optional mmap info.
#[derive(Debug)]
pub struct NvBuffer {
    /// Kernel GEM handle for this buffer.
    pub gem_handle: u32,
    /// Buffer size in bytes.
    pub size: u64,
    /// GPU virtual address (for shader dispatch).
    pub gpu_va: u64,
    /// Mmap handle for CPU access (offset for mmap).
    pub map_handle: u64,
    /// Memory domain (VRAM, GTT, or either).
    pub domain: MemoryDomain,
}

impl NvDevice {
    /// Open the NVIDIA GPU device via nouveau with SM auto-detection.
    ///
    /// Probes the GPU identity via sysfs and selects the correct compute
    /// engine class automatically. Falls back to SM70 if detection fails.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if no nouveau render node is found or
    /// channel creation fails.
    #[cfg(feature = "nouveau")]
    pub fn open() -> DriverResult<Self> {
        let drm = DrmDevice::open_by_driver("nouveau")?;
        let sm = ioctl::probe_gpu_identity(&drm.path)
            .and_then(|id| id.nvidia_sm())
            .unwrap_or(70);
        tracing::info!(path = %drm.path, detected_sm = sm, "nouveau SM auto-detected");
        Self::open_from_drm(drm, sm)
    }

    /// Open the NVIDIA GPU device via nouveau, specifying the SM architecture.
    ///
    /// The SM version determines which compute engine class to request from
    /// the kernel (e.g. SM70 → Volta Compute A, SM75 → Turing, SM80+ → Ampere).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if no nouveau render node is found or
    /// channel creation fails.
    #[cfg(feature = "nouveau")]
    pub fn open_with_sm(sm: u32) -> DriverResult<Self> {
        let drm = DrmDevice::open_by_driver("nouveau")?;
        Self::open_from_drm(drm, sm)
    }

    /// Open a specific NVIDIA GPU device by render node path and SM version.
    ///
    /// Use this to target a specific GPU when multiple NVIDIA cards are
    /// present (e.g. `/dev/dri/renderD129`).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the path cannot be opened or channel
    /// creation fails.
    #[cfg(feature = "nouveau")]
    pub fn open_path(path: &str, sm: u32) -> DriverResult<Self> {
        let drm = DrmDevice::open(path)?;
        Self::open_from_drm(drm, sm)
    }

    #[cfg(feature = "nouveau")]
    fn open_from_drm(drm: DrmDevice, sm: u32) -> DriverResult<Self> {
        let compute_class = probe::compute_class_for_sm(sm);

        // Phase 0: Sovereign BAR0 GR init — write PGRAPH registers BEFORE
        // channel creation so the compute engine has valid context state.
        // This replaces the PMU firmware that nouveau lacks on Volta, and
        // supplements GSP on Ampere where the kernel path may be incomplete.
        probe::try_bar0_gr_init(&drm.path, sm);

        // Phase 1: New UAPI probe (kernel 6.6+). On kernel 6.17+ Volta,
        // VM_INIT is required — CHANNEL_ALLOC fails without it.
        let new_uapi = match ioctl::vm_init(drm.fd()) {
            Ok(()) => {
                tracing::info!(
                    path = %drm.path,
                    va_base = format_args!("0x{NV_KERNEL_MANAGED_ADDR:X}"),
                    "nouveau VM_INIT succeeded — using new UAPI"
                );
                true
            }
            Err(e) => {
                tracing::debug!(
                    path = %drm.path,
                    error = %e,
                    "VM_INIT not available — falling back to legacy UAPI"
                );
                false
            }
        };

        // Phase 2: Channel creation (should benefit from BAR0 GR init).
        let channel = match ioctl::create_channel(drm.fd(), compute_class) {
            Ok(ch) => ch,
            Err(e) => {
                tracing::error!(
                    path = %drm.path,
                    compute_class = format_args!("0x{compute_class:04X}"),
                    new_uapi,
                    error = %e,
                    "Channel creation failed — running diagnostics"
                );
                probe::run_open_diagnostics(&drm, sm, compute_class);
                return Err(e);
            }
        };
        tracing::info!(
            path = %drm.path, channel,
            compute_class = format_args!("0x{compute_class:04X}"),
            new_uapi,
            "NVIDIA nouveau channel created with compute subchannel"
        );

        let exec_syncobj = if new_uapi {
            ioctl::syncobj_create(drm.fd()).ok()
        } else {
            None
        };

        let mut dev = Self {
            drm,
            channel,
            compute_class,
            sm_version: sm,
            new_uapi,
            next_va: NV_USER_VA_START,
            buffers: HashMap::new(),
            next_handle: 1,
            last_submit_gem: None,
            exec_syncobj,
            inflight: Vec::new(),
        };

        // Phase 3: Submit any remaining FECS channel methods (low-address
        // entries that can go through the push buffer).
        dev.try_fecs_channel_init();

        Ok(dev)
    }

    /// The compute class this device was opened with.
    #[must_use]
    pub const fn compute_class(&self) -> u32 {
        self.compute_class
    }

    /// The SM architecture version this device targets.
    #[must_use]
    pub const fn sm_version(&self) -> u32 {
        self.sm_version
    }

    const fn alloc_handle(&mut self) -> u32 {
        let h = self.next_handle;
        self.next_handle += 1;
        h
    }

    /// Whether this device uses the new UAPI (`VM_INIT`/`VM_BIND`/`EXEC`).
    #[must_use]
    pub const fn uses_new_uapi(&self) -> bool {
        self.new_uapi
    }

    /// Submit low-address FECS method entries via the channel push buffer.
    ///
    /// This is Phase 3 of device init — runs AFTER BAR0 GR init and channel
    /// creation. Submits only entries with addresses <= 0x7FFC (valid for
    /// 13-bit push buffer method encoding). Most architectures have zero
    /// such entries; the bulk of GR init is BAR0 register writes handled
    /// by [`try_bar0_gr_init`].
    #[cfg(feature = "nouveau")]
    fn try_fecs_channel_init(&mut self) {
        let chip = probe::sm_to_chip(self.sm_version);
        let blobs = match GrFirmwareBlobs::parse(chip) {
            Ok(b) => b,
            Err(e) => {
                tracing::debug!(chip, error = %e, "firmware not available — skipping FECS channel init");
                return;
            }
        };

        let seq = GrInitSequence::for_gv100(&blobs);
        let (_bar0, fecs) = gsp::split_for_application(&seq);

        let channel_methods: Vec<(u32, u32)> = fecs
            .iter()
            .filter(|w| {
                matches!(
                    w.category,
                    gsp::RegCategory::BundleInit | gsp::RegCategory::MethodInit
                )
            })
            .map(|w| (w.offset, w.value))
            .collect();

        if channel_methods.is_empty() {
            tracing::debug!(chip, "no FECS channel methods to submit");
            return;
        }

        let pb = pushbuf::PushBuf::gr_context_init(self.compute_class, &channel_methods);
        let pb_bytes = pb.as_bytes();

        let Ok(pb_size) = u64::try_from(pb_bytes.len()) else {
            tracing::warn!("GR init pushbuf too large — skipping");
            return;
        };

        let pb_handle = match self.alloc(pb_size, MemoryDomain::Gtt) {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(error = %e, "failed to allocate GR init pushbuf");
                return;
            }
        };

        if let Err(e) = self.upload(pb_handle, 0, pb_bytes) {
            tracing::warn!(error = %e, "failed to upload GR init pushbuf");
            let _ = self.free(pb_handle);
            return;
        }

        tracing::info!(
            chip,
            entries = channel_methods.len(),
            "submitting FECS channel methods"
        );

        let submit_result = if self.new_uapi {
            let pb_va = self.buffers.get(&pb_handle.0).map_or(0, |b| b.gpu_va);
            let Ok(push_len) = u32::try_from(pb_size) else {
                tracing::warn!("GR init pushbuf size exceeds u32 — skipping");
                let _ = self.free(pb_handle);
                return;
            };
            if let Some(syncobj) = self.exec_syncobj {
                ioctl::exec_submit_with_signal(
                    self.drm.fd(),
                    self.channel,
                    pb_va,
                    push_len,
                    syncobj,
                )
            } else {
                ioctl::exec_submit(self.drm.fd(), self.channel, pb_va, push_len)
            }
        } else {
            let pb_gem = self.buffers.get(&pb_handle.0).map_or(0, |b| b.gem_handle);
            ioctl::pushbuf_submit(self.drm.fd(), self.channel, pb_gem, 0, pb_size, &[pb_gem])
        };

        match submit_result {
            Ok(()) => {
                tracing::info!(chip, "FECS channel method init submitted");
                if let Some(syncobj) = self.exec_syncobj {
                    if let Err(e) =
                        ioctl::syncobj_wait(self.drm.fd(), syncobj, probe::syncobj_deadline())
                    {
                        tracing::warn!(error = %e, "FECS init syncobj wait failed");
                    }
                } else if let Some(gem) = self.buffers.get(&pb_handle.0).map(|b| b.gem_handle) {
                    let _ = ioctl::gem_cpu_prep(self.drm.fd(), gem);
                }
            }
            Err(e) => {
                tracing::warn!(chip, error = %e, "FECS channel method init failed");
            }
        }

        let _ = self.free(pb_handle);
    }

    /// Create a minimal `NvDevice` for testing (no channel alloc).
    #[cfg(test)]
    #[expect(dead_code, reason = "available for future hardware integration tests")]
    fn new_for_testing() -> DriverResult<Self> {
        let drm = DrmDevice::open_default()?;
        Ok(Self {
            drm,
            channel: 0,
            compute_class: pushbuf::class::VOLTA_COMPUTE_A,
            sm_version: 70,
            new_uapi: false,
            next_va: NV_USER_VA_START,
            buffers: HashMap::new(),
            next_handle: 1,
            last_submit_gem: None,
            exec_syncobj: None,
            inflight: Vec::new(),
        })
    }
}

/// Reinterpret a `&[u32]` as `&[u8]` for buffer upload.
fn u32_slice_as_bytes(words: &[u32]) -> &[u8] {
    bytemuck::cast_slice(words)
}

/// Page-align a size upward to `GPU_PAGE_SIZE` (4 KiB).
const fn page_align(size: u64) -> u64 {
    (size + GPU_PAGE_MASK) & !GPU_PAGE_MASK
}

impl ComputeDevice for NvDevice {
    fn alloc(&mut self, size: u64, domain: MemoryDomain) -> DriverResult<BufferHandle> {
        let aligned_size = page_align(size);
        let gem = ioctl::gem_new(self.drm.fd(), aligned_size, domain)?;

        let (gpu_va, map_handle) = if self.new_uapi {
            // New UAPI: allocate a VA slot and bind the GEM object there.
            let va = self.next_va;
            let next = self
                .next_va
                .checked_add(aligned_size)
                .ok_or_else(|| DriverError::platform_overflow("VA space exhausted"))?;
            if next > NV_KERNEL_MANAGED_ADDR {
                return Err(DriverError::platform_overflow(
                    "user VA heap would collide with kernel-managed region",
                ));
            }
            self.next_va = next;
            ioctl::vm_bind_map(self.drm.fd(), gem.handle, va, 0, aligned_size)?;
            (va, gem.map_handle)
        } else {
            // Legacy UAPI: kernel assigns GPU VA via gem_new offset.
            (gem.offset, gem.map_handle)
        };

        let handle_id = self.alloc_handle();
        self.buffers.insert(
            handle_id,
            NvBuffer {
                gem_handle: gem.handle,
                size: aligned_size,
                gpu_va,
                map_handle,
                domain,
            },
        );
        Ok(BufferHandle(handle_id))
    }

    fn free(&mut self, handle: BufferHandle) -> DriverResult<()> {
        let buf = self
            .buffers
            .remove(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        if self.new_uapi {
            let _ = ioctl::vm_bind_unmap(self.drm.fd(), buf.gpu_va, buf.size);
        }
        crate::drm::gem_close(self.drm.fd(), buf.gem_handle)
    }

    fn upload(&mut self, handle: BufferHandle, offset: u64, data: &[u8]) -> DriverResult<()> {
        let buf = self
            .buffers
            .get(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;

        if offset + data.len() as u64 > buf.size {
            return Err(DriverError::MmapFailed(
                format!(
                    "write out of bounds: offset={offset}, len={}, size={}",
                    data.len(),
                    buf.size
                )
                .into(),
            ));
        }
        let mut region = ioctl::gem_mmap_region(self.drm.fd(), buf.map_handle, buf.size)?;
        let off = usize::try_from(offset)
            .map_err(|_| DriverError::platform_overflow("offset exceeds platform pointer width"))?;
        region.slice_at_mut(off, data.len())?.copy_from_slice(data);
        Ok(())
    }

    fn readback(&self, handle: BufferHandle, offset: u64, len: usize) -> DriverResult<Vec<u8>> {
        let buf = self
            .buffers
            .get(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;

        if offset + len as u64 > buf.size {
            return Err(DriverError::MmapFailed(
                format!(
                    "read out of bounds: offset={offset}, len={len}, size={}",
                    buf.size
                )
                .into(),
            ));
        }
        let region = ioctl::gem_mmap_region(self.drm.fd(), buf.map_handle, buf.size)?;
        let off = usize::try_from(offset)
            .map_err(|_| DriverError::platform_overflow("offset exceeds platform pointer width"))?;
        Ok(region.slice_at(off, len)?.to_vec())
    }

    fn dispatch(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
    ) -> DriverResult<()> {
        // Track temp allocations so we can clean up on error.
        let mut temps: Vec<BufferHandle> = Vec::with_capacity(4);
        let result = self.dispatch_inner(shader, buffers, dims, info, &mut temps);
        if result.is_ok() {
            self.inflight.extend(temps);
        } else {
            for h in temps {
                let _ = self.free(h);
            }
        }
        result
    }

    fn sync(&mut self) -> DriverResult<()> {
        if let Some(syncobj) = self.exec_syncobj {
            // New UAPI: wait on syncobj
            ioctl::syncobj_wait(self.drm.fd(), syncobj, probe::syncobj_deadline())?;
        } else if let Some(gem_handle) = self.last_submit_gem {
            // Legacy UAPI: wait via GEM CPU prep
            ioctl::gem_cpu_prep(self.drm.fd(), gem_handle)?;
        }
        let inflight = std::mem::take(&mut self.inflight);
        for handle in inflight {
            let _ = self.free(handle);
        }
        Ok(())
    }
}

impl NvDevice {
    /// Inner dispatch — separated so the caller can clean up `temps` on error.
    fn dispatch_inner(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
        temps: &mut Vec<BufferHandle>,
    ) -> DriverResult<()> {
        let shader_size = u64::try_from(shader.len())
            .map_err(|_| DriverError::platform_overflow("shader size fits in u64"))?;
        let shader_handle = self.alloc(shader_size, MemoryDomain::Gtt)?;
        temps.push(shader_handle);
        self.upload(shader_handle, 0, shader)?;

        let shader_va = self.buffers.get(&shader_handle.0).map_or(0, |b| b.gpu_va);

        // Build CBUF descriptor buffer for group 0.
        //
        // The compiler (naga_translate/expr.rs) generates CBUF loads like:
        //   addr_lo = c[group][binding * 8]
        //   addr_hi = c[group][binding * 8 + 4]
        //   size    = c[group][binding * 8 + 8]   (for arrayLength)
        //
        // All user buffers are currently in group 0. Each binding needs
        // 12 bytes in the descriptor: [addr_lo, addr_hi, size].
        // We round up to 16 bytes per entry for alignment.
        let desc_entry_size = 16_u64;
        let desc_buf_size = desc_entry_size
            * u64::try_from(buffers.len().max(1))
                .map_err(|_| DriverError::platform_overflow("buffer count fits in u64"))?;
        let desc_handle = self.alloc(desc_buf_size, MemoryDomain::Gtt)?;
        temps.push(desc_handle);

        let desc_len = usize::try_from(desc_buf_size)
            .map_err(|_| DriverError::platform_overflow("descriptor buffer size exceeds usize"))?;
        let mut desc_data = vec![0u8; desc_len];
        for (i, bh) in buffers.iter().enumerate() {
            if let Some(buf) = self.buffers.get(&bh.0) {
                let off = i * 8;
                let va = buf.gpu_va;
                let sz = u32::try_from(buf.size).unwrap_or(u32::MAX);
                let va_lo = u32::try_from(va & 0xFFFF_FFFF).unwrap_or(u32::MAX);
                let va_hi = u32::try_from(va >> 32).unwrap_or(u32::MAX);
                desc_data[off..off + 4].copy_from_slice(&va_lo.to_le_bytes());
                desc_data[off + 4..off + 8].copy_from_slice(&va_hi.to_le_bytes());
                let sz_off = off + 8;
                if sz_off + 4 <= desc_data.len() {
                    desc_data[sz_off..sz_off + 4].copy_from_slice(&sz.to_le_bytes());
                }
            }
        }
        self.upload(desc_handle, 0, &desc_data)?;
        let desc_va = self.buffers.get(&desc_handle.0).map_or(0, |b| b.gpu_va);

        let cbufs = vec![qmd::CbufBinding {
            index: 0,
            addr: desc_va,
            size: u32::try_from(desc_buf_size).unwrap_or(u32::MAX),
        }];

        let qmd_params = qmd::QmdParams {
            shader_va,
            grid: dims,
            workgroup: info.workgroup,
            gpr_count: info.gpr_count.max(4),
            shared_mem_bytes: info.shared_mem_bytes,
            barrier_count: info.barrier_count,
            cbufs,
        };
        let qmd_words = qmd::build_qmd_for_sm(self.sm_version, &qmd_params);
        let qmd_bytes = u32_slice_as_bytes(&qmd_words);

        let qmd_size = u64::try_from(qmd_bytes.len())
            .map_err(|_| DriverError::platform_overflow("QMD size fits in u64"))?;
        let qmd_handle = self.alloc(qmd_size, MemoryDomain::Gtt)?;
        temps.push(qmd_handle);
        self.upload(qmd_handle, 0, qmd_bytes)?;
        let qmd_va = self.buffers.get(&qmd_handle.0).map_or(0, |b| b.gpu_va);

        let local_mem_window = if self.sm_version >= 70 {
            LOCAL_MEM_WINDOW_VOLTA
        } else {
            LOCAL_MEM_WINDOW_LEGACY
        };
        let pb = pushbuf::PushBuf::compute_dispatch(self.compute_class, qmd_va, local_mem_window);
        let pb_bytes = pb.as_bytes();

        let pb_size = u64::try_from(pb_bytes.len())
            .map_err(|_| DriverError::platform_overflow("pushbuf size fits in u64"))?;
        let pb_handle = self.alloc(pb_size, MemoryDomain::Gtt)?;
        temps.push(pb_handle);
        self.upload(pb_handle, 0, pb_bytes)?;
        let pb_gem = self.buffers.get(&pb_handle.0).map_or(0, |b| b.gem_handle);

        if self.new_uapi {
            let pb_va = self.buffers.get(&pb_handle.0).map_or(0, |b| b.gpu_va);
            let push_len = u32::try_from(pb_size)
                .map_err(|_| DriverError::platform_overflow("pushbuf size fits in u32"))?;
            if let Some(syncobj) = self.exec_syncobj {
                ioctl::exec_submit_with_signal(
                    self.drm.fd(),
                    self.channel,
                    pb_va,
                    push_len,
                    syncobj,
                )?;
            } else {
                ioctl::exec_submit(self.drm.fd(), self.channel, pb_va, push_len)?;
            }
        } else {
            let mut bo_handles: Vec<u32> = Vec::with_capacity(buffers.len() + 4);
            if let Some(b) = self.buffers.get(&shader_handle.0) {
                bo_handles.push(b.gem_handle);
            }
            if let Some(b) = self.buffers.get(&qmd_handle.0) {
                bo_handles.push(b.gem_handle);
            }
            if let Some(b) = self.buffers.get(&pb_handle.0) {
                bo_handles.push(b.gem_handle);
            }
            if let Some(b) = self.buffers.get(&desc_handle.0) {
                bo_handles.push(b.gem_handle);
            }
            for bh in buffers {
                if let Some(b) = self.buffers.get(&bh.0) {
                    bo_handles.push(b.gem_handle);
                }
            }
            ioctl::pushbuf_submit(self.drm.fd(), self.channel, pb_gem, 0, pb_size, &bo_handles)?;
        }

        self.last_submit_gem = self.buffers.get(&qmd_handle.0).map(|b| b.gem_handle);
        Ok(())
    }
}

impl Drop for NvDevice {
    fn drop(&mut self) {
        let inflight = std::mem::take(&mut self.inflight);
        for h in inflight {
            let _ = self.free(h);
        }
        let handles: Vec<BufferHandle> = self.buffers.keys().map(|k| BufferHandle(*k)).collect();
        for h in handles {
            let _ = self.free(h);
        }
        if let Some(syncobj) = self.exec_syncobj {
            let _ = ioctl::syncobj_destroy(self.drm.fd(), syncobj);
        }
        let _ = ioctl::destroy_channel(self.drm.fd(), self.channel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qmd_construction() {
        let qmd = qmd::build_compute_qmd(0x1_0000_0000, DispatchDims::new(64, 1, 1), 256);
        // CTA_RASTER_WIDTH at bit 224 = word 7
        assert_eq!(qmd[7], 64);
        // CTA_RASTER_HEIGHT at bit 256 = word 8 lower 16 bits
        assert_eq!(qmd[8] & 0xFFFF, 1);
    }

    #[test]
    fn nv_buffer_debug_format() {
        let buf = NvBuffer {
            gem_handle: 1,
            size: 4096,
            gpu_va: 0x1000,
            map_handle: 0x2000,
            domain: MemoryDomain::Vram,
        };
        let s = format!("{buf:?}");
        assert!(s.contains("gem_handle"));
    }

    #[test]
    fn sm_to_chip_mapping() {
        assert_eq!(probe::sm_to_chip(50), "gm200");
        assert_eq!(probe::sm_to_chip(60), "gp100");
        assert_eq!(probe::sm_to_chip(70), "gv100");
        assert_eq!(probe::sm_to_chip(75), "tu102");
        assert_eq!(probe::sm_to_chip(80), "ga100");
        assert_eq!(probe::sm_to_chip(86), "ga102");
        assert_eq!(probe::sm_to_chip(89), "ad102");
    }

    #[test]
    fn compute_class_selection() {
        assert_eq!(
            probe::compute_class_for_sm(70),
            pushbuf::class::VOLTA_COMPUTE_A
        );
        assert_eq!(
            probe::compute_class_for_sm(75),
            pushbuf::class::TURING_COMPUTE_A
        );
        assert_eq!(
            probe::compute_class_for_sm(86),
            pushbuf::class::AMPERE_COMPUTE_A
        );
    }
}
