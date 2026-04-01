// SPDX-License-Identifier: AGPL-3.0-only
//! Channel allocation: VA space, GPFIFO ring, USERD, compute engine bind, doorbell.

use std::os::fd::AsRawFd;

use crate::error::{DriverError, DriverResult};

use super::uvm::VOLTA_USERMODE_A;
use super::uvm_rm_setup::UvmRmInit;

/// Default GPFIFO ring entries (each entry = 8 bytes, 512 entries = 4 KiB).
pub(in crate::nv) const GPFIFO_ENTRIES: u32 = 512;

/// Default GPFIFO ring size in bytes.
pub(in crate::nv) const GPFIFO_SIZE: u64 = GPFIFO_ENTRIES as u64 * 8;

/// USERD page size.
pub(in crate::nv) const USERD_SIZE: u64 = 4096;

/// Volta+ RAMUSERD `GP_PUT` offset (bytes) — dword 35.
pub(in crate::nv) const USERD_GP_PUT_OFFSET: usize = 35 * 4; // 0x8C

/// Volta+ RAMUSERD `GP_GET` offset (bytes) — dword 34.
pub(in crate::nv) const USERD_GP_GET_OFFSET: usize = 34 * 4; // 0x88

/// GPFIFO entry in the ring buffer (8 bytes).
///
/// Layout (NVA06F+ Kepler/Volta/Ampere):
/// ```text
/// DWORD 0 [31:2]  = push buffer GPU VA [31:2]
/// DWORD 0 [1:0]   = 0 (unconditional fetch)
/// DWORD 1 [8:0]   = push buffer GPU VA [40:32]
/// DWORD 1 [9]     = privilege level (0 = user)
/// DWORD 1 [30:10] = length in dwords
/// DWORD 1 [31]    = 0 (not a SYNC entry)
/// ```
///
/// The address is NOT shifted — it goes directly into the entry with bits
/// `[1:0]` = 0 (4-byte alignment is required).
pub(in crate::nv) const fn gpfifo_entry(push_buf_va: u64, length_dwords: u32) -> u64 {
    (push_buf_va & !3) | ((length_dwords as u64) << 42)
}

/// RM objects and CPU mappings for the GPFIFO / compute channel.
pub(in crate::nv) struct UvmChannelState {
    pub(in crate::nv) h_vaspace: u32,
    pub(in crate::nv) h_changrp: u32,
    pub(in crate::nv) h_channel: u32,
    pub(in crate::nv) h_compute: u32,
    pub(in crate::nv) h_virt_mem: u32,
    pub(in crate::nv) userd_cpu_addr: u64,
    pub(in crate::nv) gpfifo_cpu_addr: u64,
    pub(in crate::nv) gp_put: u32,
    pub(in crate::nv) userd_mmap_fd: std::fs::File,
    pub(in crate::nv) gpfifo_mmap_fd: std::fs::File,
    pub(in crate::nv) usermode_mmap_fd: std::fs::File,
    pub(in crate::nv) doorbell_addr: u64,
    pub(in crate::nv) work_submit_token: u32,
}

/// Allocate VA space, channel group, USERD/GPFIFO memory, GPFIFO channel, compute engine, and doorbell.
pub(in crate::nv) fn allocate_uvm_channel(
    init: &mut UvmRmInit,
    gpu_index: u32,
) -> DriverResult<UvmChannelState> {
    let client = &mut init.client;
    let gpu_gen = init.gpu_gen;
    let h_device = init.h_device;
    let h_subdevice = init.h_subdevice;

    let h_userd_mem = h_device + 0x5000;
    let h_gpfifo_mem = h_device + 0x5001;
    let h_virt_mem = h_device + 0x5002;

    // CUDA allocates USERD in VRAM (NV01_MEMORY_LOCAL_USER) with 2 MiB
    // size/alignment. Try that first; fall back to system memory if needed.
    let userd_vram_size: u64 = 0x20_0000; // 2 MiB like CUDA
    let userd_in_vram = match client.alloc_local_memory(h_device, h_userd_mem, userd_vram_size) {
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

    let h_vaspace = client.alloc_vaspace(h_device)?;
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

    let compute_class = gpu_gen.compute_class();
    let h_compute = client.alloc_compute_engine(h_channel, compute_class)?;

    // BIND the compute engine to the channel — without this the GPU
    // cannot route push buffer methods to the compute engine.
    client.channel_bind_engine(h_channel, h_compute, compute_class, 1)?;
    tracing::info!(
        h_compute = format_args!("0x{h_compute:08X}"),
        "Compute engine bound to channel"
    );

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

    Ok(UvmChannelState {
        h_vaspace,
        h_changrp,
        h_channel,
        h_compute,
        h_virt_mem,
        userd_cpu_addr,
        gpfifo_cpu_addr,
        gp_put: 0,
        userd_mmap_fd,
        gpfifo_mmap_fd,
        usermode_mmap_fd,
        doorbell_addr,
        work_submit_token,
    })
}
