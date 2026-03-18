// SPDX-License-Identifier: AGPL-3.0-only
//! Safe wrappers for x86_64 cache coherence intrinsics.
//!
//! Used for DMA buffer coherence: on non-coherent platforms (e.g. AMD Zen 2 with
//! VFIO), the GPU may not snoop CPU cache. Flushing cache lines and issuing a
//! memory fence ensures the GPU sees the latest CPU writes.
//!
//! # Safety contract
//!
//! `cache_line_flush` requires that the address points to valid mapped memory.
//! `clflush_range` takes `&[u8]` and thus guarantees valid memory; the slice
//! must be backed by DMA-mapped memory (DmaBuffer, MappedBar, etc.).

/// Flush a single cache line containing the given address.
///
/// Prefer [`clflush_range`] which takes `&[u8]` and is safe.
///
/// # Safety
///
/// The address must point to valid mapped memory. Callers must ensure this;
/// in practice we only call from DMA/BAR0 contexts with valid mappings.
#[cfg(target_arch = "x86_64")]
#[inline]
pub(crate) unsafe fn cache_line_flush(addr: *const u8) {
    unsafe { core::arch::x86_64::_mm_clflush(addr) }
}

/// No-op on non-x86_64 (cache flush not needed for coherent platforms).
///
/// # Safety
///
/// Same contract as x86_64 variant — address must be valid mapped memory.
#[cfg(not(target_arch = "x86_64"))]
#[inline]
#[expect(dead_code, reason = "platform-specific stub; not used on non-x86_64")]
pub(crate) unsafe fn cache_line_flush(_addr: *const u8) {}

/// Full memory fence (store + load barrier).
///
/// Ensures all preceding stores are globally visible before any subsequent
/// loads. Required after cache flushes for DMA coherence.
#[cfg(target_arch = "x86_64")]
#[inline]
pub fn memory_fence() {
    unsafe { core::arch::x86_64::_mm_mfence() }
}

/// No-op on non-x86_64.
#[cfg(not(target_arch = "x86_64"))]
#[inline]
#[expect(dead_code, reason = "platform-specific stub; not used on non-x86_64")]
pub fn memory_fence() {}

/// Flush all cache lines covering the given slice.
///
/// Rounds to 64-byte cache line boundaries. Call `memory_fence()` after
/// if you need a full barrier (typically yes for DMA coherence).
///
/// Takes `&[u8]` to guarantee the range is valid; callers must ensure the
/// slice is backed by DMA-mapped memory (DmaBuffer, MappedBar, etc.).
#[cfg(target_arch = "x86_64")]
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

/// No-op on non-x86_64.
#[cfg(not(target_arch = "x86_64"))]
#[inline]
#[expect(dead_code, reason = "platform-specific stub; not used on non-x86_64")]
pub fn clflush_range(_slice: &[u8]) {}
