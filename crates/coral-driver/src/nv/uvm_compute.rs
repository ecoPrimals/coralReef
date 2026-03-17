// SPDX-License-Identifier: AGPL-3.0-only
//! UVM-based compute device — dispatches via the NVIDIA proprietary driver.
//!
//! Bypasses nouveau entirely, using the RM object hierarchy through
//! `/dev/nvidiactl` and UVM through `/dev/nvidia-uvm` for memory management.
//! Reuses the identical QMD and push buffer formats from the nouveau path.

use std::collections::HashMap;

use crate::error::{DriverError, DriverResult};
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use super::qmd;
use super::uvm::{
    AMPERE_CHANNEL_GPFIFO_A, AMPERE_COMPUTE_A, AMPERE_COMPUTE_B, NvGpuDevice, NvUvmDevice,
    RmClient, VOLTA_CHANNEL_GPFIFO_A, VOLTA_COMPUTE_A,
};

/// GPU generation derived from SM version, used for class selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GpuGen {
    Volta,
    Turing,
    /// GA100 (A100, SM 8.0) — uses AMPERE_COMPUTE_A.
    AmpereA,
    /// GA10x (RTX 30xx, SM 8.6+) — uses AMPERE_COMPUTE_B.
    AmpereB,
}

impl GpuGen {
    const fn from_sm(sm: u32) -> Self {
        match sm {
            75 => Self::Turing,
            80 => Self::AmpereA,
            81..=89 => Self::AmpereB,
            _ => Self::Volta,
        }
    }

    const fn channel_class(self) -> u32 {
        match self {
            Self::AmpereA | Self::AmpereB => AMPERE_CHANNEL_GPFIFO_A,
            Self::Volta | Self::Turing => VOLTA_CHANNEL_GPFIFO_A,
        }
    }

    const fn compute_class(self) -> u32 {
        match self {
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
}

/// GPFIFO entry in the ring buffer (8 bytes).
///
/// ```text
/// [63:42] = length in dwords (push buffer size / 4)
/// [41:40] = control bits (0 = normal)
/// [39:2]  = GPU VA of push buffer >> 2
/// [1:0]   = fetch type (0 = normal)
/// ```
const fn gpfifo_entry(push_buf_va: u64, length_dwords: u32) -> u64 {
    ((push_buf_va >> 2) & 0x00_0000_003F_FFFF_FFFF) | ((length_dwords as u64) << 42)
}

/// USERD GP_PUT doorbell offset (bytes).
const USERD_GP_PUT_OFFSET: usize = 0x0C;

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
    /// Inflight temporary buffers that survive until sync().
    inflight: Vec<BufferHandle>,
    /// CPU-mapped pointer to the USERD page (for GP_PUT doorbell writes).
    userd_cpu_addr: u64,
    /// CPU-mapped pointer to the GPFIFO ring (for writing GPFIFO entries).
    gpfifo_cpu_addr: u64,
    /// Current GP_PUT index (next slot to write in the GPFIFO ring).
    gp_put: u32,
    /// Handle of the NV01_MEMORY_VIRTUAL for DMA mapping.
    h_virt_mem: u32,
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
    /// ROOT → DEVICE → SUBDEVICE → UUID query → UVM_REGISTER_GPU →
    /// VA_SPACE → CHANNEL_GROUP → GPFIFO → COMPUTE
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

        let h_vaspace = client.alloc_vaspace(h_device)?;
        let h_changrp = client.alloc_channel_group(h_device, h_vaspace)?;

        let h_gpfifo_mem = h_device + 0x5000;
        let h_userd_mem = h_device + 0x5001;
        let h_virt_mem = h_device + 0x5002;
        client.alloc_system_memory(h_device, h_gpfifo_mem, GPFIFO_SIZE)?;
        client.alloc_system_memory(h_device, h_userd_mem, USERD_SIZE)?;
        client.alloc_virtual_memory(h_device, h_virt_mem, h_vaspace)?;

        let gpfifo_gpu_va =
            client.rm_map_memory_dma(h_device, h_virt_mem, h_gpfifo_mem, 0, GPFIFO_SIZE)?;

        let h_channel = client.alloc_gpfifo_channel(
            h_changrp,
            h_userd_mem,
            gpfifo_gpu_va,
            GPFIFO_ENTRIES,
            gpu_gen.channel_class(),
        )?;

        let h_compute = client.alloc_compute_engine(h_channel, gpu_gen.compute_class())?;

        let userd_cpu_addr = client.rm_map_memory(h_device, h_userd_mem, 0, USERD_SIZE)?;
        let gpfifo_cpu_addr = client.rm_map_memory(h_device, h_gpfifo_mem, 0, GPFIFO_SIZE)?;

        tracing::info!(
            gpu_index,
            sm,
            h_device = format_args!("0x{h_device:08X}"),
            h_channel = format_args!("0x{h_channel:08X}"),
            h_compute = format_args!("0x{h_compute:08X}"),
            userd_cpu_addr = format_args!("0x{userd_cpu_addr:016X}"),
            gpfifo_cpu_addr = format_args!("0x{gpfifo_cpu_addr:016X}"),
            "NvUvmComputeDevice fully initialized"
        );

        Ok(Self {
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
        })
    }

