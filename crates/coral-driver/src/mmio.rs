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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volatile_ptr_construction_from_aligned_u32() {
        let mut value: u32 = 0xDEAD_BEEF;
        let ptr = unsafe { VolatilePtr::new(&mut value as *mut u32) };
        assert_eq!(ptr.read(), 0xDEAD_BEEF);
    }

    #[test]
    fn volatile_ptr_read_write_roundtrip() {
        let mut value: u32 = 0;
        let ptr = unsafe { VolatilePtr::new(&mut value as *mut u32) };
        ptr.write(0x1234_5678);
        assert_eq!(ptr.read(), 0x1234_5678);
        assert_eq!(value, 0x1234_5678);
    }

    #[test]
    fn volatile_ptr_multiple_writes_persist() {
        let mut value: u32 = 0;
        let ptr = unsafe { VolatilePtr::new(&mut value as *mut u32) };
        ptr.write(1);
        ptr.write(2);
        ptr.write(3);
        assert_eq!(ptr.read(), 3);
    }

    #[test]
    fn volatile_ptr_clone_copy_independent_access() {
        let mut value: u32 = 0x42;
        let ptr1 = unsafe { VolatilePtr::new(&mut value as *mut u32) };
        let ptr2 = ptr1;
        ptr1.write(0x100);
        assert_eq!(ptr2.read(), 0x100);
    }
}
