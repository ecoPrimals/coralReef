// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`ComputeDevice`] implementation, sync/drop, and `Send`/`Sync` markers.

use std::os::fd::AsRawFd;

use crate::error::DriverError;
use crate::error::DriverResult;
use crate::nv::pushbuf::PushBuf;
use crate::nv::qmd;
use crate::nv::uvm::constants::{nv_ctl_path, nv_gpu_path_prefix};
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use super::device::NvUvmComputeDevice;
use super::types::{UvmBuffer, page_align, u32_slice_as_bytes, uvm_cache_line_flush};

impl ComputeDevice for NvUvmComputeDevice {
    fn alloc(&mut self, size: u64, _domain: MemoryDomain) -> DriverResult<BufferHandle> {
        let aligned = page_align(size);

        // Blackwell via kmod (non-UVM path only): allocate VRAM + GPU VA
        // mapping from kernel context. On Blackwell UVM, fall through to
        // the system-memory path below since the kmod DMA mapping doesn't
        // work with externally-owned VA spaces.
        if let Some(ref kmod) = self.coral_kmod && !self.uses_uvm_mapping {
            let (h_mem, gpu_va) = kmod.alloc_gpu_buffer(self.kmod_h_client, aligned)?;

            // CPU mapping via BAR1 (GPU device fd) for VRAM access.
            let gpu_path = format!("{}{}", nv_gpu_path_prefix(), self.gpu.index());
            let mmap_file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&gpu_path)
                .map_err(|e| {
                    DriverError::DeviceNotFound(
                        format!("{gpu_path} for VRAM mmap: {e}").into(),
                    )
                })?;
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
            return Ok(BufferHandle(handle_id));
        }

        // Non-kmod path: system memory + userspace DMA mapping.
        let h_mem = self.alloc_mem_handle();
        self.client
            .alloc_system_memory(self.h_device, h_mem, aligned)?;

        let gpu_va = self.gpu_map_buffer(h_mem, aligned)?;

        let mmap_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(nv_ctl_path())
            .map_err(|e| {
                DriverError::DeviceNotFound(format!("{} for mmap: {e}", nv_ctl_path()).into())
            })?;
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
        if buf.gpu_va != 0 {
            if self.uses_uvm_mapping {
                let _ = self.uvm.uvm_free(buf.gpu_va, buf.size, &self.gpu_uuid);
            } else {
                let _ = self.client.rm_unmap_memory_dma(
                    self.h_device,
                    self.h_virt_mem,
                    buf.h_memory,
                    buf.gpu_va,
                );
            }
        }
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

