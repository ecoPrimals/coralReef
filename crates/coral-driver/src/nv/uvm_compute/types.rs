// SPDX-License-Identifier: AGPL-3.0-or-later
//! GPU generation, GPFIFO encoding, and per-buffer bookkeeping for UVM compute.

use crate::nv::generation;

/// Flush a single CPU cache line containing the given address.
///
/// On x86_64: `CLFLUSH` instruction. Other architectures: no-op (cache-coherent
/// or handled by DMA mapping). Used for GPFIFO/USERD doorbell writes where the
/// GPU DMA engine reads from system memory.
///
/// # Safety
///
/// `addr` must point to valid mapped memory.
#[cfg(target_arch = "x86_64")]
#[inline]
pub(super) unsafe fn uvm_cache_line_flush(addr: *const u8) {
    // SAFETY: Caller guarantees `addr` points to valid mapped memory.
    unsafe { core::arch::x86_64::_mm_clflush(addr) }
}

/// No-op on non-x86_64 (cache-coherent DMA or hardware-managed).
///
/// # Safety
///
/// `addr` must point to valid mapped memory (same contract as the x86_64 variant).
#[cfg(not(target_arch = "x86_64"))]
#[inline]
pub(super) unsafe fn uvm_cache_line_flush(_addr: *const u8) {}

/// GPU generation derived from SM version, used for class selection.
///
/// Thin wrapper around [`generation::GenerationProfile`] — delegates all
/// class lookups through the unified profile registry. Retained as a
/// local type so that existing UVM code continues to compile; callers
/// should prefer `generation::profile_for_sm` directly where possible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GpuGen {
    pub(super) sm: u32,
}

impl GpuGen {
    pub(super) const fn from_sm(sm: u32) -> Self {
        Self { sm }
    }

    pub(super) fn channel_class(self) -> u32 {
        generation::profile_for_sm(self.sm).channel_class
    }

    pub(super) fn compute_class(self) -> u32 {
        generation::profile_for_sm(self.sm).compute_class
    }

    pub(super) fn uses_semaphore_fence(self) -> bool {
        generation::uses_semaphore_fence(generation::profile_for_sm(self.sm))
    }
}

/// An explicitly-allocated GR context buffer promoted to RM.
///
/// These are allocated during `NvUvmComputeDevice::open()` and promoted
/// via `GPU_PROMOTE_CTX` to replace demand-paged internal buffers.
pub(super) struct CtxBuffer {
    pub(super) buffer_id: u16,
    pub(super) h_memory: u32,
    #[expect(dead_code, reason = "diagnostic use in future iterations")]
    pub(super) size: u64,
    pub(super) gpu_va: u64,
}

/// A buffer allocated via RM + UVM.
pub(super) struct UvmBuffer {
    pub(super) h_memory: u32,
    pub(super) size: u64,
    pub(super) gpu_va: u64,
    /// CPU linear address from `NV_ESC_RM_MAP_MEMORY` (0 = not mapped).
    pub(super) cpu_addr: u64,
    /// Dedicated nvidiactl fd that holds this buffer's mmap context. On
    /// Blackwell (580.x), each nvidiactl fd supports only one active
    /// mmap context, so each buffer needs its own fd.
    #[expect(dead_code, reason = "kept alive for mmap lifetime")]
    pub(super) mmap_fd: Option<std::fs::File>,
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
pub(super) const fn gpfifo_entry(push_buf_va: u64, length_dwords: u32) -> u64 {
    (push_buf_va & !3) | ((length_dwords as u64) << 42)
}

/// Volta+ RAMUSERD `GP_PUT` offset (bytes) — dword 35.
/// Present on all generations from Volta through Blackwell.
pub(super) const USERD_GP_PUT_OFFSET: usize = 35 * 4; // 0x8C

/// Volta-Hopper RAMUSERD `GP_GET` offset (bytes) — dword 34.
/// NOTE: Blackwell (clca6f) removed GP_GET from the USERD control struct.
/// The entire 0x00-0x8B range is "Ignored00" on Blackwell.
/// On Blackwell, completion must be tracked via semaphore release instead.
pub(super) const USERD_GP_GET_OFFSET: usize = 34 * 4; // 0x88

/// Default GPFIFO ring entries (each entry = 8 bytes, 512 entries = 4 KiB).
pub(super) const GPFIFO_ENTRIES: u32 = 512;

/// Default GPFIFO ring size in bytes.
pub(super) const GPFIFO_SIZE: u64 = GPFIFO_ENTRIES as u64 * 8;

/// USERD page size.
pub(super) const USERD_SIZE: u64 = 4096;

/// Page-align a size upward (4 KiB pages).
pub(super) const fn page_align(size: u64) -> u64 {
    (size + 0xFFF) & !0xFFF
}

/// Reinterpret a `&[u32]` as `&[u8]` for buffer upload.
pub(super) fn u32_slice_as_bytes(words: &[u32]) -> &[u8] {
    bytemuck::cast_slice(words)
}
