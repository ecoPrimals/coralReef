// SPDX-License-Identifier: AGPL-3.0-only
//! UVM-based compute device — dispatches via the NVIDIA proprietary driver.
//!
//! Bypasses nouveau entirely, using the RM object hierarchy through
//! `/dev/nvidiactl` and UVM through `/dev/nvidia-uvm` for memory management.
//! Reuses the identical QMD and push buffer formats from the nouveau path.

use std::collections::HashMap;
use std::os::fd::AsRawFd;

use crate::error::{DriverError, DriverResult};
use crate::mmio::VolatilePtr;
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use super::qmd;
use super::uvm::{
    ADA_COMPUTE_A, AMPERE_CHANNEL_GPFIFO_A, AMPERE_COMPUTE_A, AMPERE_COMPUTE_B,
    BLACKWELL_CHANNEL_GPFIFO_B, BLACKWELL_COMPUTE_A, BLACKWELL_COMPUTE_B, HOPPER_COMPUTE_A,
    NvGpuDevice, NvUvmDevice, RmClient, VOLTA_CHANNEL_GPFIFO_A, VOLTA_COMPUTE_A,
};

/// Flush one cache line so GPU DMA sees CPU writes (UVM mmap paths; mirrors `vfio::cache_ops`).
#[cfg(target_arch = "x86_64")]
#[inline]
unsafe fn uvm_cache_line_flush(addr: *const u8) {
    unsafe { core::arch::x86_64::_mm_clflush(addr) }
}

#[cfg(not(target_arch = "x86_64"))]
#[inline]
unsafe fn uvm_cache_line_flush(_addr: *const u8) {}

/// GPU generation derived from SM version, used for class selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GpuGen {
    Volta,
    Turing,
    /// GA100 (A100, SM 8.0) — uses `AMPERE_COMPUTE_A`.
    AmpereA,
    /// `GA10x` (RTX 30xx, SM 8.6+) — uses `AMPERE_COMPUTE_B`.
    AmpereB,
    /// AD10x (RTX 40xx, SM 8.9) — uses `ADA_COMPUTE_A`.
    Ada,
    /// GH100 (H100, SM 9.0) — uses `HOPPER_COMPUTE_A`.
    Hopper,
    /// GB100/200 (B200, SM 10.0) — data center Blackwell, `BLACKWELL_COMPUTE_A`.
    BlackwellA,
    /// GB20x (RTX 50xx, SM 12.0) — consumer Blackwell, `BLACKWELL_COMPUTE_B`.
    BlackwellB,
}

impl GpuGen {
    const fn from_sm(sm: u32) -> Self {
        match sm {
            75 => Self::Turing,
            80 => Self::AmpereA,
            81..=88 => Self::AmpereB,
            89 => Self::Ada,
            90 => Self::Hopper,
            100 => Self::BlackwellA,
            120.. => Self::BlackwellB,
            _ => Self::Volta,
        }
    }

    const fn channel_class(self) -> u32 {
        match self {
            Self::BlackwellA | Self::BlackwellB => BLACKWELL_CHANNEL_GPFIFO_B,
            Self::AmpereA | Self::AmpereB | Self::Ada | Self::Hopper => AMPERE_CHANNEL_GPFIFO_A,
            Self::Volta | Self::Turing => VOLTA_CHANNEL_GPFIFO_A,
        }
    }

    const fn compute_class(self) -> u32 {
        match self {
            Self::BlackwellA => BLACKWELL_COMPUTE_A,
            Self::BlackwellB => BLACKWELL_COMPUTE_B,
            Self::Hopper => HOPPER_COMPUTE_A,
            Self::Ada => ADA_COMPUTE_A,
            Self::AmpereA => AMPERE_COMPUTE_A,
            Self::AmpereB => AMPERE_COMPUTE_B,
            Self::Volta | Self::Turing => VOLTA_COMPUTE_A,
        }
    }
}

