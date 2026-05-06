// SPDX-License-Identifier: AGPL-3.0-or-later
//! Coral-kmod initialization path for [`NvUvmComputeDevice`](super::NvUvmComputeDevice).

use std::collections::HashMap;
use std::os::fd::AsRawFd;

use crate::error::{DriverError, DriverResult};
use crate::mmio::VolatilePtr;
use crate::nv::uvm::{NvGpuDevice, NvUvmDevice, RmClient};

use super::super::types::{CtxBuffer, GpuGen, uvm_cache_line_flush};
use super::NvUvmComputeDevice;

impl NvUvmComputeDevice {
    /// Open via coral-kmod kernel module (kernel-privileged RM client).
    ///
    /// For Blackwell+ GPUs this uses a two-phase initialization:
    ///   Phase 1 (kmod): create RM client, device, subdevice, faulting VA space
    ///   Interstitial (userspace): UVM_REGISTER_GPU + UVM_REGISTER_GPU_VASPACE
    ///   Phase 2 (kmod): create channel group, context share, channel, compute engine
    ///
    /// Pre-Blackwell GPUs complete all setup in a single INIT_COMPUTE call.
    pub(super) fn open_via_kmod(
        kmod: crate::nv::coral_kmod::CoralKmod,
        gpu_index: u32,
        sm: u32,
    ) -> DriverResult<Self> {
        use crate::nv::coral_kmod::kmod_map_rm_memory;

        let gpu_gen = GpuGen::from_sm(sm);
        let mut info = kmod.init_compute(gpu_index, sm)?;
        let ctl_fd = info.ctl_fd;

        tracing::info!(
            h_client = format_args!("0x{:08X}", info.h_client),
            h_vaspace = format_args!("0x{:08X}", info.h_vaspace),
            needs_phase2 = info.needs_phase2,
            ctl_fd,
            "coral-kmod: phase 1 complete"
        );

        // The ctl_fd is a kernel-privileged /dev/nvidiactl installed into
        // our process by the kernel module. We do NOT wrap ctl_fd in a File
        // (which would close it on drop). The kernel module holds a reference
        // to the underlying file via ch->ctl_filp.

        // SAFETY: ctl_fd is a valid kernel-module-opened fd.
        let mut client = unsafe { RmClient::wrap_kmod_fd(ctl_fd, info.h_client) }?;
        let mut uvm = NvUvmDevice::open()?;
        let gpu = NvGpuDevice::open(gpu_index)?;

        match gpu.register_fd(ctl_fd) {
            Ok(()) => {}
            Err(e) => {
                tracing::warn!(
                    "NV_ESC_REGISTER_FD failed: {e} \
                     (kmod ctl_fd may not be cross-context compatible — continuing)"
                );
            }
        }
        uvm.initialize()?;

        match uvm.mm_initialize() {
            Ok(true) => tracing::debug!("UVM_MM_INITIALIZE OK (mm pinned)"),
            Ok(false) => tracing::debug!("UVM_MM_INITIALIZE: platform doesn't need it"),
            Err(e) => tracing::warn!("UVM_MM_INITIALIZE failed: {e} (continuing)"),
        }

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
                    tracing::debug!("UVM_REGISTER_GPU OK");
                }
                Ok(()) => {
                    tracing::warn!(
                        status = format_args!("0x{:08X}", reg.rm_status),
                        "UVM_REGISTER_GPU returned non-OK status (continuing)"
                    );
                }
                Err(e) => {
                    tracing::warn!("UVM_REGISTER_GPU ioctl failed: {e} (continuing)");
                }
            }
        }

        match uvm.register_gpu_vaspace(&info.gpu_uuid, ctl_fd, info.h_client, info.h_vaspace) {
            Ok(()) => tracing::debug!("UVM_REGISTER_GPU_VASPACE OK"),
            Err(e) => {
                tracing::warn!(
                    "UVM_REGISTER_GPU_VASPACE failed: {e} \
                     (continuing — UVM_REGISTER_GPU may have implicitly registered on R580+)"
                );
            }
        }

        match uvm.pageable_mem_access() {
            Ok(supported) => tracing::debug!(supported, "UVM_PAGEABLE_MEM_ACCESS OK"),
            Err(e) => tracing::warn!("UVM_PAGEABLE_MEM_ACCESS failed: {e} (continuing)"),
        }

        let mut uvm_va_next: u64 = 0x1_0000_0000;

        // Early fence memory handles — allocated before phase 2 so they can be
        // UVM-mapped alongside the GPFIFO, before UVM_REGISTER_CHANNEL.
        let mut early_fence_mem_h: u32 = 0;
        let mut early_fence_pb_h: u32 = 0;
        let mut early_fence_va: u64 = 0;
        let mut early_fpb_va: u64 = 0;

        if info.needs_phase2 {
            use crate::nv::uvm::ExternalMapping;
            use super::super::types::page_align;

            eprintln!("[coral-kmod] phase 2: UVM-mapping GPFIFO before COMPLETE_INIT");

            let gpfifo_size = page_align(info.gpfifo_size.max(512 * 8));
            let gpfifo_gpu_va = uvm_va_next;
            uvm.create_external_range(gpfifo_gpu_va, gpfifo_size)?;
            uvm.map_external_allocation(&ExternalMapping {
                base: gpfifo_gpu_va,
                length: gpfifo_size,
                offset: 0,
                rm_ctrl_fd: ctl_fd,
                h_client: info.h_client,
                h_memory: info.h_gpfifo_mem,
                gpu_uuid: &info.gpu_uuid,
            })?;
            uvm_va_next = gpfifo_gpu_va + gpfifo_size;
            eprintln!("[coral-kmod] GPFIFO UVM-mapped at 0x{gpfifo_gpu_va:016X}");

            // Allocate and UVM-map fence memory NOW, before UVM_REGISTER_CHANNEL.
            // This ensures the fence VAs are in the same pre-channel-registration
            // region as the GPFIFO, avoiding any interference from the channel
            // resource range.
            let uses_fence = GpuGen::from_sm(sm).uses_semaphore_fence();
            if uses_fence {
                let h_fm = info.h_device + 0x5005;
                client.alloc_system_memory(info.h_device, h_fm, 4096)?;
                let fv_va = uvm_va_next;
                uvm.create_external_range(fv_va, 4096)?;
                uvm.map_external_allocation(&ExternalMapping {
                    base: fv_va,
                    length: 4096,
                    offset: 0,
                    rm_ctrl_fd: ctl_fd,
                    h_client: info.h_client,
                    h_memory: h_fm,
                    gpu_uuid: &info.gpu_uuid,
                })?;
                uvm_va_next = fv_va + page_align(4096);

                let h_fp = info.h_device + 0x5006;
                client.alloc_system_memory(info.h_device, h_fp, 4096)?;
                let fp_va = uvm_va_next;
                uvm.create_external_range(fp_va, 4096)?;
                uvm.map_external_allocation(&ExternalMapping {
                    base: fp_va,
                    length: 4096,
                    offset: 0,
                    rm_ctrl_fd: ctl_fd,
                    h_client: info.h_client,
                    h_memory: h_fp,
                    gpu_uuid: &info.gpu_uuid,
                })?;
                uvm_va_next = fp_va + page_align(4096);

                early_fence_mem_h = h_fm;
                early_fence_pb_h = h_fp;
                early_fence_va = fv_va;
                early_fpb_va = fp_va;
                info.h_fence_mem = h_fm;
                eprintln!(
                    "[coral-kmod] fence pre-mapped: fence_va=0x{fv_va:016X} fpb_va=0x{fp_va:016X}"
                );
            }

            tracing::debug!("calling CORAL_COMPLETE_INIT (phase 2) with gpfifo_gpu_va=0x{gpfifo_gpu_va:016X}");
            let info2 = kmod.complete_init(info.h_client, gpfifo_gpu_va)?;

            info.h_channel = info2.h_channel;
            info.h_changrp = info2.h_changrp;
            info.h_ctxshare = info2.h_ctxshare;
            info.h_compute = info2.h_compute;
            info.h_virt_mem = info2.h_virt_mem;
            info.h_usermode = info2.h_usermode;
            info.hw_channel_id = info2.hw_channel_id;
            info.work_submit_token = info2.work_submit_token;
            info.channel_class = info2.channel_class;
            info.compute_class = info2.compute_class;
            info.gpfifo_entries = info2.gpfifo_entries;
            info.gpfifo_gpu_va = info2.gpfifo_gpu_va;
            info.userd_size = info2.userd_size;
            info.gpfifo_size = info2.gpfifo_size;
            info.userd_is_vram = info2.userd_is_vram;
            info.ctx_bufs = info2.ctx_bufs;

            eprintln!("[coral-kmod] phase 2 complete: h_channel=0x{:08X} hw_ch={}", info.h_channel, info.hw_channel_id);

            // UVM_REGISTER_CHANNEL — must happen before scheduling
            let chan_resource_range: u64 = 0x1000_0000; // 256 MiB
            let chan_resource_base =
                (uvm_va_next + chan_resource_range - 1) & !(chan_resource_range - 1);
            uvm_va_next = chan_resource_base + chan_resource_range;

            match uvm.register_channel(
                &info.gpu_uuid,
                ctl_fd,
                info.h_client,
                info.h_channel,
                chan_resource_base,
                chan_resource_range,
            ) {
                Ok(()) => eprintln!("[coral-kmod] UVM_REGISTER_CHANNEL OK: base=0x{chan_resource_base:X}"),
                Err(e) => eprintln!("[coral-kmod] UVM_REGISTER_CHANNEL failed: {e} (continuing)"),
            }

            // Schedule the TSG
            client.tsg_gpfifo_schedule(info.h_changrp)?;
            eprintln!("[coral-kmod] tsg_gpfifo_schedule OK");
        }

        // Now map channel memory — all handles and sizes are available.
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

        let uses_semaphore_fence = gpu_gen.uses_semaphore_fence();

        let uses_uvm = info.h_virt_mem == 0;
        let (
            fence_cpu_addr,
            fence_gpu_va,
            fence_mmap_fd,
            fence_pb_cpu_addr,
            fence_pb_gpu_va,
            fence_pb_mmap_fd,
        ) = if uses_semaphore_fence && early_fence_mem_h != 0 {
            // Fence memory was already allocated and UVM-mapped early (before
            // UVM_REGISTER_CHANNEL). Just do the CPU mappings here.
            let fence_fd = open_mmap_fd()?;
            let fence_cpu = kmod_map_rm_memory(
                ctl_fd,
                fence_fd.as_raw_fd(),
                info.h_client,
                info.h_device,
                early_fence_mem_h,
                0,
                4096,
            )?;
            unsafe { VolatilePtr::new(fence_cpu as *mut u32).write(0) };

            let fpb_fd = open_mmap_fd()?;
            let fpb_cpu = kmod_map_rm_memory(
                ctl_fd,
                fpb_fd.as_raw_fd(),
                info.h_client,
                info.h_device,
                early_fence_pb_h,
                0,
                4096,
            )?;

            eprintln!(
                "[coral-kmod] fence CPU-mapped: va=0x{:016X} fpb_va=0x{:016X}",
                early_fence_va, early_fpb_va
            );
            (
                fence_cpu,
                early_fence_va,
                Some(fence_fd),
                fpb_cpu,
                early_fpb_va,
                Some(fpb_fd),
            )
        } else if uses_semaphore_fence && info.h_virt_mem != 0 {
            // Non-faulting VA space: allocate fence memory and DMA-map it.
            let h_fence_mem = if info.h_fence_mem != 0 {
                info.h_fence_mem
            } else {
                let h = info.h_device + 0x5005;
                client.alloc_system_memory(info.h_device, h, 4096)?;
                h
            };

            let fence_fd = open_mmap_fd()?;
            let fence_cpu = kmod_map_rm_memory(
                ctl_fd,
                fence_fd.as_raw_fd(),
                info.h_client,
                info.h_device,
                h_fence_mem,
                0,
                4096,
            )?;
            unsafe { VolatilePtr::new(fence_cpu as *mut u32).write(0) };

            let fence_va = client.rm_map_memory_dma(
                info.h_device,
                info.h_virt_mem,
                h_fence_mem,
                0,
                4096,
            )?;

            let h_fence_pb = info.h_device + 0x5006;
            client.alloc_system_memory(info.h_device, h_fence_pb, 4096)?;
            let fpb_fd = open_mmap_fd()?;
            let fpb_cpu = kmod_map_rm_memory(
                ctl_fd,
                fpb_fd.as_raw_fd(),
                info.h_client,
                info.h_device,
                h_fence_pb,
                0,
                4096,
            )?;
            let fpb_va =
                client.rm_map_memory_dma(info.h_device, info.h_virt_mem, h_fence_pb, 0, 4096)?;

            eprintln!(
                "[coral-kmod] fence (DMA): va=0x{fence_va:016X} fpb_va=0x{fpb_va:016X}"
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

        let mut dev = Self {
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
            compute_subchannel: if uses_semaphore_fence { 1 } else { 0 },
            uses_semaphore_fence,
            fence_cpu_addr,
            fence_gpu_va,
            fence_value: 0,
            fence_mmap_fd,
            fence_pb_cpu_addr,
            fence_pb_gpu_va,
            fence_pb_mmap_fd,
            coral_kmod: Some(kmod),
            kmod_h_client: info.h_client,
            uses_uvm_mapping: uses_uvm,
            uvm_va_next,
            caps: crate::nv::generation::profile_for_sm(sm).to_capabilities(),
        };

        // Zero the error notifier so we can distinguish stale from fresh errors.
        if errnotif_cpu_addr != 0 {
            unsafe {
                std::ptr::write_bytes(errnotif_cpu_addr as *mut u8, 0, 48);
                uvm_cache_line_flush(errnotif_cpu_addr as *const u8);
            }
            eprintln!("[coral-kmod] errnotif zeroed at 0x{errnotif_cpu_addr:X}");
        }

        // Test bare fence release before compute_init to validate GPFIFO + fence mechanism.
        if uses_semaphore_fence && dev.fence_pb_cpu_addr != 0 {
            eprintln!("[coral-kmod] testing bare fence release...");
            dev.submit_fence_release()?;
            match dev.poll_gpfifo_completion() {
                Ok(()) => eprintln!("[coral-kmod] bare fence release OK — GPFIFO works!"),
                Err(e) => {
                    let en = dev.read_error_notifier();
                    eprintln!("[coral-kmod] bare fence release FAILED: {e}  errnotif=[{en}]");
                }
            }
        }

        // Compute init: SET_OBJECT + SLM configuration.
        // The kernel-privileged bind from COMPLETE_INIT should have
        // registered the compute class in the PBDMA subchannel table.
        if uses_uvm {
            use crate::nv::pushbuf::PushBuf;

            let compute_class = gpu_gen.compute_class();
            let slm_size: u64 = 0x20000;
            let h_slm = info.h_device + 0x5FFD;
            dev.client
                .alloc_system_memory(info.h_device, h_slm, slm_size)?;
            let slm_gpu_va = dev.gpu_map_buffer(h_slm, slm_size)?;
            let slm_per_tpc: u64 = 0x8000;

            let init_pb = PushBuf::compute_init(
                compute_class,
                0xFF00_0000,
                slm_gpu_va,
                slm_per_tpc,
            );
            let init_bytes = init_pb.as_bytes();
            let init_dwords = init_pb.as_words().len() as u32;

            let h_init = info.h_device + 0x5FFE;
            dev.client
                .alloc_system_memory(info.h_device, h_init, 4096)?;
            let init_gpu_va = dev.gpu_map_buffer_infra(h_init, 4096)?;
            let init_fd = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/nvidiactl")
                .map_err(|e| {
                    DriverError::DeviceNotFound(format!("nvidiactl: {e}").into())
                })?;
            let init_cpu = dev.client.rm_map_memory_on_fd(
                init_fd.as_raw_fd(),
                info.h_device,
                h_init,
                0,
                4096,
            )?;

            unsafe {
                std::ptr::copy_nonoverlapping(
                    init_bytes.as_ptr(),
                    init_cpu as *mut u8,
                    init_bytes.len(),
                );
            }

            eprintln!(
                "[coral-kmod] compute_init: SET_OBJECT class=0x{compute_class:04X}"
            );
            dev.submit_gpfifo(init_gpu_va, init_dwords)?;
            if dev.uses_semaphore_fence {
                dev.submit_fence_release()?;
            }
            match dev.poll_gpfifo_completion() {
                Ok(()) => {
                    eprintln!(
                        "[coral-kmod] compute_init OK — SET_OBJECT accepted!"
                    );
                }
                Err(e) => {
                    eprintln!("[coral-kmod] compute_init FAILED: {e}");
                }
            }

            dev.client
                .rm_unmap_memory(info.h_device, h_init, init_cpu)
                .ok();
            dev.client.free_object(info.h_device, h_init).ok();
        }

        tracing::info!(
            gpu_index,
            sm,
            "NvUvmComputeDevice initialized via coral-kmod (kernel-privileged)"
        );

        Ok(dev)
    }
}
