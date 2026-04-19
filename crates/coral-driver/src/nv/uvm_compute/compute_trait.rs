// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`ComputeDevice`] implementation, sync/drop, and `Send`/`Sync` markers.

use std::os::fd::AsRawFd;

use crate::error::DriverError;
use crate::error::DriverResult;
use crate::nv::pushbuf::PushBuf;
use crate::nv::qmd;
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use super::device::NvUvmComputeDevice;
use super::types::{GpuGen, UvmBuffer, page_align, u32_slice_as_bytes, uvm_cache_line_flush};

impl ComputeDevice for NvUvmComputeDevice {
    fn alloc(&mut self, size: u64, _domain: MemoryDomain) -> DriverResult<BufferHandle> {
        let aligned = page_align(size);

        // Blackwell via kmod: allocate VRAM + GPU VA mapping from kernel
        // context to ensure GPU page tables are eagerly populated.
        if let Some(ref kmod) = self.coral_kmod {
            let (h_mem, gpu_va) = kmod.alloc_gpu_buffer(self.kmod_h_client, aligned)?;

            // CPU mapping via BAR1 (GPU device fd) for VRAM access.
            let mmap_file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(format!("/dev/nvidia{}", self.gpu.index()))
                .map_err(|e| {
                    DriverError::DeviceNotFound(
                        format!("nvidia{} for VRAM mmap: {e}", self.gpu.index()).into(),
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
            .open("/dev/nvidiactl")
            .map_err(|e| {
                DriverError::DeviceNotFound(format!("nvidiactl for mmap: {e}").into())
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
        // Unmap GPU VA first so RM tears down page table entries and the
        // GPU TLB won't hold stale mappings when the same VA is reused.
        if buf.gpu_va != 0 {
            let _ = self.client.rm_unmap_memory_dma(
                self.h_device,
                self.h_virt_mem,
                buf.h_memory,
                buf.gpu_va,
            );
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
        let shader_len = shader.len();
        let desc_offset = (shader_len + 255) & !255; // 256-byte align
        let desc_entry_size = 16_usize;
        let desc_entries = buffers.len().max(1);
        let desc_data_len = desc_entry_size * desc_entries;
        // Align overall CBUF size to 64 bytes (NVK min_cbuf_alignment for
        // Turing+).
        let desc_cbuf_size = ((desc_data_len + 63) & !63) as u32;
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
            eprintln!("    DIAG_EXIT_ONLY: replaced shader with single EXIT instruction (16 bytes)");
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
                let words: &mut [u32] =
                    bytemuck::cast_slice_mut(&mut patched);
                if words.len() >= 8 {
                    // Use the same 128-bit encoding as the existing MOV R2
                    // (instruction 2 at words[8..12]) for correct flag/sched bits.
                    let mov_w2 = if words.len() > 10 { words[10] } else { 0x0000_0F00 };

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
                        eprintln!("    DIAG_DIRECT_ADDR: also patched STG mem_order → Strong(System)");
                    }

                    eprintln!(
                        "    DIAG_DIRECT_ADDR: patched LDC→MOV, R0=0x{va_lo:08X} R1=0x{va_hi:08X} (VA=0x{va:016X})"
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

        // Build CBUF descriptor table at the aligned offset inside the
        // same buffer.  The compiler generates `c[0][binding * 8]` to
        // load buffer addresses, so CBUF 0 must hold the descriptor table.
        let mut desc_data = vec![0u8; desc_data_len];
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
        self.upload(
            shader_handle,
            u64::try_from(desc_offset)
                .map_err(|_| DriverError::platform_overflow("desc_offset fits u64"))?,
            &desc_data,
        )?;
        let desc_va = shader_va + desc_offset as u64;
        let desc_handle = shader_handle; // same buffer; for inflight tracking
        let desc_buf_size = desc_data_len as u64;

        // Set ALL 8 CBUFs to the descriptor table to diagnose which
        // index the hardware actually maps c[0] to on Blackwell.
        let cbufs: Vec<qmd::CbufBinding> = (0..8)
            .map(|i| qmd::CbufBinding {
                index: i,
                addr: desc_va,
                size: desc_cbuf_size,
            })
            .collect();

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

        eprintln!(
            "    DISPATCH: shader_va=0x{shader_va:016X} desc_va=0x{desc_va:016X} desc_size={desc_buf_size} \
             grid={dims:?} wg={:?} gpr={} sm={}",
            info.workgroup, info.gpr_count, self.sm_version()
        );
        for (i, bh) in buffers.iter().enumerate() {
            if let Some(buf) = self.buffers.get(&bh.0) {
                eprintln!("      buf[{i}]: gpu_va=0x{:016X} size={}", buf.gpu_va, buf.size);
            }
        }

        // Hex dump descriptor table for debugging address loading issues
        eprintln!("    DESC TABLE ({} bytes):", desc_data.len());
        for chunk in desc_data.chunks(16) {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            eprintln!("      {}", hex.join(" "));
        }

        // Hex dump first 128 bytes of shader binary
        let shader_preview = shader.len().min(128);
        eprintln!("    SHADER BINARY (first {} of {} bytes):", shader_preview, shader.len());
        for chunk in shader[..shader_preview].chunks(16) {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            eprintln!("      {}", hex.join(" "));
        }

        let qmd_words = qmd::build_qmd_for_sm(self.sm_version(), &qmd_params);
        let qmd_bytes = u32_slice_as_bytes(&qmd_words);

        // Hex dump QMD for debugging field encoding
        eprintln!("    QMD ({} words = {} bytes):", qmd_words.len(), qmd_bytes.len());
        for (i, chunk) in qmd_words.chunks(8).enumerate() {
            let hex: Vec<String> = chunk.iter().map(|w| format!("{w:08x}")).collect();
            eprintln!("      [{:2}..{:2}] {}", i * 8, i * 8 + chunk.len() - 1, hex.join(" "));
        }

        let qmd_handle = self.alloc(
            u64::try_from(qmd_bytes.len())
                .map_err(|_| DriverError::platform_overflow("qmd size fits in u64"))?,
            MemoryDomain::Gtt,
        )?;
        self.upload(qmd_handle, 0, qmd_bytes)?;
        let qmd_va = self.buffers.get(&qmd_handle.0).map_or(0, |b| b.gpu_va);

        let compute_class = self.gpu_gen.compute_class();
        let pb = PushBuf::compute_dispatch(compute_class, qmd_va);
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

        // Flush CPU cache lines for all uploaded buffers so the GPU's DMA
        // reads see the latest data. RM recycles GPU VAs after free — if
        // physical pages shuffle, stale GPU TLB entries could read old data.
        #[cfg(target_arch = "x86_64")]
        {
            for &h in &[shader_handle, qmd_handle, pb_handle] {
                if let Some(buf) = self.buffers.get(&h.0) {
                    if buf.cpu_addr != 0 && buf.size > 0 {
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
            }
            std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        }

        self.submit_gpfifo(pb_va, pb_dwords)?;

        // On Blackwell+, GP_GET is not in USERD — submit a semaphore release
        // so poll_gpfifo_completion can track completion via the fence value.
        if self.uses_semaphore_fence {
            self.submit_fence_release()?;
        }

        self.inflight.push(shader_handle);
        self.inflight.push(qmd_handle);
        self.inflight.push(pb_handle);

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
                let _ = self.client.rm_unmap_memory_dma(
                    self.h_device,
                    self.h_virt_mem,
                    cb.h_memory,
                    cb.gpu_va,
                );
            }
            let _ = self.client.free_object(self.h_device, cb.h_memory);
        }
    }
}