/// A buffer allocated via RM + UVM.
struct UvmBuffer {
    h_memory: u32,
    size: u64,
    gpu_va: u64,
    /// CPU linear address from `NV_ESC_RM_MAP_MEMORY` (0 = not mapped).
    cpu_addr: u64,
    /// Dedicated nvidiactl fd that holds this buffer's mmap context. On
    /// Blackwell (580.x), each nvidiactl fd supports only one active
    /// mmap context, so each buffer needs its own fd.
    #[expect(dead_code, reason = "kept alive for mmap lifetime")]
    mmap_fd: Option<std::fs::File>,
}

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
const fn gpfifo_entry(push_buf_va: u64, length_dwords: u32) -> u64 {
    (push_buf_va & !3) | ((length_dwords as u64) << 42)
}

/// Volta+ RAMUSERD `GP_PUT` offset (bytes) — dword 35.
const USERD_GP_PUT_OFFSET: usize = 35 * 4; // 0x8C

/// Volta+ RAMUSERD `GP_GET` offset (bytes) — dword 34.
const USERD_GP_GET_OFFSET: usize = 34 * 4; // 0x88

/// Compute device backed by the NVIDIA proprietary driver (RM + UVM).
///
/// Implements the full dispatch pipeline: RM object allocation, UVM memory
/// mapping, QMD construction (via reused `qmd.rs`), and GPFIFO submission.
pub struct NvUvmComputeDevice {
    client: RmClient,
    #[expect(
        dead_code,
        reason = "held for lifetime — UVM fd needed for GPU VA operations"
    )]
    uvm: NvUvmDevice,
    #[expect(
        dead_code,
        reason = "held for lifetime — GPU fd needed for mmap and RM operations"
    )]
    gpu: NvGpuDevice,
    gpu_gen: GpuGen,
    h_device: u32,
    #[expect(dead_code, reason = "held for RM_CONTROL calls (e.g. perf queries)")]
    h_subdevice: u32,
    #[expect(
        dead_code,
        reason = "held for VA space teardown and future sub-allocations"
    )]
    h_vaspace: u32,
    #[expect(dead_code, reason = "held for channel group teardown")]
    h_changrp: u32,
    #[expect(
        dead_code,
        reason = "held for channel teardown / GPFIFO ring ownership"
    )]
    h_channel: u32,
    h_compute: u32,
    #[expect(dead_code, reason = "needed for UVM_MAP_EXTERNAL_ALLOCATION")]
    gpu_uuid: [u8; 16],
    buffers: HashMap<u32, UvmBuffer>,
    next_handle: u32,
    next_mem_handle: u32,
    /// Inflight temporary buffers that survive until `sync()`.
    inflight: Vec<BufferHandle>,
    /// CPU-mapped pointer to the USERD page (for `GP_PUT` doorbell writes).
    userd_cpu_addr: u64,
    /// CPU-mapped pointer to the GPFIFO ring (for writing GPFIFO entries).
    gpfifo_cpu_addr: u64,
    /// Current `GP_PUT` index (next slot to write in the GPFIFO ring).
    gp_put: u32,
    /// Handle of the `NV01_MEMORY_VIRTUAL` for DMA mapping.
    h_virt_mem: u32,
    #[expect(dead_code, reason = "kept alive for USERD mmap lifetime")]
    userd_mmap_fd: std::fs::File,
    #[expect(dead_code, reason = "kept alive for GPFIFO mmap lifetime")]
    gpfifo_mmap_fd: std::fs::File,
    #[expect(dead_code, reason = "kept alive for USERMODE doorbell mmap lifetime")]
    usermode_mmap_fd: std::fs::File,
    /// CPU-mapped pointer to the USERMODE doorbell register page.
    doorbell_addr: u64,
    /// Work submit token returned by RM (written to doorbell to notify GPU).
    work_submit_token: u32,
}

/// Default GPFIFO ring entries (each entry = 8 bytes, 512 entries = 4 KiB).
const GPFIFO_ENTRIES: u32 = 512;

/// Default GPFIFO ring size in bytes.
const GPFIFO_SIZE: u64 = GPFIFO_ENTRIES as u64 * 8;