    fn alloc_handle(&mut self) -> u32 {
        let h = self.next_handle;
        self.next_handle += 1;
        h
    }

    fn alloc_mem_handle(&mut self) -> u32 {
        let h = self.next_mem_handle;
        self.next_mem_handle += 1;
        h
    }

    /// The SM version this device targets.
    #[must_use]
    pub fn sm_version(&self) -> u32 {
        match self.gpu_gen {
            GpuGen::Volta => 70,
            GpuGen::Turing => 75,
            GpuGen::AmpereA => 80,
            GpuGen::AmpereB => 86,
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
        unsafe {
            std::ptr::write_volatile(gpfifo_slot, entry);
        }

        self.gp_put = self.gp_put.wrapping_add(1);

        if self.userd_cpu_addr == 0 {
            return Err(DriverError::SubmitFailed("USERD not CPU-mapped".into()));
        }

        let doorbell = (self.userd_cpu_addr + USERD_GP_PUT_OFFSET as u64) as *mut u32;
        // SAFETY: userd_cpu_addr is a valid kernel mmap'd address.
        // GP_PUT offset (0x0C) is within the 4096-byte USERD page.
        unsafe {
            std::ptr::write_volatile(doorbell, self.gp_put);
        }

        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

        tracing::debug!(
            gp_put = self.gp_put,
            push_buf_va = format_args!("0x{push_buf_va:016X}"),
            length_dwords,
            "GPFIFO entry submitted"
        );
        Ok(())
    }

    /// Poll for GPFIFO completion by checking GP_GET in the USERD page.
    fn poll_gpfifo_completion(&self) -> DriverResult<()> {
        if self.userd_cpu_addr == 0 || self.gp_put == 0 {
            return Ok(());
        }

        const USERD_GP_GET_OFFSET: usize = 0x08;
        let gp_get_ptr = (self.userd_cpu_addr + USERD_GP_GET_OFFSET as u64) as *const u32;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            // SAFETY: userd_cpu_addr is a valid kernel mmap'd address.
            let gp_get = unsafe { std::ptr::read_volatile(gp_get_ptr) };
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

        let cpu_addr = self
            .client
            .rm_map_memory(self.h_device, h_mem, 0, aligned)?;

        let handle_id = self.alloc_handle();
        self.buffers.insert(
            handle_id,
            UvmBuffer {
                h_memory: h_mem,
                size: aligned,
                gpu_va,
                cpu_addr,
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
        // returned by the kernel's vm_mmap. The bounds check above ensures
        // offset + data.len() <= buf.size <= mapped length.
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

        // SAFETY: same as upload — cpu_addr is a kernel-provided vm_mmap address,
        // bounds checked above.
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

        let mut desc_data = vec![0u8; desc_buf_size as usize];
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

        let qmd_handle = self.alloc(qmd_bytes.len() as u64, MemoryDomain::Gtt)?;
        self.upload(qmd_handle, 0, qmd_bytes)?;
        let qmd_va = self.buffers.get(&qmd_handle.0).map_or(0, |b| b.gpu_va);

        let compute_class = self.gpu_gen.compute_class();
        let pb = PushBuf::compute_dispatch(compute_class, qmd_va, 0xFF00_0000_0000_0000);
        let pb_bytes = pb.as_bytes();

        let pb_handle = self.alloc(pb_bytes.len() as u64, MemoryDomain::Gtt)?;
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
        let decoded_va = (entry & 0x00_0000_003F_FFFF_FFFF) << 2;
        assert_eq!(decoded_va, va);
        let decoded_len = (entry >> 42) as u32;
        assert_eq!(decoded_len, dwords);
    }

    #[test]
    fn gpfifo_entry_zero_length() {
        let entry = gpfifo_entry(0x1000, 0);
        assert_eq!(entry >> 42, 0);
        assert_ne!(entry & 0x00_0000_003F_FFFF_FFFF, 0);
    }

    #[test]
    fn gpu_gen_sm_roundtrip() {
        assert_eq!(GpuGen::Volta, GpuGen::from_sm(70));
        assert_eq!(GpuGen::Turing, GpuGen::from_sm(75));
        assert_eq!(GpuGen::AmpereA, GpuGen::from_sm(80));
        assert_eq!(GpuGen::AmpereB, GpuGen::from_sm(86));
    }

    #[test]
    #[ignore = "requires proprietary nvidia driver loaded"]
    fn uvm_compute_device_open() {
        let device = NvUvmComputeDevice::open(0, 86).expect("UVM compute device");
        assert!(device.is_open());
        eprintln!("UVM compute device opened, SM{}", device.sm_version());
    }

    #[test]
    #[ignore = "requires proprietary nvidia driver loaded"]
    fn uvm_compute_alloc_free() {
        let mut device = NvUvmComputeDevice::open(0, 86).expect("UVM compute device");
        let handle = device.alloc(4096, MemoryDomain::Gtt).expect("buffer alloc");
        device.free(handle).expect("buffer free");
    }
}
