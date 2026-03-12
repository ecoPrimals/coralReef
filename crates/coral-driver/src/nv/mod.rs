// SPDX-License-Identifier: AGPL-3.0-only
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

pub mod identity;
pub mod ioctl;
pub mod pushbuf;
pub mod qmd;

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

use crate::drm::DrmDevice;
use crate::error::{DriverError, DriverResult};
use crate::gsp::GrFirmwareBlobs;
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use std::collections::HashMap;

/// Kernel-managed VA region base passed to VM_INIT.
///
/// VM_INIT reserves `[kernel_managed_addr, kernel_managed_addr + size)` for
/// kernel use (page tables, internal objects). Userspace must allocate VA
/// addresses OUTSIDE this range.
pub const NV_KERNEL_MANAGED_ADDR: u64 = 0x80_0000_0000;

/// Userspace VA heap start — below the kernel-managed region.
///
/// Userspace maps GEM buffers here and grows upward. Must stay below
/// `NV_KERNEL_MANAGED_ADDR`. 4 GiB base avoids low-address collisions.
pub const NV_USER_VA_START: u64 = 0x1_0000_0000;

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
    /// Whether the new UAPI (VM_INIT/VM_BIND/EXEC) is active.
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

/// Select the compute engine class for a GPU architecture.
///
/// Returns the DRM class ID that the kernel needs to instantiate a compute
/// engine on this GPU generation.
const fn compute_class_for_sm(sm: u32) -> u32 {
    match sm {
        75 => pushbuf::class::TURING_COMPUTE_A,
        80..=89 => pushbuf::class::AMPERE_COMPUTE_A,
        _ => pushbuf::class::VOLTA_COMPUTE_A,
    }
}