/// USERD page size.
const USERD_SIZE: u64 = 4096;

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
            super::uvm::VOLTA_USERMODE_A,
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
    fn alloc_handle(&mut self) -> u32 {
        let h = self.next_handle;
        self.next_handle += 1;
        h
    }

    #[expect(
        clippy::missing_const_for_fn,
        reason = "mutates self for handle allocation; not const-compatible"
    )]
    fn alloc_mem_handle(&mut self) -> u32 {
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
    fn submit_gpfifo(&mut self, push_buf_va: u64, length_dwords: u32) -> DriverResult<()> {
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
    fn poll_gpfifo_completion(&self) -> DriverResult<()> {
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
    fn gpu_map_buffer(&mut self, h_mem: u32, size: u64) -> DriverResult<u64> {
        self.client
            .rm_map_memory_dma(self.h_device, self.h_virt_mem, h_mem, 0, size)
    }
}

/// Page-align a size upward (4 KiB pages).
const fn page_align(size: u64) -> u64 {
    (size + 0xFFF) & !0xFFF
}

/// Reinterpret a `&[u32]` as `&[u8]` for buffer upload.
fn u32_slice_as_bytes(words: &[u32]) -> &[u8] {
    bytemuck::cast_slice(words)
}

impl ComputeDevice for NvUvmComputeDevice {
    fn alloc(&mut self, size: u64, _domain: MemoryDomain) -> DriverResult<BufferHandle> {
        let aligned = page_align(size);
        let h_mem = self.alloc_mem_handle();

        self.client
            .alloc_system_memory(self.h_device, h_mem, aligned)?;

        let gpu_va = self.gpu_map_buffer(h_mem, aligned)?;

        // On Blackwell (580.x), each nvidiactl fd supports only one mmap
        // context. Open a dedicated fd per buffer for the CPU mapping.
        let mmap_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/nvidiactl")
            .map_err(|e| DriverError::DeviceNotFound(format!("nvidiactl for mmap: {e}").into()))?;
        let cpu_addr = self.client.rm_map_memory_on_fd(
            mmap_file.as_raw_fd(),
            self.h_device,
            h_mem,
            0,
            aligned,
        )?;

        let handle_id = self.alloc_handle();
        self.buffers.insert(
            handle_id,
            UvmBuffer {
                h_memory: h_mem,
                size: aligned,
                gpu_va,
                cpu_addr,
                mmap_fd: Some(mmap_file),
            },
        );
        Ok(BufferHandle(handle_id))
    }

    fn free(&mut self, handle: BufferHandle) -> DriverResult<()> {
        let buf = self
            .buffers
            .remove(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        if buf.cpu_addr != 0 {
            let _ = self
                .client
                .rm_unmap_memory(self.h_device, buf.h_memory, buf.cpu_addr);
        }
        self.client.free_object(self.h_device, buf.h_memory)?;
        Ok(())
    }

    fn upload(&mut self, handle: BufferHandle, offset: u64, data: &[u8]) -> DriverResult<()> {
        let buf = self
            .buffers
            .get(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;

        if offset + data.len() as u64 > buf.size {
            return Err(DriverError::MmapFailed(
                format!(
                    "UVM write out of bounds: offset={offset}, len={}, size={}",
                    data.len(),
                    buf.size
                )
                .into(),
            ));
        }

        if buf.cpu_addr == 0 {
            return Err(DriverError::MmapFailed("buffer has no CPU mapping".into()));
        }

        // SAFETY: cpu_addr from rm_map_memory is a valid user-space address
        // returned by the kernel's vm_mmap; valid for buf.size bytes. Bounds
        // check above ensures offset + data.len() <= buf.size. Pointer is
        // non-null (cpu_addr != 0 checked) and properly aligned for u8.
        let dst_slice = unsafe {
            std::slice::from_raw_parts_mut((buf.cpu_addr + offset) as *mut u8, data.len())
        };
        dst_slice.copy_from_slice(data);
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
                    "UVM read out of bounds: offset={offset}, len={len}, size={}",
                    buf.size
                )
                .into(),
            ));
        }

        if buf.cpu_addr == 0 {
            return Err(DriverError::MmapFailed("buffer has no CPU mapping".into()));
        }

        // SAFETY: cpu_addr from rm_map_memory is a valid kernel vm_mmap address;
        // valid for buf.size bytes. Bounds check ensures offset + len <= buf.size.
        // Pointer is non-null (cpu_addr != 0 checked) and properly aligned for u8.
        let src_slice =
            unsafe { std::slice::from_raw_parts((buf.cpu_addr + offset) as *const u8, len) };
        Ok(src_slice.to_vec())
    }

    fn dispatch(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
    ) -> DriverResult<()> {
        use super::pushbuf::PushBuf;

        let shader_size = u64::try_from(shader.len())
            .map_err(|_| DriverError::platform_overflow("shader size fits in u64"))?;
        let shader_handle = self.alloc(shader_size, MemoryDomain::Gtt)?;
        self.upload(shader_handle, 0, shader)?;

        let shader_va = self.buffers.get(&shader_handle.0).map_or(0, |b| b.gpu_va);

        // Build CBUF descriptor buffer (matches nouveau dispatch model).
        // The compiler generates `c[0][binding * 8]` to load buffer addresses,
        // so CBUF 0 must contain the descriptor table, not raw buffer data.
        let desc_entry_size = 16_u64;
        let desc_buf_size = desc_entry_size
            * u64::try_from(buffers.len().max(1))
                .map_err(|_| DriverError::platform_overflow("buffer count fits in u64"))?;
        let desc_handle = self.alloc(desc_buf_size, MemoryDomain::Gtt)?;

        let mut desc_data = vec![
            0u8;
            usize::try_from(desc_buf_size).map_err(|_| {
                DriverError::platform_overflow("desc_buf_size fits in usize")
            })?
        ];
        for (i, bh) in buffers.iter().enumerate() {
            if let Some(buf) = self.buffers.get(&bh.0) {
                let off = i * 8;
                let va = buf.gpu_va;
                let sz = u32::try_from(buf.size).unwrap_or(u32::MAX);
                let va_lo = (va & 0xFFFF_FFFF) as u32;
                let va_hi = (va >> 32) as u32;
                desc_data[off..off + 4].copy_from_slice(&va_lo.to_le_bytes());
                desc_data[off + 4..off + 8].copy_from_slice(&va_hi.to_le_bytes());
                let sz_off = off + 8;
                if sz_off + 4 <= desc_data.len() {
                    desc_data[sz_off..sz_off + 4].copy_from_slice(&sz.to_le_bytes());
                }
            }
        }
        self.upload(desc_handle, 0, &desc_data)?;
        let desc_va = self.buffers.get(&desc_handle.0).map_or(0, |b| b.gpu_va);

        let cbufs = vec![qmd::CbufBinding {
            index: 0,
            addr: desc_va,
            size: u32::try_from(desc_buf_size).unwrap_or(u32::MAX),
        }];

        let qmd_params = qmd::QmdParams {
            shader_va,
            grid: dims,
            workgroup: info.workgroup,
            gpr_count: info.gpr_count.max(4),
            shared_mem_bytes: info.shared_mem_bytes,
            barrier_count: info.barrier_count,
            cbufs,
        };
        let qmd_words = qmd::build_qmd_for_sm(self.sm_version(), &qmd_params);
        let qmd_bytes = u32_slice_as_bytes(&qmd_words);

        let qmd_handle = self.alloc(
            u64::try_from(qmd_bytes.len())
                .map_err(|_| DriverError::platform_overflow("qmd size fits in u64"))?,
            MemoryDomain::Gtt,
        )?;
        self.upload(qmd_handle, 0, qmd_bytes)?;
        let qmd_va = self.buffers.get(&qmd_handle.0).map_or(0, |b| b.gpu_va);

        let compute_class = self.gpu_gen.compute_class();
        let pb = PushBuf::compute_dispatch(compute_class, qmd_va, 0xFF00_0000_0000_0000);
        let pb_bytes = pb.as_bytes();

        let pb_handle = self.alloc(
            u64::try_from(pb_bytes.len())
                .map_err(|_| DriverError::platform_overflow("push buffer size fits in u64"))?,
            MemoryDomain::Gtt,
        )?;
        self.upload(pb_handle, 0, pb_bytes)?;
        let pb_va = self.buffers.get(&pb_handle.0).map_or(0, |b| b.gpu_va);

        let pb_dwords = u32::try_from(pb.as_words().len())
            .map_err(|_| DriverError::platform_overflow("push buffer dwords fits u32"))?;

        self.submit_gpfifo(pb_va, pb_dwords)?;

        self.inflight.push(shader_handle);
        self.inflight.push(qmd_handle);
        self.inflight.push(pb_handle);
        self.inflight.push(desc_handle);

        Ok(())
    }

    fn sync(&mut self) -> DriverResult<()> {
        self.poll_gpfifo_completion()?;
        let inflight = std::mem::take(&mut self.inflight);
        for handle in inflight {
            let _ = self.free(handle);
        }
        Ok(())
    }
}

