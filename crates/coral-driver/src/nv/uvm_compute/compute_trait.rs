// SPDX-License-Identifier: AGPL-3.0-only
//! [`ComputeDevice`] implementation, sync/drop, and `Send`/`Sync` markers.

use std::os::fd::AsRawFd;

use crate::error::DriverError;
use crate::error::DriverResult;
use crate::nv::pushbuf::PushBuf;
use crate::nv::qmd;
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use super::device::NvUvmComputeDevice;
use super::types::{UvmBuffer, page_align, u32_slice_as_bytes};

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

// SAFETY: See the "Thread safety (`Send` / `Sync`)" section on `NvUvmComputeDevice` in `device.rs`.
unsafe impl Send for NvUvmComputeDevice {}

// SAFETY: See the "Thread safety (`Send` / `Sync`)" section on `NvUvmComputeDevice` in `device.rs`.
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
