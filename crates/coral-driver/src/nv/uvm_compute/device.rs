// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`NvUvmComputeDevice`] construction, RM channel setup, and GPFIFO submission.

use std::collections::HashMap;
use std::os::fd::AsRawFd;

use crate::error::{DriverError, DriverResult};
use crate::mmio::VolatilePtr;
use crate::nv::uvm::{NvGpuDevice, NvUvmDevice, RmClient};

use super::types::{
    CtxBuffer, GPFIFO_ENTRIES, GPFIFO_SIZE, GpuGen, USERD_GP_GET_OFFSET, USERD_GP_PUT_OFFSET,
    USERD_SIZE, UvmBuffer, gpfifo_entry, uvm_cache_line_flush,
};

/// Compute device backed by the NVIDIA proprietary driver (RM + UVM).
///
/// Implements the full dispatch pipeline: RM object allocation, UVM memory
/// mapping, QMD construction (via reused `qmd.rs`), and GPFIFO submission.
///
/// ## Thread safety (`Send` / `Sync`)
///
/// The type holds `std::fs::File` handles and kernel-mapped CPU addresses returned
/// by the RM/UVM ioctls. Those OS resources are safe to move across threads (`Send`)
/// and to share by immutable reference (`Sync`) when the public API contract is
/// followed: mutating operations go through `&mut self` on [`crate::ComputeDevice`],
/// and the embedded [`std::collections::HashMap`] keys/values are `Send` + `Sync`.
/// GPU submission is serialized through the single GPFIFO channel owned by this
/// struct; no unsynchronized interior mutability exposes hardware state.
pub struct NvUvmComputeDevice {
    pub(super) client: RmClient,
    #[expect(
        dead_code,
        reason = "held for lifetime — UVM fd needed for GPU VA operations"
    )]
    pub(super) uvm: NvUvmDevice,
    pub(super) gpu: NvGpuDevice,
    pub(super) gpu_gen: GpuGen,
    pub(super) h_device: u32,
    #[expect(dead_code, reason = "held for RM_CONTROL calls (e.g. perf queries)")]
    pub(super) h_subdevice: u32,
    #[expect(
        dead_code,
        reason = "held for VA space teardown and future sub-allocations"
    )]
    pub(super) h_vaspace: u32,
    #[expect(dead_code, reason = "held for channel group teardown")]
    pub(super) h_changrp: u32,
    #[expect(
        dead_code,
        reason = "held for channel teardown / GPFIFO ring ownership"
    )]
    pub(super) h_channel: u32,
    pub(super) h_compute: u32,
    #[expect(dead_code, reason = "needed for UVM_MAP_EXTERNAL_ALLOCATION")]
    pub(super) gpu_uuid: [u8; 16],
    pub(super) buffers: HashMap<u32, UvmBuffer>,
    /// GR context buffers promoted to RM via `GPU_PROMOTE_CTX`.
    /// Freed on drop to release the RM allocations.
    pub(super) ctx_buffers: Vec<CtxBuffer>,
    pub(super) next_handle: u32,
    pub(super) next_mem_handle: u32,
    /// Inflight temporary buffers that survive until `sync()`.
    pub(super) inflight: Vec<crate::BufferHandle>,
    /// Deferred-free buffers from previous dispatches. Freed on drop or
    /// when explicitly drained. Prevents VA recycling races on Blackwell.
    pub(super) deferred_free: Vec<crate::BufferHandle>,
    /// CPU-mapped pointer to the USERD page (for `GP_PUT` doorbell writes).
    pub(super) userd_cpu_addr: u64,
    /// CPU-mapped pointer to the GPFIFO ring (for writing GPFIFO entries).
    pub(super) gpfifo_cpu_addr: u64,
    /// Current `GP_PUT` index (next slot to write in the GPFIFO ring).
    pub(super) gp_put: u32,
    /// Handle of the `NV01_MEMORY_VIRTUAL` for DMA mapping.
    pub(super) h_virt_mem: u32,
    #[expect(dead_code, reason = "kept alive for USERD mmap lifetime")]
    pub(super) userd_mmap_fd: std::fs::File,
    #[expect(dead_code, reason = "kept alive for GPFIFO mmap lifetime")]
    pub(super) gpfifo_mmap_fd: std::fs::File,
    #[expect(dead_code, reason = "kept alive for USERMODE doorbell mmap lifetime")]
    pub(super) usermode_mmap_fd: std::fs::File,
    /// CPU-mapped pointer to the error notifier buffer (16 bytes per entry).
    pub(super) errnotif_cpu_addr: u64,
    #[expect(dead_code, reason = "kept alive for error notifier mmap lifetime")]
    pub(super) errnotif_mmap_fd: std::fs::File,
    /// CPU-mapped pointer to the USERMODE doorbell register page.
    pub(super) doorbell_addr: u64,
    /// Work submit token returned by RM (written to doorbell to notify GPU).
    pub(super) work_submit_token: u32,
    /// Whether this GPU uses semaphore-based completion (Blackwell+).
    /// Blackwell removed GP_GET from the USERD control struct, so we must
    /// use a semaphore release in the push buffer to signal completion.
    pub(super) uses_semaphore_fence: bool,
    /// CPU-mapped address of the 4-byte fence value (for semaphore completion).
    pub(super) fence_cpu_addr: u64,
    /// GPU virtual address of the fence buffer (for semaphore release target).
    pub(super) fence_gpu_va: u64,
    /// Current fence value (incremented on each submission).
    pub(super) fence_value: u32,
    #[expect(dead_code, reason = "kept alive for fence mmap lifetime")]
    pub(super) fence_mmap_fd: Option<std::fs::File>,
    /// CPU-mapped address of the persistent fence push buffer (6 dwords).
    pub(super) fence_pb_cpu_addr: u64,
    /// GPU virtual address of the fence push buffer.
    pub(super) fence_pb_gpu_va: u64,
    #[expect(dead_code, reason = "kept alive for fence pb mmap lifetime")]
    pub(super) fence_pb_mmap_fd: Option<std::fs::File>,
    /// Handle to `/dev/coral-rm` for kmod-based buffer allocation (Blackwell+).
    pub(super) coral_kmod: Option<crate::nv::coral_kmod::CoralKmod>,
    /// h_client from kmod's INIT_COMPUTE, needed for kmod buffer ops.
    pub(super) kmod_h_client: u32,
}