// SAFETY: All internal state is protected by the struct's ownership semantics.
// GPU operations are serialized through the channel's pushbuffer protocol. File
// descriptors are thread-safe.
unsafe impl Send for NvUvmComputeDevice {}

// SAFETY: All internal state is protected by the struct's ownership semantics.
// GPU operations are serialized through the channel's pushbuffer protocol. File
// descriptors are thread-safe.
unsafe impl Sync for NvUvmComputeDevice {}

impl Drop for NvUvmComputeDevice {
    fn drop(&mut self) {
        let inflight = std::mem::take(&mut self.inflight);
        for h in inflight {
            let _ = self.free(h);
        }
        let handles: Vec<u32> = self.buffers.keys().copied().collect();
        for h in handles {
            let _ = self.free(BufferHandle(h));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_gen_class_selection() {
        assert_eq!(GpuGen::from_sm(70).channel_class(), VOLTA_CHANNEL_GPFIFO_A);
        assert_eq!(GpuGen::from_sm(70).compute_class(), VOLTA_COMPUTE_A);
        assert_eq!(GpuGen::from_sm(75).channel_class(), VOLTA_CHANNEL_GPFIFO_A);
        assert_eq!(GpuGen::from_sm(75).compute_class(), VOLTA_COMPUTE_A);
        assert_eq!(GpuGen::from_sm(80).channel_class(), AMPERE_CHANNEL_GPFIFO_A);
        assert_eq!(GpuGen::from_sm(80).compute_class(), AMPERE_COMPUTE_A);
        assert_eq!(GpuGen::from_sm(86).channel_class(), AMPERE_CHANNEL_GPFIFO_A);
        assert_eq!(GpuGen::from_sm(86).compute_class(), AMPERE_COMPUTE_B);
        assert_eq!(GpuGen::from_sm(89).compute_class(), ADA_COMPUTE_A);
        assert_eq!(GpuGen::from_sm(90).compute_class(), HOPPER_COMPUTE_A);
        assert_eq!(GpuGen::from_sm(100).compute_class(), BLACKWELL_COMPUTE_A);
        assert_eq!(GpuGen::from_sm(120).compute_class(), BLACKWELL_COMPUTE_B);
        assert_eq!(
            GpuGen::from_sm(120).channel_class(),
            BLACKWELL_CHANNEL_GPFIFO_B
        );
    }

    #[test]
    fn page_alignment() {
        assert_eq!(page_align(1), 4096);
        assert_eq!(page_align(4096), 4096);
        assert_eq!(page_align(4097), 8192);
        assert_eq!(page_align(0), 0);
    }

    #[test]
    fn gpfifo_entry_encoding() {
        let va = 0x0000_0001_0000_1000_u64;
        let dwords = 64_u32;
        let entry = gpfifo_entry(va, dwords);
        // DWORD 0 = address[31:0] (bits[1:0]=0 for alignment)
        let dw0 = entry as u32;
        assert_eq!(dw0, va as u32);
        // DWORD 1 bits[8:0] = address[40:32], bits[30:10] = length
        let dw1 = (entry >> 32) as u32;
        let decoded_addr_hi = (dw1 & 0x1FF) as u64;
        let decoded_va = (dw0 as u64) | (decoded_addr_hi << 32);
        assert_eq!(decoded_va, va);
        let decoded_len = (dw1 >> 10) & 0x1F_FFFF;
        assert_eq!(decoded_len, dwords);
    }

    #[test]
    fn gpfifo_entry_zero_length() {
        let entry = gpfifo_entry(0x1000, 0);
        let dw1 = (entry >> 32) as u32;
        assert_eq!((dw1 >> 10) & 0x1F_FFFF, 0);
        assert_eq!(entry as u32, 0x1000);
    }

    #[test]
    fn gpu_gen_sm_roundtrip() {
        assert_eq!(GpuGen::Volta, GpuGen::from_sm(70));
        assert_eq!(GpuGen::Turing, GpuGen::from_sm(75));
        assert_eq!(GpuGen::AmpereA, GpuGen::from_sm(80));
        assert_eq!(GpuGen::AmpereB, GpuGen::from_sm(86));
        assert_eq!(GpuGen::Ada, GpuGen::from_sm(89));
        assert_eq!(GpuGen::Hopper, GpuGen::from_sm(90));
        assert_eq!(GpuGen::BlackwellA, GpuGen::from_sm(100));
        assert_eq!(GpuGen::BlackwellB, GpuGen::from_sm(120));
    }

    fn detect_sm_version() -> u32 {
        std::process::Command::new("nvidia-smi")
            .args(["--query-gpu=compute_cap", "--format=csv,noheader"])
            .output()
            .ok()
            .and_then(|out| {
                let s = String::from_utf8_lossy(&out.stdout);
                let parts: Vec<&str> = s.trim().split('.').collect();
                if parts.len() == 2 {
                    let major: u32 = parts[0].parse().ok()?;
                    let minor: u32 = parts[1].parse().ok()?;
                    Some(major * 10 + minor)
                } else {
                    None
                }
            })
            .unwrap_or(86)
    }

    #[test]
    #[ignore = "requires proprietary nvidia driver loaded"]
    fn uvm_compute_device_open() {
        let sm = detect_sm_version();
        let device = NvUvmComputeDevice::open(0, sm).expect("UVM compute device");
        assert!(device.is_open());
    }

    #[test]
    #[ignore = "requires proprietary nvidia driver loaded"]
    fn uvm_compute_alloc_free() {
        let sm = detect_sm_version();
        let mut device = NvUvmComputeDevice::open(0, sm).expect("UVM compute device");
        let handle = device.alloc(4096, MemoryDomain::Gtt).expect("buffer alloc");
        device.free(handle).expect("buffer free");
    }

    #[test]
    #[ignore = "requires proprietary nvidia driver loaded"]
    fn uvm_map_memory_single_context() {
        use crate::nv::uvm::{NvGpuDevice, NvUvmDevice, RmClient};

        let mut client = RmClient::new().expect("RM root client");
        let uvm = NvUvmDevice::open().expect("open UVM");
        let gpu = NvGpuDevice::open(0).expect("open GPU");
        gpu.register_fd(client.ctl_fd()).expect("register GPU fd");
        uvm.initialize().expect("UVM_INITIALIZE");

        let h_device = client.alloc_device(gpu.index()).expect("RM device");
        let h_subdevice = client.alloc_subdevice(h_device).expect("RM subdevice");
        let _uuid = client
            .register_gpu_with_uvm(h_subdevice, &uvm)
            .expect("register UVM");

        // On Blackwell (580.x), only one rm_map_memory context per nvidiactl fd.
        // Verify the combined-allocation strategy from open() works.
        let h_mem = h_device + 0x5000;
        let combined_size = USERD_SIZE + GPFIFO_SIZE;
        client
            .alloc_system_memory(h_device, h_mem, combined_size)
            .expect("alloc combined");
        let addr = client
            .rm_map_memory(h_device, h_mem, 0, combined_size)
            .expect("rm_map_memory combined");
        assert!(addr != 0);

        let userd_ptr = addr as *mut u32;
        let gpfifo_ptr = (addr + USERD_SIZE) as *mut u32;
        // SAFETY: addr is a valid kernel mmap'd address from rm_map_memory
        // (asserted non-null above). userd_ptr and gpfifo_ptr are within the
        // mapped range (USERD_SIZE + GPFIFO_SIZE). Volatile reads/writes
        // match the GPU-visible mapping semantics.
        unsafe {
            crate::mmio::VolatilePtr::new(userd_ptr).write(0xDEAD_BEEF);
            crate::mmio::VolatilePtr::new(gpfifo_ptr).write(0xCAFE_BABE);
            assert_eq!(crate::mmio::VolatilePtr::new(userd_ptr).read(), 0xDEAD_BEEF);
            assert_eq!(
                crate::mmio::VolatilePtr::new(gpfifo_ptr).read(),
                0xCAFE_BABE
            );
        }

        client
            .rm_unmap_memory(h_device, h_mem, addr)
            .expect("unmap");
    }
}
