// SPDX-License-Identifier: AGPL-3.0-or-later
//! Userspace GPFIFO channel setup: FIFO mappings, compute bind, TSG, doorbell, fence.

use std::os::fd::AsRawFd;

use crate::error::{DriverError, DriverResult};
use crate::mmio::VolatilePtr;
use crate::nv::uvm::{RmClient, VOLTA_USERMODE_A};

use super::device::NvUvmComputeDevice;
use super::types::{GPFIFO_ENTRIES, GPFIFO_SIZE, GpuGen, USERD_SIZE};

use super::ctx_buffers::uses_semaphore_fence_for_gen;

fn open_nvidiactl_mmap_fd() -> DriverResult<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/nvidiactl")
        .map_err(|e| DriverError::DeviceNotFound(format!("nvidiactl for mmap: {e}").into()))
}

/// RM objects and CPU mappings for the GPFIFO channel through compute engine bind.
pub(super) struct UserspaceGpfifoChannelState {
    pub h_changrp: u32,
    pub h_channel: u32,
    pub hw_channel_id: u32,
    pub userd_mmap_fd: std::fs::File,
    pub gpfifo_mmap_fd: std::fs::File,
    pub errnotif_mmap_fd: std::fs::File,
    pub userd_cpu_addr: u64,
    pub gpfifo_cpu_addr: u64,
    pub errnotif_cpu_addr: u64,
    pub h_compute: u32,
}

/// Allocate channel group, GPFIFO channel, CPU-map rings, and bind the compute engine.
#[expect(
    clippy::too_many_arguments,
    reason = "RM userspace GPFIFO setup takes many correlated object handles"
)]
pub(super) fn userspace_setup_gpfifo_channel(
    client: &mut RmClient,
    gpu_index: u32,
    gpu_gen: GpuGen,
    h_device: u32,
    h_subdevice: u32,
    h_vaspace: u32,
    h_userd_mem: u32,
    h_gpfifo_mem: u32,
    h_virt_mem: u32,
    h_errnotif_mem: u32,
    userd_in_vram: bool,
) -> DriverResult<UserspaceGpfifoChannelState> {
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
        open_nvidiactl_mmap_fd()?
    };
    let userd_cpu_addr = client.rm_map_memory_on_fd(
        userd_mmap_fd.as_raw_fd(),
        h_device,
        h_userd_mem,
        0,
        USERD_SIZE,
    )?;
    let gpfifo_mmap_fd = open_nvidiactl_mmap_fd()?;
    let gpfifo_cpu_addr = client.rm_map_memory_on_fd(
        gpfifo_mmap_fd.as_raw_fd(),
        h_device,
        h_gpfifo_mem,
        0,
        GPFIFO_SIZE,
    )?;

    // CPU-map the error notifier so we can read GPU error codes on timeout.
    let errnotif_mmap_fd = open_nvidiactl_mmap_fd()?;
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

    Ok(UserspaceGpfifoChannelState {
        h_changrp,
        h_channel,
        hw_channel_id,
        userd_mmap_fd,
        gpfifo_mmap_fd,
        errnotif_mmap_fd,
        userd_cpu_addr,
        gpfifo_cpu_addr,
        errnotif_cpu_addr,
        h_compute,
    })
}

/// TSG schedule, work submit token, USERMODE doorbell, and Blackwell semaphore fence buffers.
pub(super) struct UserspaceDoorbellFenceState {
    pub work_submit_token: u32,
    pub usermode_mmap_fd: std::fs::File,
    pub doorbell_addr: u64,
    pub uses_semaphore_fence: bool,
    pub fence_cpu_addr: u64,
    pub fence_gpu_va: u64,
    pub fence_mmap_fd: Option<std::fs::File>,
    pub fence_pb_cpu_addr: u64,
    pub fence_pb_gpu_va: u64,
    pub fence_pb_mmap_fd: Option<std::fs::File>,
}

#[expect(
    clippy::too_many_arguments,
    reason = "TSG schedule, doorbell, and fence setup share RM client state"
)]
pub(super) fn userspace_schedule_tsg_doorbell_and_fence(
    client: &mut RmClient,
    gpu_index: u32,
    gpu_gen: GpuGen,
    h_device: u32,
    h_subdevice: u32,
    h_changrp: u32,
    h_channel: u32,
    hw_channel_id: u32,
    h_virt_mem: u32,
) -> DriverResult<UserspaceDoorbellFenceState> {
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
    let doorbell_addr =
        client.rm_map_memory_on_fd(usermode_mmap_fd.as_raw_fd(), h_device, h_usermode, 0, 4096)?;

    // Blackwell (clca6f) removed GP_GET from the USERD control struct —
    // the GPU no longer writes GP_GET to USERD. We must use a semaphore
    // release written by the GPU into a separate fence buffer.
    let uses_semaphore_fence = uses_semaphore_fence_for_gen(gpu_gen);

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
        let fence_va = client.rm_map_memory_dma(h_device, h_virt_mem, h_fence_mem, 0, 4096)?;
        let fence_fd = open_nvidiactl_mmap_fd()?;
        let fence_cpu =
            client.rm_map_memory_on_fd(fence_fd.as_raw_fd(), h_device, h_fence_mem, 0, 4096)?;
        unsafe { VolatilePtr::new(fence_cpu as *mut u32).write(0) };

        // Fence push buffer: rewritten before each fence submission.
        let h_fence_pb = h_device + 0x5006;
        client.alloc_system_memory(h_device, h_fence_pb, 4096)?;
        let fpb_va = client.rm_map_memory_dma(h_device, h_virt_mem, h_fence_pb, 0, 4096)?;
        let fpb_fd = open_nvidiactl_mmap_fd()?;
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

    Ok(UserspaceDoorbellFenceState {
        work_submit_token,
        usermode_mmap_fd,
        doorbell_addr,
        uses_semaphore_fence,
        fence_cpu_addr,
        fence_gpu_va,
        fence_mmap_fd,
        fence_pb_cpu_addr,
        fence_pb_gpu_va,
        fence_pb_mmap_fd,
    })
}

/// NOP GPFIFO smoke test, SLM allocation, and one-time compute init push buffer.
pub(super) fn userspace_nop_smoke_slq_compute_init(
    dev: &mut NvUvmComputeDevice,
    h_device: u32,
    h_virt_mem: u32,
    compute_class: u32,
) -> DriverResult<()> {
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

        let init_pb = PushBuf::compute_init(compute_class, 0xFF00_0000, slm_gpu_va, slm_per_tpc);
        let init_bytes = init_pb.as_bytes();
        let init_len = u32::try_from(init_pb.as_words().len())
            .map_err(|_| DriverError::platform_overflow("init pb dwords fits u32"))?;

        let h_init_mem = h_device + 0x5FFE;
        dev.client.alloc_system_memory(h_device, h_init_mem, 4096)?;
        let init_gpu_va = dev
            .client
            .rm_map_memory_dma(h_device, h_virt_mem, h_init_mem, 0, 4096)?;
        let init_fd = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/nvidiactl")
            .map_err(|e| DriverError::DeviceNotFound(format!("nvidiactl: {e}").into()))?;
        let init_cpu =
            dev.client
                .rm_map_memory_on_fd(init_fd.as_raw_fd(), h_device, h_init_mem, 0, 4096)?;

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

    Ok(())
}
