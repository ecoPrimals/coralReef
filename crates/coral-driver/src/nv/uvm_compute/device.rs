// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`NvUvmComputeDevice`] construction, RM channel setup, and GPFIFO submission.

use std::collections::HashMap;
use std::os::fd::AsRawFd;

use crate::error::{DriverError, DriverResult};
use crate::mmio::VolatilePtr;
use crate::nv::uvm::{NvGpuDevice, NvUvmDevice, RmClient, VOLTA_USERMODE_A};

use super::types::{
    GPFIFO_ENTRIES, GPFIFO_SIZE, GpuGen, USERD_GP_GET_OFFSET, USERD_GP_PUT_OFFSET, USERD_SIZE,
    UvmBuffer, gpfifo_entry, uvm_cache_line_flush,
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
    pub(super) next_handle: u32,
    pub(super) next_mem_handle: u32,
    /// Inflight temporary buffers that survive until `sync()`.
    pub(super) inflight: Vec<crate::BufferHandle>,
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
    /// CPU-mapped pointer to the USERMODE doorbell register page.
    pub(super) doorbell_addr: u64,
    /// Work submit token returned by RM (written to doorbell to notify GPU).
    pub(super) work_submit_token: u32,
}

impl NvUvmComputeDevice {
    /// Open a UVM compute device for the specified GPU index and SM version.
    ///
    /// Performs the full RM object chain:
    /// ROOT → DEVICE → SUBDEVICE → UUID query → `UVM_REGISTER_GPU` →
    /// `VA_SPACE` → `CHANNEL_GROUP` → GPFIFO → COMPUTE
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if any step in the initialization chain fails.
    pub fn open(gpu_index: u32, sm: u32) -> DriverResult<Self> {
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

        tracing::info!(
            gpu_index,
            sm,
            h_device = format_args!("0x{h_device:08X}"),
            h_channel = format_args!("0x{h_channel:08X}"),
            h_compute = format_args!("0x{h_compute:08X}"),
            work_submit_token = format_args!("0x{work_submit_token:08X}"),
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
            next_handle: 1,
            next_mem_handle: h_device + 0x6000,
            inflight: Vec::new(),
            userd_cpu_addr,
            gpfifo_cpu_addr,
            gp_put: 0,
            h_virt_mem,
            userd_mmap_fd,
            gpfifo_mmap_fd,
            usermode_mmap_fd,
            doorbell_addr,
            work_submit_token,
        };

        // NOP smoke test: submit a single NOP to verify GPFIFO is working.
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
        // SAFETY: `nop_cpu` is a valid user-space mapping of `nop_h_mem` (4096 bytes, page-aligned)
        // returned by `rm_map_memory_on_fd`. Writing a single u32 NOP command is within bounds.
        unsafe { VolatilePtr::new(nop_cpu as *mut u32).write(0) };
        dev.submit_gpfifo(nop_gpu_va, 1)?;
        dev.poll_gpfifo_completion()?;
        tracing::info!("NOP smoke test passed — GPFIFO pipeline operational");

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

    /// Poll for GPFIFO completion by checking `GP_GET` in the USERD page.
    pub(super) fn poll_gpfifo_completion(&self) -> DriverResult<()> {
        if self.userd_cpu_addr == 0 || self.gp_put == 0 {
            return Ok(());
        }

        let gp_get_ptr = (self.userd_cpu_addr + USERD_GP_GET_OFFSET as u64) as *mut u32;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            // SAFETY: userd_cpu_addr is a valid kernel mmap'd address; GP_GET lies
            // within the USERD page; volatile read matches GPU DMA updates.
            let gp_get = unsafe { VolatilePtr::new(gp_get_ptr).read() };
            if gp_get >= self.gp_put {
                return Ok(());
            }
            if std::time::Instant::now() > deadline {
                return Err(DriverError::SubmitFailed(
                    format!(
                        "GPFIFO completion timeout: GP_GET={gp_get} GP_PUT={}",
                        self.gp_put
                    )
                    .into(),
                ));
            }
            std::hint::spin_loop();
            std::thread::sleep(std::time::Duration::from_micros(10));
        }
    }

    /// Map an RM buffer into the GPU VA space via DMA.
    pub(super) fn gpu_map_buffer(&mut self, h_mem: u32, size: u64) -> DriverResult<u64> {
        self.client
            .rm_map_memory_dma(self.h_device, self.h_virt_mem, h_mem, 0, size)
    }
}
