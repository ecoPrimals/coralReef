// SPDX-License-Identifier: AGPL-3.0-or-later
//! Direct userspace RM + UVM initialization for [`NvUvmComputeDevice`](super::NvUvmComputeDevice).

use std::collections::HashMap;
use std::os::fd::AsRawFd;

use crate::error::{DriverError, DriverResult};
use crate::mmio::VolatilePtr;
use crate::nv::uvm::constants::{nv_ctl_path, nv_gpu_path_prefix};
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

        // VA space strategy:
        //
        // Blackwell (SM >= 100): faulting VA space (0x48) + UVM registration.
        // The kmod's COMPLETE_INIT path uses demand-paged context buffers
        // via UVM — no GPU_PROMOTE_CTX needed.  All user buffers (GPFIFO,
        // fence, push buffers) are UVM-externally-mapped so PBDMA can
        // fetch them without faulting.  GR context buffers are allocated
        // on demand by GSP-RM, with UVM servicing the page faults.
        //
        // Pre-Blackwell: non-faulting VA space (flags=0) with DMA mapping.
        let is_blackwell_plus = sm >= 100;
        let uses_uvm_mapping = is_blackwell_plus;

        let h_vaspace = if is_blackwell_plus {
            let h = client.alloc_vaspace_for_uvm(h_device)?;
            eprintln!("[coral-driver] Blackwell faulting VA space allocated (0x48)");
            h
        } else {
            let h = client.alloc_vaspace(h_device)?;
            eprintln!("[coral-driver] VA space allocated (non-faulting)");
            h
        };

        // Register the GPU VA space with UVM so it can service page faults
        // for demand-paged GR context buffers and UVM-mapped user buffers.
        if is_blackwell_plus {
            match uvm.register_gpu_vaspace(
                &gpu_uuid,
                client.ctl_fd(),
                client.handle(),
                h_vaspace,
            ) {
                Ok(()) => eprintln!("[coral-driver] UVM_REGISTER_GPU_VASPACE OK"),
                Err(e) => {
                    eprintln!("[coral-driver] UVM_REGISTER_GPU_VASPACE failed: {e} (continuing)");
                }
            }
        }

        match uvm.pageable_mem_access() {
            Ok(supported) => tracing::debug!(supported, "UVM_PAGEABLE_MEM_ACCESS OK"),
            Err(e) => tracing::warn!("UVM_PAGEABLE_MEM_ACCESS failed: {e} (continuing)"),
        }

        // Blackwell compute TSGs: hVASpace=0 (VA space comes from context share).
        // CUDA trace confirms first compute TSG has hVASpace=0 while CE TSGs
        // pass the VA space handle directly.
        let tsg_vaspace = if is_blackwell_plus { 0 } else { h_vaspace };
        let h_changrp = client.alloc_channel_group(h_device, tsg_vaspace)?;
        tracing::debug!(
            h_changrp = format_args!("0x{h_changrp:08X}"),
            "channel_group OK"
        );

        let h_ctxshare = client.alloc_context_share(h_changrp, h_vaspace, is_blackwell_plus)?;
        tracing::debug!(
            h_ctxshare = format_args!("0x{h_ctxshare:08X}"),
            "context_share OK"
        );

        // For non-faulting VA: allocate NV01_MEMORY_VIRTUAL for DMA mappings.
        // For faulting (Blackwell): skip — UVM manages GPU page tables.
        let h_virt_mem = if is_blackwell_plus {
            0_u32
        } else {
            client.alloc_virtual_memory(h_device, h_virt_mem, h_vaspace)?;
            tracing::debug!("virtual_memory OK");
            h_virt_mem
        };

        // Map GPFIFO into GPU VA space.
        // Blackwell: UVM external mapping (must happen before channel creation).
        // Pre-Blackwell: DMA mapping via NV01_MEMORY_VIRTUAL.
        let mut uvm_va_next: u64 = 0x1_0000_0000;
        let gpfifo_gpu_va = if is_blackwell_plus {
            use crate::nv::uvm::ExternalMapping;
            use super::super::types::page_align;
            let gpfifo_size = page_align(GPFIFO_SIZE.max(512 * 8));
            let va = uvm_va_next;
            uvm.create_external_range(va, gpfifo_size)?;
            uvm.map_external_allocation(&ExternalMapping {
                base: va,
                length: gpfifo_size,
                offset: 0,
                rm_ctrl_fd: client.ctl_fd(),
                h_client: client.handle(),
                h_memory: h_gpfifo_mem,
                gpu_uuid: &gpu_uuid,
            })?;
            uvm_va_next = va + gpfifo_size;
            eprintln!("[coral-driver] GPFIFO UVM-mapped at 0x{va:016X}");
            va
        } else {
            client.rm_map_memory_dma(h_device, h_virt_mem, h_gpfifo_mem, 0, GPFIFO_SIZE)?
        };
        tracing::debug!(gpfifo_gpu_va = format_args!("0x{gpfifo_gpu_va:016X}"));

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
        let ctl_path = nv_ctl_path();
        let open_ctl = || -> DriverResult<std::fs::File> {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(ctl_path)
                .map_err(|e| DriverError::DeviceNotFound(format!("{ctl_path} for mmap: {e}").into()))
        };
        // VRAM buffers must be mapped on the GPU device fd (BAR1), not nvidiactl.
        let gpu_path = format!("{}{gpu_index}", nv_gpu_path_prefix());
        let userd_mmap_fd = if userd_in_vram {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&gpu_path)
                .map_err(|e| {
                    DriverError::DeviceNotFound(format!("{gpu_path} for USERD: {e}").into())
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

        eprintln!("[coral-driver] alloc_compute_engine: h_channel=0x{h_channel:08X} class=0x{compute_class:04X}");
        let h_compute = client.alloc_compute_engine(h_channel, compute_class)?;
        eprintln!("[coral-driver] alloc_compute_engine OK: h_compute=0x{h_compute:08X}");

        eprintln!("[coral-driver] channel_bind_engine: h_compute=0x{h_compute:08X} class=0x{compute_class:04X} engine_type=1 (GR0)");
        client.channel_bind_engine(h_channel, h_compute, compute_class, 1)?;
        eprintln!("[coral-driver] channel_bind_engine OK");

        // ── Blackwell post-bind: work submit token + ctx buffer query ──
        //
        // CUDA 580.x trace shows these two controls after engine bind
        // and before TSG schedule. GR_CTXSW_SETUP_BIND (0x2080123A) is
        // NOT called by CUDA — the RM_ALLOC of the compute class + bind
        // is sufficient; GSP-RM creates context buffers lazily via UVM
        // page faults when the first dispatch hits.
        if is_blackwell_plus {
            let token = 0x4000_0005_u32 + (h_channel & 0xFF);
            match client.set_work_submit_token(h_channel, token) {
                Ok(()) => eprintln!("[coral-driver] SET_WORK_SUBMIT_TOKEN OK (0x{token:08X})"),
                Err(e) => eprintln!("[coral-driver] SET_WORK_SUBMIT_TOKEN failed: {e} (non-fatal)"),
            }
            match client.gr_get_ctx_buffer_size(h_subdevice, h_channel) {
                Ok(sz) => eprintln!("[coral-driver] GR_GET_CTX_BUFFER_SIZE: {sz} bytes (0x{sz:X})"),
                Err(e) => eprintln!("[coral-driver] GR_GET_CTX_BUFFER_SIZE failed: {e} (non-fatal)"),
            }
        }

        // ── Context buffer binding ────────────────────────────────────
        //
        // Blackwell (faulting VA): call gr_ctxsw_setup_bind (demand-paged)
        // to trigger GSP-RM context buffer allocation. Without this, the
        // GR engine rejects SET_OBJECT because context buffers don't exist.
        //
        // Pre-Blackwell (non-faulting VA): explicit alloc + DMA map +
        // gr_ctxsw_setup_bind_with_mem.
        let ctx_buffers = if is_blackwell_plus {
            match client.gr_ctxsw_setup_bind(h_subdevice, h_channel) {
                Ok(()) => eprintln!("[coral-driver] gr_ctxsw_setup_bind (demand-paged) OK"),
                Err(e) => eprintln!("[coral-driver] gr_ctxsw_setup_bind failed: {e} (continuing)"),
            }
            Vec::<CtxBuffer>::new()
        } else {
            let ctx_size: u64 = 16 * 1024 * 1024;
            let h_ctx_mem = h_device + 0x7000;
            match client.alloc_system_memory(h_device, h_ctx_mem, ctx_size) {
                Ok(_) => {
                    match client.rm_map_memory_dma(h_device, h_virt_mem, h_ctx_mem, 0, ctx_size) {
                        Ok(ctx_va) => {
                            eprintln!("[coral-driver] GR context buffer: va=0x{ctx_va:X} size=0x{ctx_size:X}");
                            match client.gr_ctxsw_setup_bind_with_mem(h_subdevice, h_channel, ctx_va) {
                                Ok(()) => eprintln!("[coral-driver] gr_ctxsw_setup_bind_with_mem OK"),
                                Err(e) => {
                                    eprintln!("[coral-driver] gr_ctxsw_setup_bind_with_mem failed: {e}");
                                    client.gr_ctxsw_setup_bind(h_subdevice, h_channel).ok();
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[coral-driver] DMA map GR ctx failed: {e}");
                            client.free_object(h_device, h_ctx_mem).ok();
                            client.gr_ctxsw_setup_bind(h_subdevice, h_channel).ok();
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[coral-driver] alloc GR ctx failed: {e}");
                    client.gr_ctxsw_setup_bind(h_subdevice, h_channel).ok();
                }
            }
            Vec::<CtxBuffer>::new()
        };

        // Register the channel with UVM so it can service page faults
        // for demand-paged GR context buffers during compute dispatch.
        if is_blackwell_plus {
            let chan_resource_range: u64 = 0x1000_0000; // 256 MiB
            let chan_resource_base =
                (uvm_va_next + chan_resource_range - 1) & !(chan_resource_range - 1);
            uvm_va_next = chan_resource_base + chan_resource_range;

            match uvm.register_channel(
                &gpu_uuid,
                client.ctl_fd(),
                client.handle(),
                h_channel,
                chan_resource_base,
                chan_resource_range,
            ) {
                Ok(()) => eprintln!("[coral-driver] UVM_REGISTER_CHANNEL OK: base=0x{chan_resource_base:X}"),
                Err(e) => eprintln!("[coral-driver] UVM_REGISTER_CHANNEL failed: {e} (continuing)"),
            }
        }

        client.tsg_gpfifo_schedule(h_changrp).map_err(|e| {
            eprintln!("[coral-driver] tsg_gpfifo_schedule FAILED: {e}");
            e
        })?;
        eprintln!("[coral-driver] tsg_gpfifo_schedule OK");

        // Set context-switch preemption mode (CUDA does this after schedule)
        if is_blackwell_plus {
            match client.gr_set_ctxsw_preemption_mode(h_subdevice, h_changrp) {
                Ok(()) => eprintln!("[coral-driver] GR_SET_CTXSW_PREEMPTION_MODE OK"),
                Err(e) => eprintln!("[coral-driver] GR_SET_CTXSW_PREEMPTION_MODE failed: {e} (non-fatal)"),
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
        let doorbell_gpu_path = format!("{}{gpu_index}", nv_gpu_path_prefix());
        let usermode_mmap_fd = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&doorbell_gpu_path)
            .map_err(|e| {
                DriverError::DeviceNotFound(
                    format!("{doorbell_gpu_path} for doorbell: {e}").into(),
                )
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
            let h_fence_mem = h_device + 0x5005;
            client.alloc_system_memory(h_device, h_fence_mem, 4096)?;

            let h_fence_pb = h_device + 0x5006;
            client.alloc_system_memory(h_device, h_fence_pb, 4096)?;

            let (fence_va, fpb_va) = if is_blackwell_plus {
                use crate::nv::uvm::ExternalMapping;
                use super::super::types::page_align;

                let fv_va = uvm_va_next;
                uvm.create_external_range(fv_va, page_align(4096))?;
                uvm.map_external_allocation(&ExternalMapping {
                    base: fv_va,
                    length: page_align(4096),
                    offset: 0,
                    rm_ctrl_fd: client.ctl_fd(),
                    h_client: client.handle(),
                    h_memory: h_fence_mem,
                    gpu_uuid: &gpu_uuid,
                })?;
                uvm_va_next = fv_va + page_align(4096);

                let fp_va = uvm_va_next;
                uvm.create_external_range(fp_va, page_align(4096))?;
                uvm.map_external_allocation(&ExternalMapping {
                    base: fp_va,
                    length: page_align(4096),
                    offset: 0,
                    rm_ctrl_fd: client.ctl_fd(),
                    h_client: client.handle(),
                    h_memory: h_fence_pb,
                    gpu_uuid: &gpu_uuid,
                })?;
                uvm_va_next = fp_va + page_align(4096);
                eprintln!("[coral-driver] fence UVM-mapped: val=0x{fv_va:016X} pb=0x{fp_va:016X}");
                (fv_va, fp_va)
            } else {
                let fv = client.rm_map_memory_dma(h_device, h_virt_mem, h_fence_mem, 0, 4096)?;
                let fp = client.rm_map_memory_dma(h_device, h_virt_mem, h_fence_pb, 0, 4096)?;
                (fv, fp)
            };

            let fence_fd = open_ctl()?;
            let fence_cpu =
                client.rm_map_memory_on_fd(fence_fd.as_raw_fd(), h_device, h_fence_mem, 0, 4096)?;
            // SAFETY: fence_cpu is a valid mmap'd pointer from rm_map_memory_on_fd,
            // aligned to page boundary (4096 bytes), writable.
            unsafe { VolatilePtr::new(fence_cpu as *mut u32).write(0) };

            let fpb_fd = open_ctl()?;
            let fpb_cpu =
                client.rm_map_memory_on_fd(fpb_fd.as_raw_fd(), h_device, h_fence_pb, 0, 4096)?;

            tracing::info!(
                fence_va = format_args!("0x{fence_va:016X}"),
                fpb_va = format_args!("0x{fpb_va:016X}"),
                "semaphore fence allocated"
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
            compute_subchannel: if uses_semaphore_fence { 1 } else { 0 },
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
            .open(nv_ctl_path())
            .map_err(|e| {
                DriverError::DeviceNotFound(format!("{}: {e}", nv_ctl_path()).into())
            })?;
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

        eprintln!("[coral-driver] NOP submit: nop_gpu_va=0x{nop_gpu_va:X} fence_gpu_va=0x{:X} pb_dwords={pb_dwords}", dev.fence_gpu_va);
        dev.submit_gpfifo(nop_gpu_va, pb_dwords)?;
        dev.poll_gpfifo_completion()?;
        eprintln!("[coral-driver] NOP smoke test passed");
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

        // One-time compute init: SET_OBJECT + memory windows + SLM base.
        //
        // Blackwell: SET_OBJECT on subchannel 1 (subchannel 0 causes Xid 13
        // "Class Mismatch"). Memory window addresses must be set or the SM
        // faults with "Invalid Address Space" during warp setup.
        //
        // Pre-Blackwell: SET_OBJECT on subchannel 0.
        {
            use crate::nv::pushbuf::PushBuf;

            // High VA bases for per-warp local and per-CTA shared memory
            // address translation. These don't need UVM mapping — UVM
            // faulting resolves page faults lazily when a shader actually
            // accesses local/shared memory through these windows.
            const LOCAL_MEM_WINDOW: u64 = 0x7293_0000_0000;
            const SHARED_MEM_WINDOW: u64 = 0x7294_0000_0000;

            let init_pb = if is_blackwell_plus {
                PushBuf::compute_init_subchannel(
                    compute_class,
                    LOCAL_MEM_WINDOW,
                    SHARED_MEM_WINDOW,
                    slm_gpu_va,
                    slm_per_tpc,
                    1,
                )
            } else {
                PushBuf::compute_init(compute_class, 0xFF00_0000, slm_gpu_va, slm_per_tpc)
            };
            let init_bytes = init_pb.as_bytes();
            let init_len = u32::try_from(init_pb.as_words().len())
                .map_err(|_| DriverError::platform_overflow("init pb dwords fits u32"))?;

            let h_init_mem = h_device + 0x5FFE;
            dev.client.alloc_system_memory(h_device, h_init_mem, 4096)?;
            let init_gpu_va = dev.gpu_map_buffer_infra(h_init_mem, 4096)?;
            let init_fd = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(nv_ctl_path())
                .map_err(|e| {
                    DriverError::DeviceNotFound(format!("{}: {e}", nv_ctl_path()).into())
                })?;
            let init_cpu = dev.client.rm_map_memory_on_fd(
                init_fd.as_raw_fd(),
                h_device,
                h_init_mem,
                0,
                4096,
            )?;

            // SAFETY: init_cpu is a valid mmap'd pointer (4096 bytes writable),
            // init_bytes.len() <= 4096, and the regions do not overlap.
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
            eprintln!("[coral-driver] compute_init OK (SET_OBJECT + SLM)");

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
