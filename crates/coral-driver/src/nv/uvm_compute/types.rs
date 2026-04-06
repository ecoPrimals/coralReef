// SPDX-License-Identifier: AGPL-3.0-or-later
//! GPU generation, GPFIFO encoding, and per-buffer bookkeeping for UVM compute.

use crate::nv::uvm::{
    ADA_COMPUTE_A, AMPERE_CHANNEL_GPFIFO_A, AMPERE_COMPUTE_A, AMPERE_COMPUTE_B,
    BLACKWELL_CHANNEL_GPFIFO_B, BLACKWELL_COMPUTE_A, BLACKWELL_COMPUTE_B, HOPPER_COMPUTE_A,
    VOLTA_CHANNEL_GPFIFO_A, VOLTA_COMPUTE_A,
};

/// Flush one cache line so GPU DMA sees CPU writes (UVM mmap paths; mirrors `vfio::cache_ops`).
#[cfg(target_arch = "x86_64")]
#[inline]
pub(super) unsafe fn uvm_cache_line_flush(addr: *const u8) {
    // SAFETY: Caller documents `addr` points into a valid CPU mapping; `_mm_clflush`
    // only affects the cache line containing that address.
    unsafe { core::arch::x86_64::_mm_clflush(addr) }
}

#[cfg(not(target_arch = "x86_64"))]
#[inline]
pub(super) unsafe fn uvm_cache_line_flush(_addr: *const u8) {}

/// GPU generation derived from SM version, used for class selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GpuGen {
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
    pub(super) const fn from_sm(sm: u32) -> Self {
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

    pub(super) const fn channel_class(self) -> u32 {
        match self {
            Self::BlackwellA | Self::BlackwellB => BLACKWELL_CHANNEL_GPFIFO_B,
            Self::AmpereA | Self::AmpereB | Self::Ada | Self::Hopper => AMPERE_CHANNEL_GPFIFO_A,
            Self::Volta | Self::Turing => VOLTA_CHANNEL_GPFIFO_A,
        }
    }

    pub(super) const fn compute_class(self) -> u32 {
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
pub(super) const USERD_GP_PUT_OFFSET: usize = 35 * 4; // 0x8C

/// Volta+ RAMUSERD `GP_GET` offset (bytes) — dword 34.
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
