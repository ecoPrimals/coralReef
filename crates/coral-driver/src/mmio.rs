// SPDX-License-Identifier: AGPL-3.0-only
//! Safe wrapper for volatile MMIO register access.
//!
//! Encapsulates `ptr::read_volatile` / `ptr::write_volatile` behind a type that
//! takes ownership of the alignment and validity invariants at construction.
//! Stack-local tests can use [`VolatilePtr::from_mut`]. MMIO call sites that
//! hold a validated raw pointer use [`VolatilePtr::new`] after bounds/alignment
//! checks, then call `read()` / `write()` without additional `unsafe`.

/// A pointer to volatile MMIO memory, validated for alignment and bounds at construction.
///
/// Construction is `unsafe`; once created, `read()` and `write()` are safe.
#[derive(Clone, Copy)]
pub struct VolatilePtr<T> {
    ptr: *mut T,
}

impl<T: Copy> VolatilePtr<T> {
    /// Create a `VolatilePtr` from a unique mutable reference (stack tests, etc.).
    ///
    /// For MMIO from mmap'd raw pointers, use [`Self::new`].
    #[cfg(test)]
    #[inline]
    pub fn from_mut(r: &mut T) -> Self {
        Self {
            ptr: std::ptr::from_mut(r),
        }
    }

    /// Create a `VolatilePtr` from a raw pointer.
    ///
    /// # Safety
    ///
    /// `ptr` must be valid for reads/writes of `T`, properly aligned, and point
    /// to volatile MMIO (e.g. mmap'd BAR0/BAR region). The caller must ensure
    /// bounds were checked before computing the pointer.
    #[inline]
    pub unsafe fn new(ptr: *mut T) -> Self {
        Self { ptr }
    }

    /// Volatile read. Safe because validity/alignment were established at construction.
    #[inline]
    pub fn read(&self) -> T {
        // SAFETY: ptr was validated in new(); volatile required for MMIO.
        unsafe { std::ptr::read_volatile(self.ptr) }
    }

    /// Volatile write. Safe because validity/alignment were established at construction.
    #[inline]
    pub fn write(&self, value: T) {
        // SAFETY: ptr was validated in new(); volatile required for MMIO.
        unsafe { std::ptr::write_volatile(self.ptr, value) }
    }
}

/// Typed MMIO register map — bounds-checked volatile access over a contiguous
/// memory-mapped region (BAR0, sysfs resource0, etc.).
///
/// All unsafe volatile pointer arithmetic is confined to this struct's methods.
/// Callers construct a `RegisterMap` once (the only unsafe step), then perform
/// reads and writes through safe APIs.
///
/// This eliminates the duplicate bounds-check + `VolatilePtr::new` pattern across
/// `MappedBar`, `SysfsBar0`, `Bar0Access`, `NouveauOracleBar0`, and `Bar0Capture`.
pub struct RegisterMap {
    base: *mut u8,
    size: usize,
}

impl RegisterMap {
    /// Create a `RegisterMap` over a memory-mapped region.
    ///
    /// # Safety
    ///
    /// `base` must point to a valid mmap'd region of at least `size` bytes.
    /// The region must remain mapped for the lifetime of this `RegisterMap`.
    /// The caller is responsible for unmapping (typically via `Drop` on the owning struct).
    #[inline]
    pub unsafe fn new(base: *mut u8, size: usize) -> Self {
        Self { base, size }
    }

    /// Size of the mapped region in bytes.
    #[inline]
    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }

    /// Read a 32-bit register at the given byte offset.
    ///
    /// Returns `None` if offset is out of bounds or not 4-byte aligned.
    #[inline]
    #[must_use]
    pub fn read_u32(&self, offset: usize) -> Option<u32> {
        if !offset.is_multiple_of(4) || offset + 4 > self.size {
            return None;
        }
        // SAFETY: base is valid mmap; offset is bounds-checked and aligned.
        let vol = unsafe { VolatilePtr::new(self.base.add(offset).cast::<u32>()) };
        Some(vol.read())
    }

    /// Write a 32-bit register at the given byte offset.
    ///
    /// Returns `false` if offset is out of bounds or not 4-byte aligned.
    #[inline]
    pub fn write_u32(&self, offset: usize, value: u32) -> bool {
        if !offset.is_multiple_of(4) || offset + 4 > self.size {
            return false;
        }
        // SAFETY: base is valid mmap; offset is bounds-checked and aligned.
        let vol = unsafe { VolatilePtr::new(self.base.add(offset).cast::<u32>()) };
        vol.write(value);
        true
    }

    /// Read a 32-bit register, returning 0xDEAD_DEAD for out-of-bounds access.
    ///
    /// Useful in diagnostic contexts where sentinel values are acceptable.
    #[inline]
    #[must_use]
    pub fn read_u32_or_dead(&self, offset: usize) -> u32 {
        self.read_u32(offset).unwrap_or(0xDEAD_DEAD)
    }

    /// Base pointer (for callers that need to construct sub-regions).
    #[inline]
    #[must_use]
    pub const fn base_ptr(&self) -> *mut u8 {
        self.base
    }
}

// SAFETY: RegisterMap holds a raw pointer to mmap'd memory. Access is through
// volatile reads/writes which are inherently atomic for 32-bit aligned access
// on x86_64. The mmap lifetime is managed by the owning struct.
unsafe impl Send for RegisterMap {}
unsafe impl Sync for RegisterMap {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volatile_ptr_construction_from_aligned_u32() {
        let mut value: u32 = 0xDEAD_BEEF;
        let ptr = VolatilePtr::from_mut(&mut value);
        assert_eq!(ptr.read(), 0xDEAD_BEEF);
    }

    #[test]
    fn volatile_ptr_read_write_roundtrip() {
        let mut value: u32 = 0;
        let ptr = VolatilePtr::from_mut(&mut value);
        ptr.write(0x1234_5678);
        assert_eq!(ptr.read(), 0x1234_5678);
        assert_eq!(value, 0x1234_5678);
    }

    #[test]
    fn volatile_ptr_multiple_writes_persist() {
        let mut value: u32 = 0;
        let ptr = VolatilePtr::from_mut(&mut value);
        ptr.write(1);
        ptr.write(2);
        ptr.write(3);
        assert_eq!(ptr.read(), 3);
    }

    #[test]
    fn volatile_ptr_clone_copy_independent_access() {
        let mut value: u32 = 0x42;
        let ptr1 = VolatilePtr::from_mut(&mut value);
        let ptr2 = ptr1;
        ptr1.write(0x100);
        assert_eq!(ptr2.read(), 0x100);
    }
}
