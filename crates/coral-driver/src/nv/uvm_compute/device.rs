// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`NvUvmComputeDevice`] construction, RM channel setup, and GPFIFO submission.

use std::collections::HashMap;
use std::os::fd::AsRawFd;

use crate::error::{DriverError, DriverResult};
use crate::mmio::VolatilePtr;
use crate::nv::uvm::{NvGpuDevice, NvUvmDevice, RmClient, VOLTA_USERMODE_A};

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
    #[expect(
        dead_code,
        reason = "held for lifetime — GPU fd needed for mmap and RM operations"
    )]
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
        if sm >= 100 {
            if let Some(kmod) = crate::nv::coral_kmod::CoralKmod::try_open() {
                match Self::open_via_kmod(kmod, gpu_index, sm) {
                    Ok(dev) => return Ok(dev),
                    Err(e) => {
                        eprintln!(
                            "[coral-driver] coral-kmod init failed ({e}), falling back to userspace RM"
                        );
                    }
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
            ctl_fd, userd_mmap_file.as_raw_fd(),
            info.h_client, info.h_device, info.h_userd_mem,
            0, info.userd_size,
        )?;
        let gpfifo_mmap_file = open_mmap_fd()?;
        let gpfifo_cpu_addr = kmod_map_rm_memory(
            ctl_fd, gpfifo_mmap_file.as_raw_fd(),
            info.h_client, info.h_device, info.h_gpfifo_mem,
            0, info.gpfifo_size,
        )?;
        let errnotif_mmap_file = open_mmap_fd()?;
        let errnotif_cpu_addr = kmod_map_rm_memory(
            ctl_fd, errnotif_mmap_file.as_raw_fd(),
            info.h_client, info.h_device, info.h_errnotif_mem,
            0, 4096,
        )?;

        // USERMODE doorbell is BAR-mapped — needs the GPU device fd as mmap target.
        let gpu_dev_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(format!("/dev/nvidia{gpu_index}"))
            .map_err(|e| {
                DriverError::DeviceNotFound(
                    format!("nvidia{gpu_index} for doorbell: {e}").into(),
                )
            })?;
        let doorbell_addr = kmod_map_rm_memory(
            ctl_fd, gpu_dev_file.as_raw_fd(),
            info.h_client, info.h_device, info.h_usermode,
            0, 4096,
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

        let uses_semaphore_fence = matches!(gpu_gen, GpuGen::BlackwellA | GpuGen::BlackwellB);

        // SAFETY: ctl_fd is a valid kernel-module-opened fd.
        let client = unsafe { RmClient::wrap_kmod_fd(ctl_fd, info.h_client) }?;
        let uvm = NvUvmDevice::open()?;
        let gpu = NvGpuDevice::open(gpu_index)?;

        gpu.register_fd(ctl_fd)?;
        uvm.initialize()?;

        // UVM setup: CUDA's order is REGISTER_GPU → PAGEABLE_MEM_ACCESS.
        // Register GPU with UVM (reuse the UUID from kmod).
        {
            use crate::nv::uvm::nv_status::NV_OK;
            use crate::nv::uvm::structs::UvmRegisterGpuParams;
            use crate::nv::uvm::UVM_REGISTER_GPU;
            let mut reg = UvmRegisterGpuParams::default();
            reg.gpu_uuid = info.gpu_uuid;
            reg.rm_ctrl_fd = ctl_fd;
            reg.h_client = info.h_client;
            match uvm.raw_ioctl(UVM_REGISTER_GPU, &mut reg, "UVM_REGISTER_GPU") {
                Ok(()) if reg.rm_status == NV_OK => {
                    eprintln!("[coral-driver] UVM_REGISTER_GPU OK");
                }
                Ok(()) => {
                    eprintln!(
                        "[coral-driver] UVM_REGISTER_GPU status=0x{:08X} (continuing)",
                        reg.rm_status
                    );
                }
                Err(e) => {
                    eprintln!("[coral-driver] UVM_REGISTER_GPU ioctl failed: {e} (continuing)");
                }
            }
        }

        // Query pageable memory access support (CUDA calls this after REGISTER_GPU).
        match uvm.pageable_mem_access() {
            Ok(supported) => eprintln!("[coral-driver] UVM_PAGEABLE_MEM_ACCESS OK (supported={supported})"),
            Err(e) => eprintln!("[coral-driver] UVM_PAGEABLE_MEM_ACCESS FAILED: {e} (continuing)"),
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
                eprintln!("[coral-driver] alloc_vaspace_for_uvm OK: h_vaspace=0x{h:08X}");
                h
            }
            Err(e) => {
                eprintln!("[coral-driver] alloc_vaspace_for_uvm FAILED: {e}");
                eprintln!("[coral-driver] falling back to alloc_vaspace (flags=0)");
                client.alloc_vaspace(h_device)?
            }
        };

        // Attempt UVM VA space registration for demand-paged fault servicing.
        // Currently fails with NV_ERR_GPU_IN_FULL (0x5D) on desktop systems
        // where the compositor already consumes fault-handling capacity.
        match uvm.register_gpu_vaspace(
            &gpu_uuid,
            client.ctl_fd(),
            client.handle(),
            h_vaspace,
        ) {
            Ok(()) => eprintln!("[coral-driver] register_gpu_vaspace OK"),
            Err(e) => eprintln!("[coral-driver] register_gpu_vaspace FAILED: {e} (continuing)"),
        }

        let h_changrp = client.alloc_channel_group(h_device, h_vaspace)?;

        let h_ctxshare = client.alloc_context_share(h_changrp, h_vaspace, h_subdevice)?;

        client.alloc_virtual_memory(h_device, h_virt_mem, h_vaspace)?;

        let gpfifo_gpu_va =
            client.rm_map_memory_dma(h_device, h_virt_mem, h_gpfifo_mem, 0, GPFIFO_SIZE)?;

        let (h_channel, hw_channel_id) = client.alloc_gpfifo_channel(
            h_changrp,
            h_userd_mem,
            h_errnotif_mem,
            h_ctxshare,
            gpfifo_gpu_va,
            GPFIFO_ENTRIES,
            gpu_gen.channel_class(),
        )?;

        // CPU-map USERD and GPFIFO on dedicated nvidiactl fds.
        let open_ctl = || -> DriverResult<std::fs::File> {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/nvidiactl")
                .map_err(|e| DriverError::DeviceNotFound(format!("nvidiactl for mmap: {e}").into()))
        };
        // VRAM buffers must be mapped on the GPU device fd (BAR1), not nvidiactl.
        let userd_mmap_fd = if userd_in_vram {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(format!("/dev/nvidia{gpu_index}"))
                .map_err(|e| {
                    DriverError::DeviceNotFound(format!("nvidia{gpu_index} for USERD: {e}").into())
                })?
        } else {
            open_ctl()?
        };
        let userd_cpu_addr = client.rm_map_memory_on_fd(
            userd_mmap_fd.as_raw_fd(),
            h_device,
            h_userd_mem,
            0,
            USERD_SIZE,
        )?;
        let gpfifo_mmap_fd = open_ctl()?;
        let gpfifo_cpu_addr = client.rm_map_memory_on_fd(
            gpfifo_mmap_fd.as_raw_fd(),
            h_device,
            h_gpfifo_mem,
            0,
            GPFIFO_SIZE,
        )?;

        // CPU-map the error notifier so we can read GPU error codes on timeout.
        let errnotif_mmap_fd = open_ctl()?;
        let errnotif_cpu_addr = client.rm_map_memory_on_fd(
            errnotif_mmap_fd.as_raw_fd(),
            h_device,
            h_errnotif_mem,
            0,
            4096,
        )?;

        let compute_class = gpu_gen.compute_class();
        let h_compute = client.alloc_compute_engine(h_channel, compute_class)?;

        // BIND the compute engine to the channel — without this the GPU
        // cannot route push buffer methods to the compute engine.
        client.channel_bind_engine(h_channel, h_compute, compute_class, 1)?;
        tracing::info!(
            h_compute = format_args!("0x{h_compute:08X}"),
            "Compute engine bound to channel"
        );

        // ── Context buffer binding ────────────────────────────────────
        //
        // Blackwell+ requires kernel privilege for context buffer promotion
        // (GPU_PROMOTE_CTX returns INSUFFICIENT_PERMISSIONS from userspace).
        //
        // Hybrid approach: if coral-kmod is loaded, use CORAL_IOCTL_BIND_CHANNEL
        // which calls nvUvmInterface{RetainChannel,BindChannelResources} from
        // kernel context. Falls back to userspace GPU_PROMOTE_CTX for older GPUs.
        let (ctx_buffers, kmod_bind_ok) = 'promote: {
            // Try kernel-privileged binding via coral-kmod (Blackwell+).
            if sm >= 100 {
                if let Some(kmod) = crate::nv::coral_kmod::CoralKmod::try_open() {
                    match kmod.bind_channel(
                        &gpu_uuid,
                        client.handle(),
                        h_vaspace,
                        h_channel,
                        sm,
                    ) {
                        Ok(result) => {
                            eprintln!(
                                "[coral-driver] BIND_CHANNEL via kmod: {} resources bound, \
                                 chId={} engineType={} tsgId={}",
                                result.resource_count,
                                result.hw_channel_id,
                                result.channel_engine_type,
                                result.tsg_id,
                            );
                            let ctx = result
                                .resources
                                .iter()
                                .map(|r| {
                                    eprintln!(
                                        "[coral-driver]   kmod resource id={} gpu_va=0x{:016X} \
                                         size=0x{:X} align=0x{:X}",
                                        r.resource_id, r.gpu_va, r.size, r.alignment,
                                    );
                                    CtxBuffer {
                                        buffer_id: r.resource_id as u16,
                                        h_memory: 0,
                                        size: r.size,
                                        gpu_va: r.gpu_va,
                                    }
                                })
                                .collect::<Vec<_>>();
                            break 'promote (ctx, true);
                        }
                        Err(e) => {
                            eprintln!(
                                "[coral-driver] BIND_CHANNEL via kmod failed: {e}, \
                                 falling back to GPU_PROMOTE_CTX"
                            );
                        }
                    }
                }
            }

            // Userspace GPU_PROMOTE_CTX path (works on pre-Blackwell).
            let descs = match client.query_gr_context_buffers_info(h_subdevice) {
                Ok(d) if !d.is_empty() => {
                    eprintln!(
                        "[coral-driver] GPU_PROMOTE_CTX: {} buffers from RM query",
                        d.len()
                    );
                    d
                }
                other => {
                    if let Err(e) = &other {
                        eprintln!("[coral-driver] KGR_GET_CONTEXT_BUFFERS_INFO failed: {e}");
                    }
                    eprintln!(
                        "[coral-driver] Using hardcoded Blackwell context buffer sizes"
                    );
                    crate::nv::uvm::rm_client::alloc::hardcoded_blackwell_ctx_buffers()
                }
            };

            use crate::nv::uvm::structs::PromoteCtxBufferEntry;

            let mut promote_entries: Vec<PromoteCtxBufferEntry> = Vec::new();
            let mut allocated: Vec<CtxBuffer> = Vec::new();
            let mut ctx_handle_counter = h_device + 0x7000_u32;

            for desc in &descs {
                let h_mem = ctx_handle_counter;
                ctx_handle_counter += 1;

                if let Err(e) = client.alloc_system_memory(h_device, h_mem, desc.size) {
                    eprintln!(
                        "[coral-driver]   FAILED alloc ctx_buf id={}: {e}",
                        desc.buffer_id
                    );
                    continue;
                }

                let gpu_va = if desc.is_nonmapped {
                    0_u64
                } else {
                    match client.rm_map_memory_dma(h_device, h_virt_mem, h_mem, 0, desc.size) {
                        Ok(va) => va,
                        Err(e) => {
                            eprintln!(
                                "[coral-driver]   FAILED map ctx_buf id={}: {e}",
                                desc.buffer_id
                            );
                            client.free_object(h_device, h_mem).ok();
                            continue;
                        }
                    }
                };

                eprintln!(
                    "[coral-driver]   ctx_buf id={} gpu_va=0x{gpu_va:016X} size=0x{:X}",
                    desc.buffer_id, desc.size
                );

                let mut entry = PromoteCtxBufferEntry::default();
                entry.gpu_virt_addr = gpu_va;
                entry.buffer_id = desc.buffer_id;
                entry.b_initialize = u8::from(desc.needs_init);
                entry.b_nonmapped = u8::from(desc.is_nonmapped);
                if desc.needs_init {
                    entry.size = desc.size;
                    entry.phys_attr = 4;
                }
                promote_entries.push(entry);

                allocated.push(CtxBuffer {
                    buffer_id: desc.buffer_id,
                    h_memory: h_mem,
                    size: desc.size,
                    gpu_va,
                });
            }

            if !promote_entries.is_empty() {
                match client.gpu_promote_ctx(h_subdevice, h_channel, &promote_entries) {
                    Ok(()) => {
                        eprintln!(
                            "[coral-driver] GPU_PROMOTE_CTX: {} buffers promoted OK",
                            promote_entries.len()
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "[coral-driver] GPU_PROMOTE_CTX FAILED: {e} \
                             (kernel-only — will fall back to gr_ctxsw_setup_bind)"
                        );
                        for cb in &allocated {
                            if cb.gpu_va != 0 {
                                client
                                    .rm_unmap_memory_dma(h_device, h_virt_mem, cb.h_memory, cb.gpu_va)
                                    .ok();
                            }
                            client.free_object(h_device, cb.h_memory).ok();
                        }
                        break 'promote (Vec::new(), false);
                    }
                }
            }

            (allocated, false)
        };

        // Bind GR context-switch state. If kmod BindChannelResources succeeded,
        // the context is already bound — skip. Otherwise, if GPU_PROMOTE_CTX
        // succeeded pass the MAIN buffer VA; else use vMemPtr=0 (RM demand-pages).
        if kmod_bind_ok {
            eprintln!(
                "[coral-driver] Skipping gr_ctxsw_setup_bind \
                 (kmod BindChannelResources already bound context)"
            );
        } else {
            let main_ctx_va = ctx_buffers
                .iter()
                .find(|cb| cb.buffer_id == crate::nv::uvm::PROMOTE_CTX_BUFFER_ID_MAIN)
                .map_or(0_u64, |cb| cb.gpu_va);
            client.gr_ctxsw_setup_bind_with_mem(h_subdevice, h_channel, main_ctx_va)?;
        }

        client.tsg_gpfifo_schedule(h_changrp)?;

        let work_submit_token = match client.get_work_submit_token(h_channel) {
            Ok(t) => {
                tracing::info!(
                    token = format_args!("0x{t:08X}"),
                    "Work submit token acquired"
                );
                t
            }
            Err(e) => {
                tracing::warn!("get_work_submit_token failed ({e}), using cid");
                hw_channel_id
            }
        };

        // Allocate VOLTA_USERMODE_A to get the doorbell register mapping.
        let h_usermode = h_device + 0x5003;
        client.rm_alloc_simple(
            h_subdevice,
            h_usermode,
            VOLTA_USERMODE_A,
            "RM_ALLOC(VOLTA_USERMODE_A)",
        )?;

        // Map the usermode object to get the doorbell page in CPU space.
        // USERMODE is a BAR-mapped object — must use the GPU device fd.
        let usermode_mmap_fd = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(format!("/dev/nvidia{gpu_index}"))
            .map_err(|e| {
                DriverError::DeviceNotFound(format!("nvidia{gpu_index} for doorbell: {e}").into())
            })?;
        let doorbell_addr = client.rm_map_memory_on_fd(
            usermode_mmap_fd.as_raw_fd(),
            h_device,
            h_usermode,
            0,
            4096,
        )?;

        // Blackwell (clca6f) removed GP_GET from the USERD control struct —
        // the GPU no longer writes GP_GET to USERD. We must use a semaphore
        // release written by the GPU into a separate fence buffer.
        let uses_semaphore_fence = matches!(gpu_gen, GpuGen::BlackwellA | GpuGen::BlackwellB);

        let (fence_cpu_addr, fence_gpu_va, fence_mmap_fd, fence_pb_cpu_addr, fence_pb_gpu_va, fence_pb_mmap_fd) = if uses_semaphore_fence {
            // Fence value buffer: GPU writes semaphore payload here.
            let h_fence_mem = h_device + 0x5005;
            client.alloc_system_memory(h_device, h_fence_mem, 4096)?;
            let fence_va =
                client.rm_map_memory_dma(h_device, h_virt_mem, h_fence_mem, 0, 4096)?;
            let fence_fd = open_ctl()?;
            let fence_cpu = client.rm_map_memory_on_fd(
                fence_fd.as_raw_fd(),
                h_device,
                h_fence_mem,
                0,
                4096,
            )?;
            unsafe { VolatilePtr::new(fence_cpu as *mut u32).write(0) };

            // Fence push buffer: rewritten before each fence submission.
            let h_fence_pb = h_device + 0x5006;
            client.alloc_system_memory(h_device, h_fence_pb, 4096)?;
            let fpb_va =
                client.rm_map_memory_dma(h_device, h_virt_mem, h_fence_pb, 0, 4096)?;
            let fpb_fd = open_ctl()?;
            let fpb_cpu = client.rm_map_memory_on_fd(
                fpb_fd.as_raw_fd(),
                h_device,
                h_fence_pb,
                0,
                4096,
            )?;

            tracing::info!(
                fence_va = format_args!("0x{fence_va:016X}"),
                fpb_va = format_args!("0x{fpb_va:016X}"),
                "Blackwell semaphore fence allocated (GP_GET unavailable)"
            );
            (fence_cpu, fence_va, Some(fence_fd), fpb_cpu, fpb_va, Some(fpb_fd))
        } else {
            (0, 0, None, 0, 0, None)
        };

        tracing::info!(
            gpu_index,
            sm,
            h_device = format_args!("0x{h_device:08X}"),
            h_channel = format_args!("0x{h_channel:08X}"),
            h_compute = format_args!("0x{h_compute:08X}"),
            work_submit_token = format_args!("0x{work_submit_token:08X}"),
            uses_semaphore_fence,
            "NvUvmComputeDevice fully initialized"
        );

        // Smoke-test: submit a NOP push buffer to verify the GPFIFO mechanism.
        let mut dev = Self {
            client,
            uvm,
            gpu,
            gpu_gen,
            h_device,
            h_subdevice,
            h_vaspace,
            h_changrp,
            h_channel,
            h_compute,
            gpu_uuid,
            buffers: HashMap::new(),
            ctx_buffers,
            next_handle: 1,
            next_mem_handle: h_device + 0x6000,
            inflight: Vec::new(),
            deferred_free: Vec::new(),
            userd_cpu_addr,
            gpfifo_cpu_addr,
            gp_put: 0,
            h_virt_mem,
            userd_mmap_fd,
            gpfifo_mmap_fd,
            errnotif_cpu_addr,
            errnotif_mmap_fd,
            usermode_mmap_fd,
            doorbell_addr,
            work_submit_token,
            uses_semaphore_fence,
            fence_cpu_addr,
            fence_gpu_va,
            fence_value: 0,
            fence_mmap_fd,
            fence_pb_cpu_addr,
            fence_pb_gpu_va,
            fence_pb_mmap_fd,
            coral_kmod: None,
            kmod_h_client: 0,
        };

        // NOP smoke test: submit a push buffer to verify the GPFIFO mechanism.
        // On Blackwell+, we embed a semaphore release so the fence value
        // advances (since GP_GET is no longer in USERD).
        let nop_h_mem = h_device + 0x5FFF;
        dev.client.alloc_system_memory(h_device, nop_h_mem, 4096)?;
        let nop_gpu_va = dev
            .client
            .rm_map_memory_dma(h_device, h_virt_mem, nop_h_mem, 0, 4096)?;
        let nop_fd = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/nvidiactl")
            .map_err(|e| DriverError::DeviceNotFound(format!("nvidiactl: {e}").into()))?;
        let nop_cpu =
            dev.client
                .rm_map_memory_on_fd(nop_fd.as_raw_fd(), h_device, nop_h_mem, 0, 4096)?;

        let pb_dwords = if dev.uses_semaphore_fence {
            dev.fence_value += 1;
            let fv = dev.fence_value;
            let fva = dev.fence_gpu_va;
            // Build a semaphore release push buffer:
            //   SEM_ADDR_LO, SEM_ADDR_HI, SEM_PAYLOAD_LO, SEM_PAYLOAD_HI, SEM_EXECUTE
            // Method header: incrementing method, subchannel 0, address 0x5c>>2=0x17,
            //   count=5, SEC_OP=INC_METHOD (1<<29)
            let pb = nop_cpu as *mut u32;
            // SAFETY: pb is valid for at least 4096 bytes; we write 6 dwords (24 bytes).
            unsafe {
                // Method header: SEC_OP=1 (INC_METHOD), count=5, subchannel=0, address=0x17
                // Bits [31:29]=001, [28:16]=5, [15:13]=0, [11:0]=0x17
                VolatilePtr::new(pb).write((1 << 29) | (5 << 16) | 0x17);
                // SEM_ADDR_LO = lower 32 bits of fence_gpu_va (bits [31:2], dword-aligned)
                VolatilePtr::new(pb.add(1)).write((fva & 0xFFFF_FFFC) as u32);
                // SEM_ADDR_HI = upper bits
                VolatilePtr::new(pb.add(2)).write((fva >> 32) as u32);
                // SEM_PAYLOAD_LO = fence value
                VolatilePtr::new(pb.add(3)).write(fv);
                // SEM_PAYLOAD_HI = 0
                VolatilePtr::new(pb.add(4)).write(0);
                // SEM_EXECUTE: OPERATION=RELEASE(1), PAYLOAD_SIZE=32BIT(0)
                VolatilePtr::new(pb.add(5)).write(1);
            }
            6_u32
        } else {
            // Pre-Blackwell: a single NOP dword suffices.
            // SAFETY: nop_cpu is valid for 4096 bytes.
            unsafe { VolatilePtr::new(nop_cpu as *mut u32).write(0) };
            1_u32
        };

        dev.submit_gpfifo(nop_gpu_va, pb_dwords)?;
        dev.poll_gpfifo_completion()?;
        tracing::info!("NOP smoke test passed — GPFIFO pipeline operational");

        // Allocate a Shader Local Memory (SLM) buffer for per-warp scratch
        // and call/return stack (CRS). Even shaders that don't use local
        // memory need a valid SLM base address — the SM reads it during warp
        // launch and faults if it's unmapped.
        //
        // NVK computes per-TPC size as `bytes_per_warp * max_warps_per_sm * sms_per_tpc`.
        // We allocate a generous 2 MiB buffer and set per-TPC to 32 KiB * 0xFF.
        let h_slm_mem = h_device + 0x5FFD;
        let slm_size: u64 = 2 * 1024 * 1024; // 2 MiB
        dev.client
            .alloc_system_memory(h_device, h_slm_mem, slm_size)?;
        let slm_gpu_va =
            dev.gpu_map_buffer(h_slm_mem, slm_size)?;
        tracing::info!(
            slm_gpu_va = format_args!("0x{slm_gpu_va:016X}"),
            slm_size,
            "SLM buffer allocated for per-warp scratch/CRS"
        );

        // per-TPC limit: align to 0x8000 (32 KiB, NVK convention).
        let slm_per_tpc: u64 = 0x8000;

        // One-time compute init: bind the compute class to subchannel 1 and
        // configure shared/local memory windows + SLM base. This must happen
        // exactly once per channel — repeated SET_OBJECT calls on Blackwell
        // corrupt the channel state (GR_CLASS_ERROR 0x0D).
        {
            use crate::nv::pushbuf::PushBuf;

            let init_pb =
                PushBuf::compute_init(compute_class, 0xFF00_0000, slm_gpu_va, slm_per_tpc);
            let init_bytes = init_pb.as_bytes();
            let init_len = u32::try_from(init_pb.as_words().len())
                .map_err(|_| DriverError::platform_overflow("init pb dwords fits u32"))?;

            let h_init_mem = h_device + 0x5FFE;
            dev.client
                .alloc_system_memory(h_device, h_init_mem, 4096)?;
            let init_gpu_va = dev
                .client
                .rm_map_memory_dma(h_device, h_virt_mem, h_init_mem, 0, 4096)?;
            let init_fd = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/nvidiactl")
                .map_err(|e| {
                    DriverError::DeviceNotFound(format!("nvidiactl: {e}").into())
                })?;
            let init_cpu = dev.client.rm_map_memory_on_fd(
                init_fd.as_raw_fd(),
                h_device,
                h_init_mem,
                0,
                4096,
            )?;

            // SAFETY: init_cpu is a valid 4096-byte mapping; init_bytes is <= 4096.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    init_bytes.as_ptr(),
                    init_cpu as *mut u8,
                    init_bytes.len(),
                );
            }

            dev.submit_gpfifo(init_gpu_va, init_len)?;

            if dev.uses_semaphore_fence {
                dev.submit_fence_release()?;
            }

            dev.poll_gpfifo_completion()?;
            tracing::info!(
                compute_class = format_args!("0x{compute_class:04X}"),
                "Compute init submitted — SET_OBJECT + memory windows on subchannel 1"
            );

            dev.client
                .rm_unmap_memory(h_device, h_init_mem, init_cpu)
                .ok();
            dev.client.free_object(h_device, h_init_mem).ok();
            drop(init_fd);
        }

        dev.client
            .rm_unmap_memory(h_device, nop_h_mem, nop_cpu)
            .ok();
        dev.client.free_object(h_device, nop_h_mem).ok();
        drop(nop_fd);

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
                VolatilePtr::new(base as *mut u32).read(),
                VolatilePtr::new(base.add(1) as *mut u32).read(),
                VolatilePtr::new(base.add(2) as *mut u32).read(),
                VolatilePtr::new(base.add(3) as *mut u32).read(),
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
