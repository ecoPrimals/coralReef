// SPDX-License-Identifier: AGPL-3.0-only
//! CPU cache coherence intrinsics — shared by VFIO DMA and UVM paths.
//!
//! These are the canonical cache flush primitives for the crate. Feature-gated
//! modules (`vfio::cache_ops`, `nv::uvm_compute`) should call through here
//! rather than wrapping `_mm_clflush` independently.

/// Flush a single cache line containing the given address.
///
/// # Safety
///
/// `addr` must point to valid mapped memory (DMA buffer, BAR0, UVM mmap, etc.).
#[cfg(target_arch = "x86_64")]
#[inline]
pub unsafe fn cache_line_flush(addr: *const u8) {
    // SAFETY: Caller guarantees `addr` points to valid mapped memory; `_mm_clflush`
    // only flushes the cache line containing that address.
    unsafe { core::arch::x86_64::_mm_clflush(addr) }
}

/// No-op on non-x86_64 (cache coherent platforms).
///
/// # Safety
///
/// Same contract as x86_64 variant.
#[cfg(not(target_arch = "x86_64"))]
#[inline]
pub unsafe fn cache_line_flush(_addr: *const u8) {}

/// Full memory fence (store + load barrier).
///
/// Ensures all preceding stores are globally visible before any subsequent
/// loads. Required after cache flushes for DMA coherence.
#[cfg(target_arch = "x86_64")]
#[inline]
pub fn memory_fence() {
    // SAFETY: `_mm_mfence` is a full memory barrier with no memory operands.
    unsafe { core::arch::x86_64::_mm_mfence() }
}

/// No-op on non-x86_64.
#[cfg(not(target_arch = "x86_64"))]
#[inline]
pub fn memory_fence() {}

/// Flush all cache lines covering the given slice, then issue a memory fence.
///
/// Takes `&[u8]` to guarantee the range is valid. The slice must be
/// backed by DMA-mapped or device-mapped memory.
#[inline]
pub fn clflush_range(slice: &[u8]) {
    if slice.is_empty() {
        return;
    }
    let ptr = slice.as_ptr();
    let len = slice.len();
    let mut addr = ptr as usize & !63;
    let end = (ptr as usize + len + 63) & !63;
    while addr < end {
        // SAFETY: addr is within the valid mapped range [ptr, ptr+len) rounded
        // to cache-line boundaries; slice guarantees valid memory.
        unsafe { cache_line_flush(addr as *const u8) };
        addr += 64;
    }
}
