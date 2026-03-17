// SPDX-License-Identifier: AGPL-3.0-only
//! Safe wrappers for x86_64 cache coherence intrinsics.
//!
//! Used for DMA buffer coherence: on non-coherent platforms (e.g. AMD Zen 2 with
//! VFIO), the GPU may not snoop CPU cache. Flushing cache lines and issuing a
//! memory fence ensures the GPU sees the latest CPU writes.
//!
//! # Safety contract
//!
//! `cache_line_flush` and `clflush_range` require that the address points to
//! valid mapped memory. This is guaranteed by only calling from DMA/BAR0
//! contexts with valid mappings (DmaBuffer, MappedBar, etc.).

/// Flush a single cache line containing the given address.
///
/// # Safety
///
/// The address must point to valid mapped memory. Callers must ensure this;
/// in practice we only call from DMA/BAR0 contexts with valid mappings.
#[cfg(target_arch = "x86_64")]
#[inline]
pub unsafe fn cache_line_flush(addr: *const u8) {
    unsafe { core::arch::x86_64::_mm_clflush(addr) }
}

/// No-op on non-x86_64 (cache flush not needed for coherent platforms).
/// No-op on non-x86_64 (cache flush not needed for coherent platforms).
///
/// # Safety
///
/// Same contract as x86_64 variant — address must be valid mapped memory.
#[cfg(not(target_arch = "x86_64"))]
#[inline]
#[allow(dead_code)]
pub unsafe fn cache_line_flush(_addr: *const u8) {}

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
#[allow(dead_code)]
pub fn memory_fence() {}

/// Flush all cache lines covering the range [ptr, ptr + len).
///
/// Rounds to 64-byte cache line boundaries. Call `memory_fence()` after
/// if you need a full barrier (typically yes for DMA coherence).
///
/// # Safety contract upheld internally
///
/// The range must be within valid mapped memory. Callers must ensure this;
/// we only call from DMA buffer contexts.
#[cfg(target_arch = "x86_64")]
#[inline]
pub fn clflush_range(ptr: *const u8, len: usize) {
    let mut addr = ptr as usize & !63;
    let end = (ptr as usize + len + 63) & !63;
    while addr < end {
        // SAFETY: addr is within the valid mapped range [ptr, ptr+len) rounded
        // to cache-line boundaries; the caller guarantees the mapping is valid.
        unsafe { cache_line_flush(addr as *const u8) };
        addr += 64;
    }
}

/// No-op on non-x86_64.
#[cfg(not(target_arch = "x86_64"))]
#[inline]
#[allow(dead_code)]
pub fn clflush_range(_ptr: *const u8, _len: usize) {}