        // Invalidate CPU cache lines covering the readback range so we see
        // the GPU's writes (which went through GPU L2 → DRAM, bypassing the
        // CPU cache hierarchy).
        #[cfg(target_arch = "x86_64")]
        {
            let base = (buf.cpu_addr + offset) as *const u8;
            let mut off = 0_usize;
            while off < len {
                // SAFETY: `offset + len <= buf.size` and `cpu_addr != 0`; each `base.add(off)` is in-range for the mmap.
                unsafe { uvm_cache_line_flush(base.add(off)) };
                off += 64;
            }
            std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
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
        // Colocate shader code and descriptor table in a single buffer so
        // both share the same GPU page mapping.  Descriptor data starts at a
        // 256-byte–aligned offset after the shader binary.
        //
        // SM120+ (Blackwell PTX ABI): ptxas places kernel parameters at
        // c[0][0x380] (EIATTR_PARAM_CBANK offset), NOT c[0][0x160]. The
        // .nv.constant0 section is 0x390 bytes. Descriptor-based STG.E
        // instructions also read from c[0][0x358].
        //
        // SM < 100 (SASS backend via coral-reef): parameters at c[0][0].
        let is_ptx_abi = self.sm_version() >= 100;
        let param_window_offset: usize = if is_ptx_abi { 0x380 } else { 0 };

        let shader_len = shader.len();
        let desc_offset = (shader_len + 255) & !255; // 256-byte align
        let desc_entry_size = 16_usize;
        let desc_entries = buffers.len().max(1);
        let desc_data_len = desc_entry_size * desc_entries;
        let cbuf_payload_end = param_window_offset + desc_data_len;
        // SM120 constant0 section is 0x390 bytes minimum.
        let min_cbuf_size: usize = if is_ptx_abi { 0x390 } else { 0 };
        let desc_cbuf_size = ((cbuf_payload_end.max(min_cbuf_size) + 63) & !63) as u32;
        let combined_size = u64::try_from(desc_offset + desc_cbuf_size as usize)
            .map_err(|_| DriverError::platform_overflow("combined size fits in u64"))?;

        let shader_handle = self.alloc(combined_size, MemoryDomain::Gtt)?;

        // DIAG_EXIT_ONLY: replace entire shader with a single EXIT instruction
        // to test whether the dispatch infrastructure itself works.
        let use_exit_only = std::env::var("DIAG_EXIT_ONLY").is_ok();
        if use_exit_only {
            let exit_shader: [u32; 4] = [0x0000794D, 0x00000000, 0x03800000, 0x03FFC000];
            let exit_bytes = bytemuck::cast_slice::<u32, u8>(&exit_shader);
            self.upload(shader_handle, 0, exit_bytes)?;
            tracing::warn!("DIAG_EXIT_ONLY: replaced shader with single EXIT instruction");
        }

        // DIAG_DIRECT_ADDR: patch the first two LDC instructions into MOV
        // immediates with the actual buffer VA, bypassing CBUF entirely.
        let use_direct_addr = !use_exit_only && std::env::var("DIAG_DIRECT_ADDR").is_ok();
        if use_direct_addr && !buffers.is_empty() {
            if let Some(buf) = self.buffers.get(&buffers[0].0) {
                let va = buf.gpu_va;
                let va_lo = (va & 0xFFFF_FFFF) as u32;
                let va_hi = (va >> 32) as u32;
                let mut patched = shader.to_vec();
                let words: &mut [u32] = bytemuck::cast_slice_mut(&mut patched);
                if words.len() >= 8 {
                    // Use the same 128-bit encoding as the existing MOV R2
                    // (instruction 2 at words[8..12]) for correct flag/sched bits.
                    let mov_w2 = if words.len() > 10 {
                        words[10]
                    } else {
                        0x0000_0F00
                    };

                    // Instr 0: MOV R0, va_lo
                    words[0] = 0x0000_7802; // opcode=MOV, pred=PT, dst=R0
                    words[1] = va_lo;
                    words[2] = mov_w2;
                    // word[3]: keep original (scheduling — first instr)

                    // Instr 1: MOV R1, va_hi
                    words[4] = 0x0001_7802; // opcode=MOV, pred=PT, dst=R1
                    words[5] = va_hi;
                    words[6] = mov_w2;
                    // word[7]: keep original (scheduling — second instr)

                    // Also patch STG memory ordering from Weak to Strong(System).
                    // STG is instruction 3 (words 12-15).  The mem_order field
                    // sits at instruction bits 77-80, which is word 2 bits 13-16.
                    // Strong(System) = 0xa → bits 13=0, 14=1, 15=0, 16=1.
                    if words.len() > 14 {
                        words[14] = (words[14] & !(0xF << 13)) | (0xa << 13);
                    }

                    tracing::warn!(
                        va = format_args!("0x{va:016X}"),
                        "DIAG_DIRECT_ADDR: patched LDC→MOV + STG mem_order→Strong"
                    );
                }
                self.upload(shader_handle, 0, &patched)?;
            } else {
                self.upload(shader_handle, 0, shader)?;
            }
        } else {
            self.upload(shader_handle, 0, shader)?;
        }

        let shader_va = self.buffers.get(&shader_handle.0).map_or(0, |b| b.gpu_va);

        // Build CBUF descriptor table inside the same buffer.
        //
        // SM < 100 (SASS backend): descriptors at c[0][binding * 16].
        // SM >= 100 (PTX backend): descriptors at c[0][0x160 + binding * 16]
        //   because ptxas maps `ld.param` to c[0][0x160+].
        let mut cbuf_data = vec![0u8; desc_cbuf_size as usize];
        for (i, bh) in buffers.iter().enumerate() {
            if let Some(buf) = self.buffers.get(&bh.0) {
                let off = param_window_offset + i * 16;
                let va = buf.gpu_va;
                let sz = u32::try_from(buf.size).unwrap_or(u32::MAX);
                let va_lo = (va & 0xFFFF_FFFF) as u32;
                let va_hi = (va >> 32) as u32;
                cbuf_data[off..off + 4].copy_from_slice(&va_lo.to_le_bytes());
                cbuf_data[off + 4..off + 8].copy_from_slice(&va_hi.to_le_bytes());
                cbuf_data[off + 8..off + 12].copy_from_slice(&sz.to_le_bytes());
            }
        }
        self.upload(
            shader_handle,
            u64::try_from(desc_offset)
                .map_err(|_| DriverError::platform_overflow("desc_offset fits u64"))?,
            &cbuf_data,
        )?;
        let desc_va = shader_va + desc_offset as u64;

        // CBUF 7: driver constants (grid dimensions for num_workgroups).
        let driver_const_handle =
            self.alloc(u64::from(qmd::DRIVER_CONST_SIZE), MemoryDomain::Gtt)?;
        let driver_consts = qmd::encode_driver_constants(&dims);
        self.upload(driver_const_handle, 0, &driver_consts)?;
        let driver_const_va = self
            .buffers
            .get(&driver_const_handle.0)
            .map_or(0, |b| b.gpu_va);

        // CBUFs 0-6 → descriptor table; CBUF 7 → driver constants.
        let cbufs = qmd::build_standard_cbufs(
            desc_va,
            desc_cbuf_size,
            driver_const_va,
            qmd::DRIVER_CONST_SIZE,
        );

        let qmd_params = qmd::QmdParams {
            shader_va,
            grid: dims,
            workgroup: info.workgroup,
            gpr_count: info.gpr_count.max(4),
            shared_mem_bytes: info.shared_mem_bytes,
            barrier_count: info.barrier_count,
            local_mem_low_bytes: info.local_mem_bytes.unwrap_or(0),
            cbufs,
        };

        tracing::debug!(
            shader_va = format_args!("0x{shader_va:016X}"),
            desc_va = format_args!("0x{desc_va:016X}"),
            grid = ?dims,
            wg = ?info.workgroup,
            gpr = info.gpr_count,
            sm = self.sm_version(),
            buffers = buffers.len(),
            "dispatch"
        );

        let qmd_words = qmd::build_qmd_for_sm(self.sm_version(), &qmd_params);
        let qmd_bytes = u32_slice_as_bytes(&qmd_words);

        let qmd_handle = self.alloc(
            u64::try_from(qmd_bytes.len())
                .map_err(|_| DriverError::platform_overflow("qmd size fits in u64"))?,
            MemoryDomain::Gtt,
        )?;
        self.upload(qmd_handle, 0, qmd_bytes)?;
        let qmd_va = self.buffers.get(&qmd_handle.0).map_or(0, |b| b.gpu_va);

        tracing::debug!(
            qmd_va = format_args!("0x{qmd_va:016X}"),
            aligned256 = qmd_va.is_multiple_of(256),
            qmd_words = qmd_words.len(),
            desc_cbuf_size,
            "dispatch qmd"
        );
        for row in 0..qmd_words.len() / 4 {
            let base = row * 4;
            if qmd_words[base..base + 4].iter().all(|&w| w == 0) {
                continue;
            }
            tracing::debug!(
                row_start = base,
                row_end = base + 3,
                bit_base = base * 32,
                w0 = format_args!("{:08X}", qmd_words[base]),
                w1 = format_args!("{:08X}", qmd_words[base + 1]),
                w2 = format_args!("{:08X}", qmd_words[base + 2]),
                w3 = format_args!("{:08X}", qmd_words[base + 3]),
                "qmd row"
            );
        }

        let compute_class = self.gpu_gen.compute_class();
        let launch = if compute_class > crate::nv::pushbuf::method::TURING_COMPUTE_A {
            crate::nv::generation::LaunchMethod::Pcas2
        } else {
            crate::nv::generation::LaunchMethod::Pcas
        };
        let pb = PushBuf::compute_dispatch_on_subchannel(launch, qmd_va, self.compute_subchannel);
        let pb_bytes = pb.as_bytes();

        tracing::debug!(push_buf_words = pb.as_words().len(), "dispatch push_buf layout");
        for (i, w) in pb.as_words().iter().enumerate() {
            let is_hdr = i % 2 == 0;
            if is_hdr {
                let subchan = (*w >> 13) & 0x7;
                let method = (*w & 0x1FFF) << 2;
                let count = (*w >> 16) & 0x1FFF;
                tracing::debug!(
                    i,
                    word = format_args!("0x{w:08X}"),
                    subchan,
                    method = format_args!("0x{method:04X}"),
                    count,
                    "push_buf hdr"
                );
            } else {
                tracing::debug!(i, word = format_args!("0x{w:08X}"), "push_buf dat");
            }
        }

        let pb_handle = self.alloc(
            u64::try_from(pb_bytes.len())
                .map_err(|_| DriverError::platform_overflow("push buffer size fits in u64"))?,
            MemoryDomain::Gtt,
        )?;
        self.upload(pb_handle, 0, pb_bytes)?;
        let pb_va = self.buffers.get(&pb_handle.0).map_or(0, |b| b.gpu_va);

        let pb_dwords = u32::try_from(pb.as_words().len())
            .map_err(|_| DriverError::platform_overflow("push buffer dwords fits u32"))?;

        // Flush CPU cache lines for all uploaded buffers so the GPU's DMA
        // reads see the latest data. RM recycles GPU VAs after free — if
        // physical pages shuffle, stale GPU TLB entries could read old data.
        #[cfg(target_arch = "x86_64")]
        {
            for &h in &[shader_handle, qmd_handle, pb_handle, driver_const_handle] {
                if let Some(buf) = self.buffers.get(&h.0)
                    && buf.cpu_addr != 0
                    && buf.size > 0
                {
                    let base = buf.cpu_addr as *const u8;
                    let mut off = 0_u64;
                    while off < buf.size {
                        // SAFETY: cpu_addr is valid mmap for buf.size bytes.
                        unsafe {
                            uvm_cache_line_flush(base.add(off as usize));
                        }
                        off += 64; // cache line size
                    }
                }
            }
            std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        }

        tracing::debug!(
            pb_va = format_args!("0x{pb_va:016X}"),
            pb_dwords,
            gp_put_before = self.gp_put,
            "dispatch submit"
        );

        if !buffers.is_empty()
            &&             let Some(buf) = self.buffers.get(&buffers[0].0)
        {
            tracing::debug!(
                gpu_va = format_args!("0x{:016X}", buf.gpu_va),
                size = buf.size,
                cpu_addr = format_args!("0x{:016X}", buf.cpu_addr),
                "dispatch buf[0]"
            );
        }

        self.submit_gpfifo(pb_va, pb_dwords)?;

        if self.uses_semaphore_fence {
            tracing::debug!(
                fence_value_before = self.fence_value,
                fence_gpu_va = format_args!("0x{:016X}", self.fence_gpu_va),
                "dispatch fence"
            );
            self.submit_fence_release()?;
        }

        self.inflight.push(shader_handle);
        self.inflight.push(qmd_handle);
        self.inflight.push(pb_handle);
        self.inflight.push(driver_const_handle);

        Ok(())
    }

    fn sync(&mut self) -> DriverResult<()> {
        self.poll_gpfifo_completion()?;
        // Defer frees: keep temporary buffers alive to avoid VA recycling
        // races where the GPU may still be touching the previous dispatch's
        // memory when the next alloc reuses the same VA.
        let inflight = std::mem::take(&mut self.inflight);
        self.deferred_free.extend(inflight);
        Ok(())
    }

    fn capabilities(&self) -> &crate::HardwareCapabilities {
        &self.caps
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
        // Free GR context buffers promoted via GPU_PROMOTE_CTX.
        let ctx_bufs = std::mem::take(&mut self.ctx_buffers);
        for cb in ctx_bufs {
            if cb.gpu_va != 0 {
                if self.uses_uvm_mapping {
                    let _ = self.uvm.uvm_free(cb.gpu_va, cb.size, &self.gpu_uuid);
                } else {
                    let _ = self.client.rm_unmap_memory_dma(
                        self.h_device,
                        self.h_virt_mem,
                        cb.h_memory,
                        cb.gpu_va,
                    );
                }
            }
            let _ = self.client.free_object(self.h_device, cb.h_memory);
        }
    }
}