/// Map SM architecture version to the chip codename used by firmware paths.
///
/// e.g. SM 70 → `"gv100"` (Volta), SM 75 → `"tu102"` (Turing),
/// SM 86 → `"ga102"` (Ampere), SM 89 → `"ad102"` (Ada).
const fn sm_to_chip(sm: u32) -> &'static str {
    match sm {
        50..=52 => "gm200",
        60..=62 => "gp100",
        70 => "gv100",
        75 => "tu102",
        80 => "ga100",
        86..=87 => "ga102",
        89 => "ad102",
        _ => "gv100",
    }
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
        let compute_class = compute_class_for_sm(sm);

        // Try new UAPI first (kernel 6.6+). On kernel 6.17+ Volta, this is
        // required — CHANNEL_ALLOC fails without VM_INIT.
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
                run_open_diagnostics(&drm, sm, compute_class);
                return Err(e);
            }
        };
        tracing::info!(
            path = %drm.path, channel,
            compute_class = format_args!("0x{compute_class:04X}"),
            new_uapi,
            "NVIDIA nouveau channel created with compute subchannel"
        );

        // Create a syncobj for new UAPI completion tracking.
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

        dev.try_gr_context_init();

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
        match self.compute_class {
            pushbuf::class::TURING_COMPUTE_A => 75,
            pushbuf::class::AMPERE_COMPUTE_A => 86,
            _ => 70,
        }
    }

    const fn alloc_handle(&mut self) -> u32 {
        let h = self.next_handle;
        self.next_handle += 1;
        h
    }

    /// Whether this device uses the new UAPI (VM_INIT/VM_BIND/EXEC).
    #[must_use]
    pub const fn uses_new_uapi(&self) -> bool {
        self.new_uapi
    }

    /// Attempt GR context initialization from firmware knowledge.
    ///
    /// Parses the GPU's firmware blobs from `/lib/firmware/nvidia/{chip}/gr/`
    /// and logs the available init data for diagnostics. Only submits method
    /// entries that are valid push buffer method offsets (< 0x8000) — most
    /// `sw_method_init.bin` entries are BAR0 register addresses
    /// (0x00400000+) that cannot be submitted via the channel and require
    /// BAR0 MMIO access (toadStool's nvPmu).
    ///
    /// The kernel's nouveau driver normally handles GR context init during
    /// channel creation. If it doesn't (CTXNOTVALID), the fix is in the
    /// kernel driver or via BAR0 MMIO, not via channel method submission.
    #[cfg(feature = "nouveau")]
    fn try_gr_context_init(&mut self) {
        let chip = sm_to_chip(self.sm_version);
        let blobs = match GrFirmwareBlobs::parse(chip) {
            Ok(b) => b,
            Err(e) => {
                tracing::debug!(
                    chip,
                    error = %e,
                    "GR firmware not available — skipping FECS init"
                );
                return;
            }
        };

        // Push buffer method headers encode method>>2 in 13 bits (max 0x7FFC).
        // sw_method_init.bin entries with addresses >= 0x8000 are BAR0 register
        // writes that must go via MMIO, not through the channel.
        const MAX_PUSHBUF_METHOD: u32 = 0x7FFC;

        let channel_methods: Vec<(u32, u32)> = blobs
            .method_init
            .iter()
            .filter(|m| m.addr <= MAX_PUSHBUF_METHOD)
            .map(|m| (m.addr, m.value))
            .collect();

        let bar0_count = blobs.method_init.len() - channel_methods.len();

        tracing::info!(
            chip,
            total_method_entries = blobs.method_init.len(),
            channel_submittable = channel_methods.len(),
            bar0_register_writes = bar0_count,
            bundle_init_entries = blobs.bundle_init.len(),
            ctx_template_bytes = blobs.ctx_size(),
            "GR firmware parsed — {} entries need BAR0 MMIO (not channel-submittable)",
            bar0_count
        );

        if channel_methods.is_empty() {
            tracing::debug!(
                chip,
                "no channel-submittable method entries — GR init depends on kernel or BAR0 MMIO"
            );
            return;
        }

        let pb = pushbuf::PushBuf::gr_context_init(self.compute_class, &channel_methods);
        let pb_bytes = pb.as_bytes();

        let pb_size = match u64::try_from(pb_bytes.len()) {
            Ok(s) => s,
            Err(_) => {
                tracing::warn!("GR init pushbuf too large — skipping");
                return;
            }
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
            "submitting {} channel method entries for GR init",
            channel_methods.len()
        );

        let submit_result = if self.new_uapi {
            let pb_va = self.buffers.get(&pb_handle.0).map_or(0, |b| b.gpu_va);
            let push_len = pb_size as u32;
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
            ioctl::pushbuf_submit(
                self.drm.fd(),
                self.channel,
                pb_gem,
                0,
                pb_size,
                &[pb_gem],
            )
        };

        match submit_result {
            Ok(()) => {
                tracing::info!(chip, "GR channel method init submitted");
                if let Some(syncobj) = self.exec_syncobj {
                    let timeout = {
                        let tp = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
                        tp.tv_sec * 1_000_000_000 + tp.tv_nsec as i64 + 5_000_000_000
                    };
                    if let Err(e) = ioctl::syncobj_wait(self.drm.fd(), syncobj, timeout) {
                        tracing::warn!(error = %e, "GR init syncobj wait failed");
                    }
                } else if let Some(gem) = self.buffers.get(&pb_handle.0).map(|b| b.gem_handle) {
                    let _ = ioctl::gem_cpu_prep(self.drm.fd(), gem);
                }
            }
            Err(e) => {
                tracing::warn!(
                    chip,
                    error = %e,
                    "GR channel method init failed — compute may get CTXNOTVALID"
                );
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

/// Run diagnostic probes when channel creation fails.
#[cfg(feature = "nouveau")]
fn run_open_diagnostics(drm: &DrmDevice, sm: u32, compute_class: u32) {
    let diags = ioctl::diagnose_channel_alloc(drm.fd(), compute_class);
    for diag in &diags {
        match &diag.result {
            Ok(ch) => tracing::info!(
                description = %diag.description,
                channel = ch,
                "diagnostic: PASS"
            ),
            Err(err) => tracing::warn!(
                description = %diag.description,
                error = %err,
                "diagnostic: FAIL"
            ),
        }
    }
    let chip = match sm {
        75 => "tu102",
        80..=89 => "ga102",
        _ => "gv100",
    };
    let fw = ioctl::check_nouveau_firmware(chip);
    let missing: Vec<_> = fw.iter().filter(|(_, exists)| !*exists).collect();
    if !missing.is_empty() {
        tracing::warn!(
            chip,
            missing_count = missing.len(),
            "nouveau firmware files missing — compute may not be available"
        );
    }
    if let Some(id) = ioctl::probe_gpu_identity(&drm.path) {
        tracing::info!(
            vendor = format_args!("0x{:04X}", id.vendor_id),
            device = format_args!("0x{:04X}", id.device_id),
            detected_sm = ?id.nvidia_sm(),
            "GPU identity from sysfs"
        );
    }
}

/// Reinterpret a `&[u32]` as `&[u8]` for buffer upload.
fn u32_slice_as_bytes(words: &[u32]) -> &[u8] {
    bytemuck::cast_slice(words)
}

/// Page-align a size upward (4 KiB pages).
const fn page_align(size: u64) -> u64 {
    (size + 0xFFF) & !0xFFF
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
        let shader_size = u64::try_from(shader.len())
            .map_err(|_| DriverError::platform_overflow("shader size fits in u64"))?;
        let shader_handle = self.alloc(shader_size, MemoryDomain::Gtt)?;
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
        let desc_entry_size = 16_u64; // 3 u32 fields + 4 bytes padding per binding
        let desc_buf_size = desc_entry_size * u64::try_from(buffers.len().max(1))
            .map_err(|_| DriverError::platform_overflow("buffer count fits in u64"))?;
        let desc_handle = self.alloc(desc_buf_size, MemoryDomain::Gtt)?;

        // Populate the descriptor buffer with each binding's VA and size.
        let mut desc_data = vec![0u8; desc_buf_size as usize];
        for (i, bh) in buffers.iter().enumerate() {
            if let Some(buf) = self.buffers.get(&bh.0) {
                let off = i * 8; // binding * 8 matches compiler offset
                let va = buf.gpu_va;
                let sz = u32::try_from(buf.size).unwrap_or(u32::MAX);
                desc_data[off..off + 4].copy_from_slice(&(va as u32).to_le_bytes());
                desc_data[off + 4..off + 8].copy_from_slice(&((va >> 32) as u32).to_le_bytes());
                // size at offset binding * 8 + 8
                let sz_off = off + 8;
                if sz_off + 4 <= desc_data.len() {
                    desc_data[sz_off..sz_off + 4].copy_from_slice(&sz.to_le_bytes());
                }
            }
        }
        self.upload(desc_handle, 0, &desc_data)?;
        let desc_va = self.buffers.get(&desc_handle.0).map_or(0, |b| b.gpu_va);

        // CBUF 0 points to the descriptor buffer, not the storage buffer
        let cbufs = vec![qmd::CbufBinding {
            index: 0,
            addr: desc_va,
            size: u32::try_from(desc_buf_size).unwrap_or(u32::MAX),
        }];

        // Build QMD with compiler-derived metadata (version selected by SM arch)
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

        // Upload QMD to GPU memory
        let qmd_size = u64::try_from(qmd_bytes.len())
            .map_err(|_| DriverError::platform_overflow("QMD size fits in u64"))?;
        let qmd_handle = self.alloc(qmd_size, MemoryDomain::Gtt)?;
        self.upload(qmd_handle, 0, qmd_bytes)?;
        let qmd_va = self.buffers.get(&qmd_handle.0).map_or(0, |b| b.gpu_va);

        // Build push buffer: SET_OBJECT + caches + SEND_PCAS with QMD address
        let local_mem_window = if self.sm_version >= 70 {
            0xFF00_0000_0000_0000_u64
        } else {
            0xFF00_0000_u64
        };
        let pb = pushbuf::PushBuf::compute_dispatch(self.compute_class, qmd_va, local_mem_window);
        let pb_bytes = pb.as_bytes();

        // Upload push buffer to GPU memory
        let pb_size = u64::try_from(pb_bytes.len())
            .map_err(|_| DriverError::platform_overflow("pushbuf size fits in u64"))?;
        let pb_handle = self.alloc(pb_size, MemoryDomain::Gtt)?;
        self.upload(pb_handle, 0, pb_bytes)?;
        let pb_gem = self.buffers.get(&pb_handle.0).map_or(0, |b| b.gem_handle);

        if self.new_uapi {
            // New UAPI: submit push buffer by VA via EXEC with syncobj signal.
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
            // Legacy UAPI: submit push buffer via GEM pushbuf.
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

        // Track the QMD GEM handle for fence sync (the GPU reads QMD last)
        self.last_submit_gem = self.buffers.get(&qmd_handle.0).map(|b| b.gem_handle);

        // Defer temp buffer cleanup until sync() — the GPU may still be reading
        self.inflight.push(pb_handle);
        self.inflight.push(qmd_handle);
        self.inflight.push(shader_handle);
        self.inflight.push(desc_handle);
        Ok(())
    }

    fn sync(&mut self) -> DriverResult<()> {
        if let Some(syncobj) = self.exec_syncobj {
            // New UAPI: wait on syncobj (5 second timeout)
            let timeout = {
                let tp = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
                tp.tv_sec * 1_000_000_000 + tp.tv_nsec as i64 + 5_000_000_000
            };
            ioctl::syncobj_wait(self.drm.fd(), syncobj, timeout)?;
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
        assert_eq!(sm_to_chip(50), "gm200");
        assert_eq!(sm_to_chip(60), "gp100");
        assert_eq!(sm_to_chip(70), "gv100");
        assert_eq!(sm_to_chip(75), "tu102");
        assert_eq!(sm_to_chip(80), "ga100");
        assert_eq!(sm_to_chip(86), "ga102");
        assert_eq!(sm_to_chip(89), "ad102");
    }

    #[test]
    fn compute_class_selection() {
        assert_eq!(compute_class_for_sm(70), pushbuf::class::VOLTA_COMPUTE_A);
        assert_eq!(compute_class_for_sm(75), pushbuf::class::TURING_COMPUTE_A);
        assert_eq!(compute_class_for_sm(86), pushbuf::class::AMPERE_COMPUTE_A);
    }
}
