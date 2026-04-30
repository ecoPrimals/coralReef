// SPDX-License-Identifier: AGPL-3.0-or-later
//! Coral-kmod initialization path for [`NvUvmComputeDevice`](super::NvUvmComputeDevice).

use std::collections::HashMap;
use std::os::fd::AsRawFd;

use crate::error::{DriverError, DriverResult};
use crate::mmio::VolatilePtr;
use crate::nv::uvm::{NvGpuDevice, NvUvmDevice, RmClient};

use super::super::types::{CtxBuffer, GpuGen};
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

        if info.needs_phase2 {
            tracing::debug!("calling CORAL_COMPLETE_INIT (phase 2)...");
            let info2 = kmod.complete_init(info.h_client)?;

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
            info.h_fence_mem = info2.h_fence_mem;
            info.ctx_bufs = info2.ctx_bufs;

            tracing::debug!(
                h_channel = format_args!("0x{:08X}", info.h_channel),
                hw_channel_id = info.hw_channel_id,
                "phase 2 complete"
            );
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

        let (
            fence_cpu_addr,
            fence_gpu_va,
            fence_mmap_fd,
            fence_pb_cpu_addr,
            fence_pb_gpu_va,
            fence_pb_mmap_fd,
        ) = if uses_semaphore_fence && info.h_fence_mem != 0 {
            let fence_fd = open_mmap_fd()?;
            let fence_cpu = kmod_map_rm_memory(
                ctl_fd,
                fence_fd.as_raw_fd(),
                info.h_client,
                info.h_device,
                info.h_fence_mem,
                0,
                4096,
            )?;
            // SAFETY: `kmod_map_rm_memory` returned a CPU pointer to the mapped fence page;
            // the first u32 is initialized to the idle fence value before GPU use.
            unsafe { VolatilePtr::new(fence_cpu as *mut u32).write(0) };

            let fence_va = client.rm_map_memory_dma(
                info.h_device,
                info.h_virt_mem,
                info.h_fence_mem,
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

            tracing::info!(
                fence_va = format_args!("0x{fence_va:016X}"),
                fpb_va = format_args!("0x{fpb_va:016X}"),
                "kmod: Blackwell semaphore fence wired"
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
            fence_cpu_addr,
            fence_gpu_va,
            fence_value: 0,
            fence_mmap_fd,
            fence_pb_cpu_addr,
            fence_pb_gpu_va,
            fence_pb_mmap_fd,
            coral_kmod: Some(kmod),
            kmod_h_client: info.h_client,
            uses_uvm_mapping: false,
            uvm_va_next: 0x1_0000_0000,
            caps: crate::nv::generation::profile_for_sm(sm).to_capabilities(),
        };

        tracing::info!(
            gpu_index,
            sm,
            "NvUvmComputeDevice initialized via coral-kmod (kernel-privileged)"
        );

        Ok(dev)
    }
}
