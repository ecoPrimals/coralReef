// SPDX-License-Identifier: AGPL-3.0-or-later
//! Direct userspace RM + UVM initialization for [`NvUvmComputeDevice`](super::NvUvmComputeDevice).

use std::collections::HashMap;
use std::os::fd::AsRawFd;

use crate::error::{DriverError, DriverResult};
use crate::mmio::VolatilePtr;
use crate::nv::uvm::{NvGpuDevice, NvUvmDevice, RmClient, VOLTA_USERMODE_A};

use super::super::types::{CtxBuffer, GPFIFO_ENTRIES, GPFIFO_SIZE, GpuGen, USERD_SIZE};
use super::NvUvmComputeDevice;

impl NvUvmComputeDevice {
    /// Open via direct userspace RM path (original implementation).
    pub(super) fn open_userspace(gpu_index: u32, sm: u32) -> DriverResult<Self> {
        let gpu_gen = GpuGen::from_sm(sm);

        let mut client = RmClient::new()?;
        let mut uvm = NvUvmDevice::open()?;
        let gpu = NvGpuDevice::open(gpu_index)?;
        gpu.register_fd(client.ctl_fd())?;

        uvm.initialize()?;

        match uvm.mm_initialize() {
            Ok(true) => tracing::debug!("UVM_MM_INITIALIZE OK (mm pinned)"),
            Ok(false) => tracing::debug!("UVM_MM_INITIALIZE: platform doesn't need it"),
            Err(e) => tracing::warn!("UVM_MM_INITIALIZE failed: {e} (continuing)"),
        }

        let h_device = client.alloc_device(gpu_index)?;
        let h_subdevice = client.alloc_subdevice(h_device)?;

        let h_userd_mem = h_device + 0x5000;
        let h_gpfifo_mem = h_device + 0x5001;
        let h_virt_mem = h_device + 0x5002;

        let userd_vram_size: u64 = 0x20_0000; // 2 MiB like CUDA
        let userd_in_vram = match client.alloc_local_memory(h_device, h_userd_mem, userd_vram_size)
        {
            Ok(_) => {
                tracing::debug!("USERD allocated in VRAM (2 MiB)");
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

        let gpu_uuid = client.query_gpu_uuid(h_subdevice)?;

        {
            use crate::nv::uvm::UVM_REGISTER_GPU;
            use crate::nv::uvm::nv_status::NV_OK;
            use crate::nv::uvm::structs::UvmRegisterGpuParams;
            let mut reg = UvmRegisterGpuParams::default();
            reg.gpu_uuid = gpu_uuid;
            reg.rm_ctrl_fd = client.ctl_fd();
            reg.h_client = client.handle();
            reg.rm_status = 0xDEAD_BEEF;
            match uvm.raw_ioctl(UVM_REGISTER_GPU, &mut reg, "UVM_REGISTER_GPU") {
                Ok(()) if reg.rm_status == NV_OK => {
                    tracing::debug!(
                        numa_enabled = reg.numa_enabled,
                        numa_node_id = reg.numa_node_id,
                        "UVM_REGISTER_GPU OK"
                    );
                }
                Ok(()) => {
                    tracing::warn!(
                        status = format_args!("0x{:08X}", reg.rm_status),
                        "UVM_REGISTER_GPU returned non-OK status"
                    );
                }
                Err(e) => {
                    tracing::warn!("UVM_REGISTER_GPU failed: {e}");
                }
            }
        }

        // VA space strategy for Blackwell:
        //
        // Blackwell requires flags=0x48 (IS_EXTERNALLY_OWNED | ENABLE_FAULTING_EXTERNAL)
        // for UVM_REGISTER_GPU_VASPACE. With this flag, UVM manages page tables
        // (demand-faulting context buffers and shader memory). RM_MAP_MEMORY_DMA
        // is NOT supported on externally-owned VA spaces — all GPU VA mappings
        // must use UVM_CREATE_EXTERNAL_RANGE + UVM_MAP_EXTERNAL_ALLOCATION.
        //
        // Pre-Blackwell: use flags=0x04 (RM-managed faulting) with RM_MAP_MEMORY_DMA.
        use crate::nv::uvm::{
            NV_VASPACE_FLAGS_BLACKWELL_FAULTING, NV_VASPACE_FLAGS_ENABLE_FAULTING,
        };

        let profile = crate::nv::generation::profile_for_sm(sm);
        let is_blackwell_plus = matches!(
            profile.boot_strategy,
            crate::nv::generation::BootStrategy::KmodPromote
        );

        let (h_vaspace, uses_uvm_mapping) = if is_blackwell_plus {
            match client.alloc_vaspace_with_flags(h_device, NV_VASPACE_FLAGS_BLACKWELL_FAULTING) {
                Ok(h) => {
                    tracing::debug!(
                        h_vaspace = format_args!("0x{h:08X}"),
                        "VA space BLACKWELL_FAULTING (0x48)"
                    );
                    match uvm.register_gpu_vaspace(&gpu_uuid, client.ctl_fd(), client.handle(), h) {
                        Ok(()) => {
                            tracing::debug!(
                                "UVM_REGISTER_GPU_VASPACE OK — UVM manages page tables"
                            );
                            (h, true)
                        }
                        Err(e) => {
                            tracing::warn!(
                                "UVM_REGISTER_GPU_VASPACE(0x48) failed: {e}, \
                                 falling back to RM-managed VA space"
                            );
                            client.free_object(h_device, h).ok();
                            let h2 = client
                                .alloc_vaspace_with_flags(
                                    h_device,
                                    NV_VASPACE_FLAGS_ENABLE_FAULTING,
                                )
                                .or_else(|_| client.alloc_vaspace(h_device))?;
                            tracing::debug!(
                                h_vaspace = format_args!("0x{h2:08X}"),
                                "VA space fallback ENABLE_FAULTING"
                            );
                            (h2, false)
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("alloc_vaspace(0x48) failed: {e}, using 0x04");
                    let h = client
                        .alloc_vaspace_with_flags(h_device, NV_VASPACE_FLAGS_ENABLE_FAULTING)
                        .or_else(|_| client.alloc_vaspace(h_device))?;
                    (h, false)
                }
            }
        } else {
            let h = client
                .alloc_vaspace_with_flags(h_device, NV_VASPACE_FLAGS_ENABLE_FAULTING)
                .or_else(|_| client.alloc_vaspace(h_device))?;
            match uvm.register_gpu_vaspace(&gpu_uuid, client.ctl_fd(), client.handle(), h) {
                Ok(()) => tracing::debug!("UVM_REGISTER_GPU_VASPACE OK"),
                Err(e) => tracing::warn!("UVM_REGISTER_GPU_VASPACE failed: {e} (non-fatal)"),
            }
            (h, false)
        };

        // GPU VA bump allocator base for UVM external mapping (4 GiB, well within the VA range)
        let mut uvm_va_next: u64 = 0x1_0000_0000;

        // CUDA calls UVM_PAGEABLE_MEM_ACCESS after UVM_REGISTER_GPU_VASPACE.
        match uvm.pageable_mem_access() {
            Ok(supported) => tracing::debug!(supported, "UVM_PAGEABLE_MEM_ACCESS OK"),
            Err(e) => tracing::warn!("UVM_PAGEABLE_MEM_ACCESS failed: {e} (continuing)"),
        }

        let h_changrp = client.alloc_channel_group(h_device, h_vaspace)?;
        tracing::debug!(
            h_changrp = format_args!("0x{h_changrp:08X}"),
            "channel_group OK"
        );

        let h_ctxshare = client.alloc_context_share(h_changrp, h_vaspace, is_blackwell_plus)?;
        tracing::debug!(
            h_ctxshare = format_args!("0x{h_ctxshare:08X}"),
            "context_share OK"
        );

        // NV01_MEMORY_VIRTUAL is required by RM even for externally-owned VA spaces.
        // It serves as a container for RM's internal VA space bookkeeping. On UVM mode,
        // we don't use it for DMA mapping but RM needs it for channel scheduling.
        match client.alloc_virtual_memory(h_device, h_virt_mem, h_vaspace) {
            Ok(_) => tracing::debug!("virtual_memory OK"),
            Err(e) => {
                if !uses_uvm_mapping {
                    return Err(e);
                }
                tracing::warn!("alloc_virtual_memory failed: {e} (non-fatal for UVM mode)");
            }
        }

        let gpfifo_gpu_va = if uses_uvm_mapping {
            use super::super::types::page_align;
            use crate::nv::uvm::ExternalMapping;
            let aligned = page_align(GPFIFO_SIZE);
            let va = uvm_va_next;
            uvm.create_external_range(va, aligned)?;
            uvm.map_external_allocation(&ExternalMapping {
                base: va,
                length: aligned,
                offset: 0,
                rm_ctrl_fd: client.ctl_fd(),
                h_client: client.handle(),
                h_memory: h_gpfifo_mem,
                gpu_uuid: &gpu_uuid,
            })?;
            uvm_va_next = va + aligned;
            va
        } else {
            client.rm_map_memory_dma(h_device, h_virt_mem, h_gpfifo_mem, 0, GPFIFO_SIZE)?
        };
        tracing::debug!(gpfifo_gpu_va = format_args!("0x{gpfifo_gpu_va:016X}"));

        // For externally-owned VA spaces, ALL RM allocations used by the channel
        // must have GPU VA mappings via UVM — otherwise RM refuses to schedule
        // the channel ("Cannot schedule externally-owned channel with unbound allocations").
        if uses_uvm_mapping {
            use super::super::types::page_align;
            use crate::nv::uvm::ExternalMapping;

            let userd_aligned = page_align(userd_vram_size);
            let userd_va = uvm_va_next;
            uvm.create_external_range(userd_va, userd_aligned)?;
            uvm.map_external_allocation(&ExternalMapping {
                base: userd_va,
                length: userd_aligned,
                offset: 0,
                rm_ctrl_fd: client.ctl_fd(),
                h_client: client.handle(),
                h_memory: h_userd_mem,
                gpu_uuid: &gpu_uuid,
            })?;
            uvm_va_next = userd_va + userd_aligned;
            tracing::debug!(
                userd_va = format_args!("0x{userd_va:016X}"),
                "USERD UVM mapped"
            );

            let errnotif_aligned = page_align(4096);
            let errnotif_va = uvm_va_next;
            uvm.create_external_range(errnotif_va, errnotif_aligned)?;
            uvm.map_external_allocation(&ExternalMapping {
                base: errnotif_va,
                length: errnotif_aligned,
                offset: 0,
                rm_ctrl_fd: client.ctl_fd(),
                h_client: client.handle(),
                h_memory: h_errnotif_mem,
                gpu_uuid: &gpu_uuid,
            })?;
            uvm_va_next = errnotif_va + errnotif_aligned;
            tracing::debug!(
                errnotif_va = format_args!("0x{errnotif_va:016X}"),
                "errnotif UVM mapped"
            );
        }

        let (h_channel, hw_channel_id) = client.alloc_gpfifo_channel(
            h_changrp,
            h_userd_mem,
            h_errnotif_mem,
            h_ctxshare,
            gpfifo_gpu_va,
            GPFIFO_ENTRIES,
            gpu_gen.channel_class(),
        )?;
        tracing::debug!(
            h_channel = format_args!("0x{h_channel:08X}"),
            hw_channel_id,
            "GPFIFO channel allocated"
        );

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

        tracing::debug!("USERD/GPFIFO CPU mapping done");
        let compute_class = gpu_gen.compute_class();
        let h_compute = client.alloc_compute_engine(h_channel, compute_class)?;
        tracing::debug!(
            h_compute = format_args!("0x{h_compute:08X}"),
            compute_class = format_args!("0x{compute_class:08X}"),
            "compute engine allocated"
        );

        client.channel_bind_engine(h_channel, h_compute, compute_class, 1)?;
        tracing::debug!("channel_bind_engine OK");

        // ── Context buffer binding ────────────────────────────────────
        //
        // Blackwell+ requires kernel privilege for context buffer promotion
        // (GPU_PROMOTE_CTX returns INSUFFICIENT_PERMISSIONS from userspace).
        //
        // Hybrid approach: if coral-kmod is loaded, use CORAL_IOCTL_BIND_CHANNEL
        // which calls nvUvmInterface{RetainChannel,BindChannelResources} from
        // kernel context. Falls back to userspace GPU_PROMOTE_CTX for older GPUs.
        let (ctx_buffers, kmod_bind_ok) = 'promote: {
            // Pre-Blackwell UVM: context buffers are demand-faulted by UVM.
            // Blackwell UVM still needs explicit promotion because UVM fault
            // servicing is incomplete — SM hits ESR 0x10 on first CBUF access.
            if uses_uvm_mapping && !is_blackwell_plus {
                tracing::debug!(
                    "UVM mapping active (pre-Blackwell) — skipping GPU_PROMOTE_CTX \
                     (context buffers will be demand-faulted)"
                );
                break 'promote (Vec::new(), false);
            }

            // Try kernel-privileged binding via coral-kmod (Blackwell+).
            if is_blackwell_plus && let Some(kmod) = crate::nv::coral_kmod::CoralKmod::try_open() {
                match kmod.bind_channel(&gpu_uuid, client.handle(), h_vaspace, h_channel, sm) {
                    Ok(result) => {
                        tracing::debug!(
                            resource_count = result.resource_count,
                            hw_channel_id = result.hw_channel_id,
                            engine_type = result.channel_engine_type,
                            tsg_id = result.tsg_id,
                            "BIND_CHANNEL via kmod OK"
                        );
                        let ctx = result
                            .resources
                            .iter()
                            .map(|r| CtxBuffer {
                                buffer_id: r.resource_id as u16,
                                h_memory: 0,
                                size: r.size,
                                gpu_va: r.gpu_va,
                            })
                            .collect::<Vec<_>>();
                        break 'promote (ctx, true);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "BIND_CHANNEL via kmod failed: {e}, \
                                 falling back to GPU_PROMOTE_CTX"
                        );
                    }
                }
            }

            // Userspace GPU_PROMOTE_CTX path (works on pre-Blackwell).
            let descs = match client.query_gr_context_buffers_info(h_subdevice) {
                Ok(d) if !d.is_empty() => {
                    tracing::debug!(count = d.len(), "GPU_PROMOTE_CTX: buffers from RM query");
                    d
                }
                other => {
                    if let Err(e) = &other {
                        tracing::warn!("KGR_GET_CONTEXT_BUFFERS_INFO failed: {e}");
                    }
                    tracing::debug!("Using hardcoded Blackwell context buffer sizes");
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
                    tracing::warn!(buffer_id = desc.buffer_id, "alloc ctx_buf failed: {e}");
                    continue;
                }

                let gpu_va = if desc.is_nonmapped {
                    0_u64
                } else {
                    match client.rm_map_memory_dma(h_device, h_virt_mem, h_mem, 0, desc.size) {
                        Ok(va) => va,
                        Err(e) => {
                            tracing::warn!(buffer_id = desc.buffer_id, "map ctx_buf failed: {e}");
                            client.free_object(h_device, h_mem).ok();
                            continue;
                        }
                    }
                };

                tracing::trace!(
                    buffer_id = desc.buffer_id,
                    gpu_va = format_args!("0x{gpu_va:016X}"),
                    size = format_args!("0x{:X}", desc.size),
                    "ctx_buf mapped"
                );

                let entry = PromoteCtxBufferEntry {
                    gpu_phys_addr: 0,
                    gpu_virt_addr: gpu_va,
                    size: if desc.needs_init { desc.size } else { 0 },
                    phys_attr: if desc.needs_init { 4 } else { 0 },
                    buffer_id: desc.buffer_id,
                    b_initialize: u8::from(desc.needs_init),
                    b_nonmapped: u8::from(desc.is_nonmapped),
                };
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
                        tracing::debug!(
                            count = promote_entries.len(),
                            "GPU_PROMOTE_CTX: buffers promoted OK"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "GPU_PROMOTE_CTX failed: {e} \
                             (kernel-only — will fall back to gr_ctxsw_setup_bind)"
                        );
                        for cb in &allocated {
                            if cb.gpu_va != 0 {
                                client
                                    .rm_unmap_memory_dma(
                                        h_device,
                                        h_virt_mem,
                                        cb.h_memory,
                                        cb.gpu_va,
                                    )
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

        if kmod_bind_ok {
            tracing::debug!(
                "Skipping gr_ctxsw_setup_bind (kmod BindChannelResources already bound)"
            );
        } else if !uses_uvm_mapping {
            let main_ctx_va = ctx_buffers
                .iter()
                .find(|cb| cb.buffer_id == crate::nv::uvm::PROMOTE_CTX_BUFFER_ID_MAIN)
                .map_or(0_u64, |cb| cb.gpu_va);
            tracing::debug!(
                h_channel = format_args!("0x{h_channel:08X}"),
                main_ctx_va = format_args!("0x{main_ctx_va:016X}"),
                "gr_ctxsw_setup_bind..."
            );
            client.gr_ctxsw_setup_bind_with_mem(h_subdevice, h_channel, main_ctx_va)?;
            tracing::debug!("gr_ctxsw_setup_bind OK");
        }

        // Register channel with UVM to bind internal RM allocations.
        // On Blackwell+ with externally-owned VA spaces, RM refuses to schedule
        // a channel whose internal allocations lack GPU VA bindings. UVM_REGISTER_CHANNEL
        // resolves this by having the UVM module call RetainChannel + BindChannelResources
        // using its own kernel session (same one from UVM_REGISTER_GPU_VASPACE).
        if uses_uvm_mapping {
            let chan_resource_range: u64 = 256 * 1024 * 1024;
            let chan_resource_base = uvm_va_next;
            uvm_va_next += chan_resource_range;

            tracing::debug!(
                base = format_args!("0x{chan_resource_base:X}"),
                len = format_args!("0x{chan_resource_range:X}"),
                h_client = format_args!("0x{:08X}", client.handle()),
                h_channel = format_args!("0x{h_channel:08X}"),
                "UVM_REGISTER_CHANNEL..."
            );
            uvm.register_channel(
                &gpu_uuid,
                client.ctl_fd(),
                client.handle(),
                h_channel,
                chan_resource_base,
                chan_resource_range,
            )?;
            tracing::debug!("UVM_REGISTER_CHANNEL OK — channel resources bound");
        }

        match client.tsg_gpfifo_schedule(h_changrp) {
            Ok(()) => tracing::debug!("tsg_gpfifo_schedule OK"),
            Err(e) => {
                if uses_uvm_mapping {
                    tracing::warn!(
                        "tsg_gpfifo_schedule failed: {e} \
                         (non-fatal on UVM mode — may be auto-scheduled)"
                    );
                } else {
                    return Err(e);
                }
            }
        }

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
        let uses_semaphore_fence = gpu_gen.uses_semaphore_fence();

        let (
            fence_cpu_addr,
            fence_gpu_va,
            fence_mmap_fd,
            fence_pb_cpu_addr,
            fence_pb_gpu_va,
            fence_pb_mmap_fd,
        ) = if uses_semaphore_fence {
            // Fence value buffer: GPU writes semaphore payload here.
            let h_fence_mem = h_device + 0x5005;
            client.alloc_system_memory(h_device, h_fence_mem, 4096)?;
            let fence_va = if uses_uvm_mapping {
                use super::super::types::page_align;
                use crate::nv::uvm::ExternalMapping;
                let aligned = page_align(4096);
                let va = uvm_va_next;
                uvm.create_external_range(va, aligned)?;
                uvm.map_external_allocation(&ExternalMapping {
                    base: va,
                    length: aligned,
                    offset: 0,
                    rm_ctrl_fd: client.ctl_fd(),
                    h_client: client.handle(),
                    h_memory: h_fence_mem,
                    gpu_uuid: &gpu_uuid,
                })?;
                uvm_va_next = va + aligned;
                va
            } else {
                client.rm_map_memory_dma(h_device, h_virt_mem, h_fence_mem, 0, 4096)?
            };
            let fence_fd = open_ctl()?;
            let fence_cpu =
                client.rm_map_memory_on_fd(fence_fd.as_raw_fd(), h_device, h_fence_mem, 0, 4096)?;
            // SAFETY: RM mapped the fence allocation into CPU VA; dword 0 is the fence payload.
            unsafe { VolatilePtr::new(fence_cpu as *mut u32).write(0) };

            // Fence push buffer: rewritten before each fence submission.
            let h_fence_pb = h_device + 0x5006;
            client.alloc_system_memory(h_device, h_fence_pb, 4096)?;
            let fpb_va = if uses_uvm_mapping {
                use super::super::types::page_align;
                use crate::nv::uvm::ExternalMapping;
                let aligned = page_align(4096);
                let va = uvm_va_next;
                uvm.create_external_range(va, aligned)?;
                uvm.map_external_allocation(&ExternalMapping {
                    base: va,
                    length: aligned,
                    offset: 0,
                    rm_ctrl_fd: client.ctl_fd(),
                    h_client: client.handle(),
                    h_memory: h_fence_pb,
                    gpu_uuid: &gpu_uuid,
                })?;
                uvm_va_next = va + aligned;
                va
            } else {
                client.rm_map_memory_dma(h_device, h_virt_mem, h_fence_pb, 0, 4096)?
            };
            let fpb_fd = open_ctl()?;
            let fpb_cpu =
                client.rm_map_memory_on_fd(fpb_fd.as_raw_fd(), h_device, h_fence_pb, 0, 4096)?;

            tracing::info!(
                fence_va = format_args!("0x{fence_va:016X}"),
                fpb_va = format_args!("0x{fpb_va:016X}"),
                "Blackwell semaphore fence allocated (GP_GET unavailable)"
            );
            (
                fence_cpu,
                fence_va,
                Some(fence_fd),
                fpb_cpu,
                fpb_va,
                Some(fpb_fd),
            )
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
            uses_uvm_mapping,
            uvm_va_next,
            caps: crate::nv::generation::profile_for_sm(sm).to_capabilities(),
        };

        // NOP smoke test: submit a push buffer to verify the GPFIFO mechanism.
        // On Blackwell+, we embed a semaphore release so the fence value
        // advances (since GP_GET is no longer in USERD).
        let nop_h_mem = h_device + 0x5FFF;
        dev.client.alloc_system_memory(h_device, nop_h_mem, 4096)?;
        let nop_gpu_va = dev.gpu_map_buffer_infra(nop_h_mem, 4096)?;
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
        let slm_gpu_va = dev.gpu_map_buffer(h_slm_mem, slm_size)?;
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
            dev.client.alloc_system_memory(h_device, h_init_mem, 4096)?;
            let init_gpu_va = dev.gpu_map_buffer_infra(h_init_mem, 4096)?;
            let init_fd = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/nvidiactl")
                .map_err(|e| DriverError::DeviceNotFound(format!("nvidiactl: {e}").into()))?;
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
}