impl NvUvmComputeDevice {
    /// Open a UVM compute device for the specified GPU index and SM version.
    ///
    /// On Blackwell+ (SM >= 100), first attempts to use `coral-kmod.ko` for
    /// kernel-privileged channel setup (required for `GPU_PROMOTE_CTX`).
    /// Falls back to the direct userspace RM path if the module is not loaded.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if any step in the initialization chain fails.
    pub fn open(gpu_index: u32, sm: u32) -> DriverResult<Self> {
        // Blackwell+ benefits from kernel-privileged channel setup (GPU_PROMOTE_CTX).
        if sm >= 100
            && let Some(kmod) = crate::nv::coral_kmod::CoralKmod::try_open()
        {
            match Self::open_via_kmod(kmod, gpu_index, sm) {
                Ok(dev) => return Ok(dev),
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "coral-kmod init failed, falling back to userspace RM"
                    );
                }
            }
        }

        Self::open_userspace(gpu_index, sm)
    }

    /// Open via coral-kmod kernel module (kernel-privileged RM client).
    ///
    /// The kernel module creates the channel and context with `RS_PRIV_LEVEL_KERNEL`,
    /// enabling `GPU_PROMOTE_CTX` and `KGR_GET_CONTEXT_BUFFERS_INFO`.
    fn open_via_kmod(
        kmod: crate::nv::coral_kmod::CoralKmod,
        gpu_index: u32,
        sm: u32,
    ) -> DriverResult<Self> {
        use crate::nv::coral_kmod::kmod_map_rm_memory;

        let gpu_gen = GpuGen::from_sm(sm);
        let info = kmod.init_compute(gpu_index, sm)?;
        let ctl_fd = info.ctl_fd;

        tracing::info!(
            h_client = format_args!("0x{:08X}", info.h_client),
            h_channel = format_args!("0x{:08X}", info.h_channel),
            ctx_bufs = info.ctx_bufs.len(),
            ctl_fd,
            "coral-kmod: kernel-privileged channel initialized"
        );

        // The ctl_fd is a kernel-privileged /dev/nvidiactl installed into
        // our process by the kernel module. RM_MAP_MEMORY ioctl must be
        // sent on this fd (it owns the RM client). Each mapping needs its
        // OWN separate /dev/nvidiactl as the mmap target fd — RM can't
        // create multiple mmap contexts on a single file.
        //
        // We do NOT wrap ctl_fd in a File (which would close it on drop).
        // The kernel module holds a reference to the underlying file via
        // ch->ctl_filp, so the RM client stays alive regardless.
        // The fd is cleaned up when the channel is destroyed or the process exits.

        let open_mmap_fd = || -> DriverResult<std::fs::File> {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/nvidiactl")
                .map_err(|e| DriverError::DeviceNotFound(format!("nvidiactl mmap: {e}").into()))
        };

        let userd_mmap_file = open_mmap_fd()?;
        let userd_cpu_addr = kmod_map_rm_memory(
            ctl_fd,
            userd_mmap_file.as_raw_fd(),
            info.h_client,
            info.h_device,
            info.h_userd_mem,
            0,
            info.userd_size,
        )?;
        let gpfifo_mmap_file = open_mmap_fd()?;
        let gpfifo_cpu_addr = kmod_map_rm_memory(
            ctl_fd,
            gpfifo_mmap_file.as_raw_fd(),
            info.h_client,
            info.h_device,
            info.h_gpfifo_mem,
            0,
            info.gpfifo_size,
        )?;
        let errnotif_mmap_file = open_mmap_fd()?;
        let errnotif_cpu_addr = kmod_map_rm_memory(
            ctl_fd,
            errnotif_mmap_file.as_raw_fd(),
            info.h_client,
            info.h_device,
            info.h_errnotif_mem,
            0,
            4096,
        )?;

        // USERMODE doorbell is BAR-mapped — needs the GPU device fd as mmap target.
        let gpu_dev_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(format!("/dev/nvidia{gpu_index}"))
            .map_err(|e| {
                DriverError::DeviceNotFound(format!("nvidia{gpu_index} for doorbell: {e}").into())
            })?;
        let doorbell_addr = kmod_map_rm_memory(
            ctl_fd,
            gpu_dev_file.as_raw_fd(),
            info.h_client,
            info.h_device,
            info.h_usermode,
            0,
            4096,
        )?;

        let ctx_buffers: Vec<CtxBuffer> = info
            .ctx_bufs
            .iter()
            .map(|cb| CtxBuffer {
                buffer_id: cb.buffer_id,
                h_memory: cb.h_memory,
                size: cb.size,
                gpu_va: cb.gpu_va,
            })
            .collect();

        let uses_semaphore_fence = super::ctx_buffers::uses_semaphore_fence_for_gen(gpu_gen);

        // SAFETY: ctl_fd is a valid kernel-module-opened fd.
        let client = unsafe { RmClient::wrap_kmod_fd(ctl_fd, info.h_client) }?;
        let uvm = NvUvmDevice::open()?;
        let gpu = NvGpuDevice::open(gpu_index)?;

        gpu.register_fd(ctl_fd)?;
        uvm.initialize()?;

        // UVM setup: CUDA's order is REGISTER_GPU → PAGEABLE_MEM_ACCESS.
        // Register GPU with UVM (reuse the UUID from kmod).
        {
            use crate::nv::uvm::UVM_REGISTER_GPU;
            use crate::nv::uvm::nv_status::NV_OK;
            use crate::nv::uvm::structs::UvmRegisterGpuParams;
            let mut reg = UvmRegisterGpuParams::default();
            reg.gpu_uuid = info.gpu_uuid;
            reg.rm_ctrl_fd = ctl_fd;
            reg.h_client = info.h_client;
            match uvm.raw_ioctl(UVM_REGISTER_GPU, &mut reg, "UVM_REGISTER_GPU") {
                Ok(()) if reg.rm_status == NV_OK => {
                    tracing::info!("UVM_REGISTER_GPU OK");
                }
                Ok(()) => {
                    tracing::warn!(
                        rm_status = format_args!("0x{:08X}", reg.rm_status),
                        "UVM_REGISTER_GPU non-OK status (continuing)"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "UVM_REGISTER_GPU ioctl failed (continuing)"
                    );
                }
            }
        }

        // Query pageable memory access support (CUDA calls this after REGISTER_GPU).
        match uvm.pageable_mem_access() {
            Ok(supported) => {
                tracing::debug!(supported, "UVM_PAGEABLE_MEM_ACCESS OK");
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "UVM_PAGEABLE_MEM_ACCESS failed (continuing)"
                );
            }
        }

        // Skip register_gpu_vaspace — CUDA on Blackwell (R580) does NOT call
        // it, and it fails with NV_ERR_GPU_IN_FULL on desktop systems anyway.
        // Data buffers use RM-level DMA mapping (rm_map_memory_dma), not UVM
        // external mappings, so UVM VA space registration shouldn't be needed.

        let dev = Self {
            client,
            uvm,
            gpu,
            gpu_gen,
            h_device: info.h_device,
            h_subdevice: info.h_subdevice,
            h_vaspace: info.h_vaspace,
            h_changrp: info.h_changrp,
            h_channel: info.h_channel,
            h_compute: info.h_compute,
            gpu_uuid: info.gpu_uuid,
            buffers: HashMap::new(),
            ctx_buffers,
            next_handle: 1,
            next_mem_handle: info.h_device + 0x7000,
            inflight: Vec::new(),
            deferred_free: Vec::new(),
            userd_cpu_addr,
            gpfifo_cpu_addr,
            gp_put: 0,
            h_virt_mem: info.h_virt_mem,
            userd_mmap_fd: userd_mmap_file,
            gpfifo_mmap_fd: gpfifo_mmap_file,
            errnotif_cpu_addr,
            errnotif_mmap_fd: errnotif_mmap_file,
            usermode_mmap_fd: gpu_dev_file,
            doorbell_addr,
            work_submit_token: info.work_submit_token,
            uses_semaphore_fence,
            fence_cpu_addr: 0,
            fence_gpu_va: 0,
            fence_value: 0,
            fence_mmap_fd: None,
            fence_pb_cpu_addr: 0,
            fence_pb_gpu_va: 0,
            fence_pb_mmap_fd: None,
            coral_kmod: Some(kmod),
            kmod_h_client: info.h_client,
        };

        tracing::info!(
            gpu_index,
            sm,
            "NvUvmComputeDevice initialized via coral-kmod (kernel-privileged)"
        );

        Ok(dev)
    }

    /// Open via direct userspace RM path (original implementation).
    fn open_userspace(gpu_index: u32, sm: u32) -> DriverResult<Self> {
        let gpu_gen = GpuGen::from_sm(sm);

        let mut client = RmClient::new()?;
        let uvm = NvUvmDevice::open()?;
        let gpu = NvGpuDevice::open(gpu_index)?;
        gpu.register_fd(client.ctl_fd())?;

        uvm.initialize()?;

        let h_device = client.alloc_device(gpu_index)?;
        let h_subdevice = client.alloc_subdevice(h_device)?;

        let gpu_uuid = client.register_gpu_with_uvm(h_subdevice, &uvm)?;

        let h_userd_mem = h_device + 0x5000;
        let h_gpfifo_mem = h_device + 0x5001;
        let h_virt_mem = h_device + 0x5002;

        // CUDA allocates USERD in VRAM (NV01_MEMORY_LOCAL_USER) with 2 MiB
        // size/alignment. Try that first; fall back to system memory if needed.
        let userd_vram_size: u64 = 0x20_0000; // 2 MiB like CUDA
        let userd_in_vram = match client.alloc_local_memory(h_device, h_userd_mem, userd_vram_size)
        {
            Ok(_) => {
                tracing::info!("USERD allocated in VRAM (2 MiB)");
                true
            }
            Err(e) => {
                tracing::warn!("VRAM USERD failed ({e}), falling back to contiguous sysmem");
                client.alloc_contig_system_memory(h_device, h_userd_mem, USERD_SIZE)?;
                false
            }
        };
        client.alloc_system_memory(h_device, h_gpfifo_mem, GPFIFO_SIZE)?;

        let h_errnotif_mem = h_device + 0x5004;
        client.alloc_error_notifier(h_device, h_errnotif_mem)?;

        // VA space: try ENABLE_FAULTING first (useful for replayable faults
        // on user allocations). Falls back to flags=0 if rejected.
        let h_vaspace = match client.alloc_vaspace_for_uvm(h_device) {
            Ok(h) => {
                tracing::info!(
                    h_vaspace = format_args!("0x{h:08X}"),
                    "alloc_vaspace_for_uvm OK"
                );
                h
            }
            Err(e) => {
                tracing::warn!(error = %e, "alloc_vaspace_for_uvm failed");
                tracing::warn!("falling back to alloc_vaspace (flags=0)");
                client.alloc_vaspace(h_device)?
            }
        };

        // Attempt UVM VA space registration for demand-paged fault servicing.
        // Currently fails with NV_ERR_GPU_IN_FULL (0x5D) on desktop systems
        // where the compositor already consumes fault-handling capacity.
        match uvm.register_gpu_vaspace(&gpu_uuid, client.ctl_fd(), client.handle(), h_vaspace) {
            Ok(()) => tracing::info!("register_gpu_vaspace OK"),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "register_gpu_vaspace failed (continuing)"
                );
            }
        }

        let ch = super::channel_setup::userspace_setup_gpfifo_channel(
            &mut client,
            gpu_index,
            gpu_gen,
            h_device,
            h_subdevice,
            h_vaspace,
            h_userd_mem,
            h_gpfifo_mem,
            h_virt_mem,
            h_errnotif_mem,
            userd_in_vram,
        )?;

        let (ctx_buffers, kmod_bind_ok) = super::ctx_buffers::promote_ctx_buffers_userspace(
            &mut client,
            sm,
            &gpu_uuid,
            h_device,
            h_subdevice,
            h_vaspace,
            ch.h_channel,
            h_virt_mem,
        )?;

        super::ctx_buffers::gr_ctxsw_setup_after_promotion(
            &mut client,
            kmod_bind_ok,
            &ctx_buffers,
            h_subdevice,
            ch.h_channel,
        )?;

        let doorbell_fence = super::channel_setup::userspace_schedule_tsg_doorbell_and_fence(
            &mut client,
            gpu_index,
            gpu_gen,
            h_device,
            h_subdevice,
            ch.h_changrp,
            ch.h_channel,
            ch.hw_channel_id,
            h_virt_mem,
        )?;

        let compute_class = gpu_gen.compute_class();

        tracing::info!(
            gpu_index,
            sm,
            h_device = format_args!("0x{h_device:08X}"),
            h_channel = format_args!("0x{:08X}", ch.h_channel),
            h_compute = format_args!("0x{:08X}", ch.h_compute),
            work_submit_token = format_args!("0x{:08X}", doorbell_fence.work_submit_token),
            uses_semaphore_fence = doorbell_fence.uses_semaphore_fence,
            "NvUvmComputeDevice fully initialized"
        );

        let mut dev = Self {
            client,
            uvm,
            gpu,
            gpu_gen,
            h_device,
            h_subdevice,
            h_vaspace,
            h_changrp: ch.h_changrp,
            h_channel: ch.h_channel,
            h_compute: ch.h_compute,
            gpu_uuid,
            buffers: HashMap::new(),
            ctx_buffers,
            next_handle: 1,
            next_mem_handle: h_device + 0x6000,
            inflight: Vec::new(),
            deferred_free: Vec::new(),
            userd_cpu_addr: ch.userd_cpu_addr,
            gpfifo_cpu_addr: ch.gpfifo_cpu_addr,
            gp_put: 0,
            h_virt_mem,
            userd_mmap_fd: ch.userd_mmap_fd,
            gpfifo_mmap_fd: ch.gpfifo_mmap_fd,
            errnotif_cpu_addr: ch.errnotif_cpu_addr,
            errnotif_mmap_fd: ch.errnotif_mmap_fd,
            usermode_mmap_fd: doorbell_fence.usermode_mmap_fd,
            doorbell_addr: doorbell_fence.doorbell_addr,
            work_submit_token: doorbell_fence.work_submit_token,
            uses_semaphore_fence: doorbell_fence.uses_semaphore_fence,
            fence_cpu_addr: doorbell_fence.fence_cpu_addr,
            fence_gpu_va: doorbell_fence.fence_gpu_va,
            fence_value: 0,
            fence_mmap_fd: doorbell_fence.fence_mmap_fd,
            fence_pb_cpu_addr: doorbell_fence.fence_pb_cpu_addr,
            fence_pb_gpu_va: doorbell_fence.fence_pb_gpu_va,
            fence_pb_mmap_fd: doorbell_fence.fence_pb_mmap_fd,
            coral_kmod: None,
            kmod_h_client: 0,
        };

        super::channel_setup::userspace_nop_smoke_slq_compute_init(
            &mut dev,
            h_device,
            h_virt_mem,
            compute_class,
        )?;

        Ok(dev)
    }

    #[expect(
        clippy::missing_const_for_fn,
        reason = "mutates self for handle allocation; not const-compatible"
    )]
    pub(super) fn alloc_handle(&mut self) -> u32 {
        let h = self.next_handle;
        self.next_handle += 1;
        h
    }

    #[expect(
        clippy::missing_const_for_fn,
        reason = "mutates self for handle allocation; not const-compatible"
    )]
    pub(super) fn alloc_mem_handle(&mut self) -> u32 {
        let h = self.next_mem_handle;
        self.next_mem_handle += 1;
        h
    }

    /// The SM version this device targets.
    #[must_use]
    pub const fn sm_version(&self) -> u32 {
        match self.gpu_gen {
            GpuGen::Volta => 70,
            GpuGen::Turing => 75,
            GpuGen::AmpereA => 80,
            GpuGen::AmpereB => 86,
            GpuGen::Ada => 89,
            GpuGen::Hopper => 90,
            GpuGen::BlackwellA => 100,
            GpuGen::BlackwellB => 120,
        }
    }

    /// Whether this device is operational.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.h_compute != 0
    }

    /// Write a GPFIFO entry and ring the USERD doorbell.
    pub(super) fn submit_gpfifo(
        &mut self,
        push_buf_va: u64,
        length_dwords: u32,
    ) -> DriverResult<()> {
        let entry = gpfifo_entry(push_buf_va, length_dwords);
        let slot = (self.gp_put % GPFIFO_ENTRIES) as usize;
        let entry_offset = slot * 8;

        if self.gpfifo_cpu_addr == 0 {
            return Err(DriverError::SubmitFailed(
                "GPFIFO ring not CPU-mapped".into(),
            ));
        }

        let gpfifo_slot = (self.gpfifo_cpu_addr + entry_offset as u64) as *mut u64;
        // SAFETY: gpfifo_cpu_addr is a valid kernel mmap'd address (from rm_map_memory).
        // entry_offset < GPFIFO_SIZE is guaranteed by the modulo above.
        let vol = unsafe { VolatilePtr::new(gpfifo_slot) };
        vol.write(entry);

        self.gp_put = self.gp_put.wrapping_add(1);

        if self.userd_cpu_addr == 0 {
            return Err(DriverError::SubmitFailed("USERD not CPU-mapped".into()));
        }

        // Flush GPFIFO entry from CPU cache so GPU DMA sees it.
        // SAFETY: gpfifo_slot..+8 is within the valid GPFIFO mapping.
        unsafe {
            uvm_cache_line_flush(gpfifo_slot as *const u8);
        }

        let doorbell = (self.userd_cpu_addr + USERD_GP_PUT_OFFSET as u64) as *mut u32;
        // SAFETY: userd_cpu_addr is a valid kernel mmap'd address.
        // GP_PUT offset (0x8C) is within the 4096-byte USERD page.
        let vol = unsafe { VolatilePtr::new(doorbell) };
        vol.write(self.gp_put);

        // Flush USERD page from CPU cache so GPU sees GP_PUT update.
        // SAFETY: doorbell points within the valid USERD mapping.
        unsafe {
            uvm_cache_line_flush(doorbell as *const u8);
        }

        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

        // Ring the USERMODE doorbell to notify the GPU.
        // SAFETY: doorbell_addr is a valid mmap'd BAR0 USERMODE page.
        // Offset 0x90 = NV_USERMODE_NOTIFY_CHANNEL_PENDING.
        if self.doorbell_addr != 0 {
            let db = (self.doorbell_addr + 0x90) as *mut u32;
            unsafe { VolatilePtr::new(db).write(self.work_submit_token) };
        }

        tracing::debug!(
            gp_put = self.gp_put,
            push_buf_va = format_args!("0x{push_buf_va:016X}"),
            length_dwords,
            "GPFIFO entry submitted"
        );
        Ok(())
    }

    /// Poll for GPFIFO completion.
    ///
    /// On Volta-Hopper: reads `GP_GET` from the USERD page (GPU writes it).
    /// On Blackwell+: reads the semaphore fence value from system memory
    /// (GPU writes it via SEM_RELEASE in the push buffer). Blackwell removed
    /// `GP_GET` from the USERD control struct (clca6f: entire 0x00-0x8B is Ignored).
    pub(super) fn poll_gpfifo_completion(&self) -> DriverResult<()> {
        if self.uses_semaphore_fence {
            return self.poll_fence_completion();
        }

        if self.userd_cpu_addr == 0 || self.gp_put == 0 {
            return Ok(());
        }

        let gp_get_ptr = (self.userd_cpu_addr + USERD_GP_GET_OFFSET as u64) as *mut u32;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            // Invalidate the cache line before reading so we see the GPU's update.
            unsafe { uvm_cache_line_flush(gp_get_ptr as *const u8) };
            // SAFETY: userd_cpu_addr is a valid kernel mmap'd address; GP_GET lies
            // within the USERD page; volatile read matches GPU DMA updates.
            let gp_get = unsafe { VolatilePtr::new(gp_get_ptr).read() };
            if gp_get >= self.gp_put {
                return Ok(());
            }
            if std::time::Instant::now() > deadline {
                let errnotif = self.read_error_notifier();
                return Err(DriverError::SubmitFailed(
                    format!(
                        "GPFIFO completion timeout: GP_GET={gp_get} GP_PUT={} errnotif=[{errnotif}]",
                        self.gp_put
                    )
                    .into(),
                ));
            }
            std::hint::spin_loop();
            std::thread::sleep(std::time::Duration::from_micros(10));
        }
    }

    /// Poll the semaphore fence for Blackwell+ completion.
    ///
    /// After the fence advances, we also check the error notifier — on
    /// Blackwell the fence release is a separate GPFIFO entry that the
    /// PBDMA may process even after the compute engine reports an error,
    /// which would make a failed dispatch appear to succeed.
    fn poll_fence_completion(&self) -> DriverResult<()> {
        if self.fence_cpu_addr == 0 || self.fence_value == 0 {
            return Ok(());
        }

        let fence_ptr = self.fence_cpu_addr as *mut u32;
        let expected = self.fence_value;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            unsafe { uvm_cache_line_flush(fence_ptr as *const u8) };
            let current = unsafe { VolatilePtr::new(fence_ptr).read() };
            if current >= expected {
                // Fence advanced — but check if an async error was reported.
                let errnotif = self.read_error_notifier();
                if errnotif.contains("status=0xFFFF") {
                    return Err(DriverError::SubmitFailed(
                        format!(
                            "Blackwell dispatch error (fence OK but errnotif set): \
                             fence={current} expected={expected} errnotif=[{errnotif}]"
                        )
                        .into(),
                    ));
                }
                return Ok(());
            }
            if std::time::Instant::now() > deadline {
                let errnotif = self.read_error_notifier();
                return Err(DriverError::SubmitFailed(
                    format!(
                        "Blackwell fence timeout: fence={current} expected={expected} errnotif=[{errnotif}]"
                    )
                    .into(),
                ));
            }
            std::hint::spin_loop();
            std::thread::sleep(std::time::Duration::from_micros(10));
        }
    }

    /// Read the GPU error notifier and return a diagnostic string.
    ///
    /// The NVIDIA error notifier is a 16-byte struct:
    /// - `[0:7]`  timestamp (nanoseconds)
    /// - `[8:11]` info32 (error-specific data)
    /// - `[12:13]` info16 (error-specific data)
    /// - `[14:15]` status (0 = OK, 0x8000+ = error)
    pub(super) fn read_error_notifier(&self) -> String {
        if self.errnotif_cpu_addr == 0 {
            return "error notifier not mapped".to_string();
        }
        let base = self.errnotif_cpu_addr as *const u32;
        // SAFETY: errnotif_cpu_addr is a valid 4096-byte mmap from RM.
        // We read 4 dwords (16 bytes) — one NvNotification entry.
        let (ts_lo, ts_hi, info32, status_word) = unsafe {
            (
                VolatilePtr::new(base.cast_mut()).read(),
                VolatilePtr::new(base.add(1).cast_mut()).read(),
                VolatilePtr::new(base.add(2).cast_mut()).read(),
                VolatilePtr::new(base.add(3).cast_mut()).read(),
            )
        };
        let info16 = status_word & 0xFFFF;
        let status = (status_word >> 16) & 0xFFFF;
        format!(
            "ts=0x{ts_hi:08X}_{ts_lo:08X} info32=0x{info32:08X} info16=0x{info16:04X} status=0x{status:04X}"
        )
    }

    /// Submit a semaphore release GPFIFO entry for Blackwell fence tracking.
    ///
    /// Rewrites the persistent fence push buffer with the current fence value,
    /// then submits it as a second GPFIFO entry. The GPU will write
    /// `self.fence_value` to `self.fence_gpu_va` upon completing all prior work.
    pub(super) fn submit_fence_release(&mut self) -> DriverResult<()> {
        if !self.uses_semaphore_fence || self.fence_pb_cpu_addr == 0 {
            return Ok(());
        }

        self.fence_value += 1;
        let fv = self.fence_value;
        let fva = self.fence_gpu_va;
        let pb = self.fence_pb_cpu_addr as *mut u32;

        // Write a 6-dword semaphore release push buffer.
        // SAFETY: fence_pb_cpu_addr is a valid 4096-byte mmap. We write 24 bytes.
        unsafe {
            // Method header: SEC_OP=1(INC_METHOD), count=5, subchan=0, addr=0x17 (SEM_ADDR_LO >> 2)
            VolatilePtr::new(pb).write((1 << 29) | (5 << 16) | 0x17);
            VolatilePtr::new(pb.add(1)).write((fva & 0xFFFF_FFFC) as u32);
            VolatilePtr::new(pb.add(2)).write((fva >> 32) as u32);
            VolatilePtr::new(pb.add(3)).write(fv);
            VolatilePtr::new(pb.add(4)).write(0);
            // SEM_EXECUTE: OPERATION=RELEASE(1)
            VolatilePtr::new(pb.add(5)).write(1);
            uvm_cache_line_flush(pb as *const u8);
        }

        self.submit_gpfifo(self.fence_pb_gpu_va, 6)?;
        tracing::debug!(fence_value = fv, "Blackwell fence release submitted");
        Ok(())
    }

    /// Map an RM buffer into the GPU VA space with shader read/write access.
    ///
    /// Uses `NVOS46_FLAGS_SHADER_ACCESS_READ_WRITE` so that LDG/STG/LDC
    /// instructions can access the mapping. Without this flag, shader
    /// memory accesses fault or silently return zero.
    pub(super) fn gpu_map_buffer(&mut self, h_mem: u32, size: u64) -> DriverResult<u64> {
        self.client
            .rm_map_memory_dma_shader(self.h_device, self.h_virt_mem, h_mem, 0, size)
    }
}
