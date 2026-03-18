// SPDX-License-Identifier: AGPL-3.0-only
//! Safe wrapper for volatile MMIO register access.
//!
//! Encapsulates `ptr::read_volatile` / `ptr::write_volatile` behind a type that
//! takes ownership of the alignment and validity invariants at construction.
//! Call sites that construct a `VolatilePtr` after bounds/alignment checks
//! can then use `read()` and `write()` without additional `unsafe`.

/// A pointer to volatile MMIO memory, validated for alignment and bounds at construction.
///
/// Construction is `unsafe`; once created, `read()` and `write()` are safe.
#[derive(Clone, Copy)]
pub(crate) struct VolatilePtr<T> {
    ptr: *mut T,
}

impl<T: Copy> VolatilePtr<T> {
    /// Create a `VolatilePtr` from a raw pointer.
    ///
    /// # Safety
    ///
    /// `ptr` must be valid for reads/writes of `T`, properly aligned, and point
    /// to volatile MMIO (e.g. mmap'd BAR0/BAR region). The caller must ensure
    /// bounds were checked before computing the pointer.
    #[inline]
    pub(crate) unsafe fn new(ptr: *mut T) -> Self {
        Self { ptr }
    }

    /// Volatile read. Safe because validity/alignment were established at construction.
    #[inline]
    pub(crate) fn read(&self) -> T {
        // SAFETY: ptr was validated in new(); volatile required for MMIO.
        unsafe { std::ptr::read_volatile(self.ptr) }
    }

    /// Volatile write. Safe because validity/alignment were established at construction.
    #[inline]
    pub(crate) fn write(&self, value: T) {
        // SAFETY: ptr was validated in new(); volatile required for MMIO.
        unsafe { std::ptr::write_volatile(self.ptr, value) }
    }
}
