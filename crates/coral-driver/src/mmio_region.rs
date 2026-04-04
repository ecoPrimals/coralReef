// SPDX-License-Identifier: AGPL-3.0-only
//! RAII wrapper for mmap-backed MMIO with bounds-checked volatile access.
//!
//! Volatile `u32` loads/stores for mapped BAR/MMIO are implemented only in this
//! module so [`MmioRegion::read_u32`] and [`MmioRegion::write_u32`] stay the safe API.

use std::borrow::Cow;
use std::ptr::NonNull;

use crate::error::DriverError;

/// Owns a byte range of MMIO address space and releases it on drop (`munmap` for
/// kernel mappings, or heap deallocation for test-only heap backings).
///
/// This type does **not** implement [`Send`] or [`Sync`]. Wrappers that prove
/// additional invariants (for example VFIO-mapped BARs) may provide those impls.
pub(crate) struct MmioRegion {
    ptr: NonNull<u8>,
    len: usize,
    /// Tag for mmap vs test heap; only the test heap case is inspected in [`Drop`].
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "test-only heap backing for mock MMIO; non-test builds use Mmap only"
        )
    )]
    backing: Backing,
}

enum Backing {
    /// Region was obtained via `mmap`; [`Drop`] calls `munmap`.
    Mmap,
    /// Test-only: `ptr` points into this allocation; do not `munmap`.
    #[cfg(test)]
    Heap(
        #[expect(
            dead_code,
            reason = "owns heap bytes; only dropped, not read via field access"
        )]
        Box<[u8]>,
    ),
}

impl MmioRegion {
    /// Take ownership of an `mmap` result; the region is unmapped on [`Drop`].
    ///
    /// # Safety
    ///
    /// - `ptr` must be non-null and reference exactly `len` bytes that were
    ///   mapped with `mmap` (or equivalent) in this process.
    /// - The mapping must not be unmapped elsewhere before this value is dropped.
    /// - `len` must match the length passed to `mmap`.
    #[must_use]
    pub(crate) unsafe fn new(ptr: *mut u8, len: usize) -> Self {
        let ptr = NonNull::new(ptr).expect("MmioRegion::new: mmap pointer must be non-null");
        Self {
            ptr,
            len,
            backing: Backing::Mmap,
        }
    }

    /// Byte length of the mapped region.
    #[must_use]
    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    /// Raw base pointer (for legacy callers that perform their own arithmetic).
    #[must_use]
    pub(crate) const fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Volatile 32-bit read at `offset` bytes from the region base.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::MmapFailed`] if `offset + 4` exceeds the region.
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "Callers that require aligned MMIO (e.g. BAR) validate alignment separately"
    )]
    pub(crate) fn read_u32(&self, offset: usize) -> Result<u32, DriverError> {
        if offset.checked_add(4).is_none_or(|end| end > self.len) {
            return Err(DriverError::MmapFailed(Cow::Owned(format!(
                "MMIO read: offset {offset:#x} + 4 out of range (size {:#x})",
                self.len
            ))));
        }
        // SAFETY: `offset` + 4 is within `0..len`, so `ptr.add(offset)` stays inside the mapping.
        // `ptr` was valid MMIO for this region at construction.
        Ok(unsafe { std::ptr::read_volatile(self.ptr.as_ptr().add(offset).cast::<u32>()) })
    }

    /// Volatile 32-bit write at `offset` bytes from the region base.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::MmapFailed`] if `offset + 4` exceeds the region.
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "Callers that require aligned MMIO (e.g. BAR) validate alignment separately"
    )]
    pub(crate) fn write_u32(&self, offset: usize, value: u32) -> Result<(), DriverError> {
        if offset.checked_add(4).is_none_or(|end| end > self.len) {
            return Err(DriverError::MmapFailed(Cow::Owned(format!(
                "MMIO write: offset {offset:#x} + 4 out of range (size {:#x})",
                self.len
            ))));
        }
        // SAFETY: `offset` + 4 is within `0..len`, so `ptr.add(offset)` stays inside the mapping.
        // `ptr` was valid MMIO for this region at construction.
        unsafe {
            std::ptr::write_volatile(self.ptr.as_ptr().add(offset).cast::<u32>(), value);
        }
        Ok(())
    }

    /// Heap-backed region for unit tests (no `munmap`; frees the buffer on drop).
    #[cfg(test)]
    pub(crate) fn from_heap_slice_for_test(mut backing: Box<[u8]>) -> Self {
        assert!(
            !backing.is_empty(),
            "from_heap_slice_for_test: empty slice not supported"
        );
        let len = backing.len();
        let ptr = NonNull::new(backing.as_mut_ptr()).expect("non-empty slice has non-null ptr");
        Self {
            ptr,
            len,
            backing: Backing::Heap(backing),
        }
    }
}

impl Drop for MmioRegion {
    fn drop(&mut self) {
        #[cfg(test)]
        if matches!(&self.backing, Backing::Heap(_)) {
            // Heap memory is freed when `backing` drops; do not munmap.
            return;
        }
        // SAFETY: `Backing::Mmap` is only constructed via `new`, whose safety contract
        // requires that `ptr`/`len` came from `mmap` and were not freed elsewhere.
        unsafe {
            let _ = rustix::mm::munmap(self.ptr.as_ptr().cast::<std::ffi::c_void>(), self.len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heap_roundtrip_read_write() {
        let data = vec![0u8; 64].into_boxed_slice();
        let region = MmioRegion::from_heap_slice_for_test(data);
        region.write_u32(0, 0x1122_3344).expect("write");
        assert_eq!(region.read_u32(0).expect("read"), 0x1122_3344);
    }

    #[test]
    fn heap_offset_read_write() {
        let data = vec![0u8; 128].into_boxed_slice();
        let region = MmioRegion::from_heap_slice_for_test(data);
        region.write_u32(16, 0xAAAA_BBBB).expect("write");
        assert_eq!(region.read_u32(16).expect("read"), 0xAAAA_BBBB);
    }

    #[test]
    fn read_oob_returns_error() {
        let data = vec![0u8; 8].into_boxed_slice();
        let region = MmioRegion::from_heap_slice_for_test(data);
        assert!(region.read_u32(8).is_err());
        assert!(region.read_u32(5).is_err());
    }

    #[test]
    fn write_oob_returns_error() {
        let data = vec![0u8; 8].into_boxed_slice();
        let region = MmioRegion::from_heap_slice_for_test(data);
        assert!(region.write_u32(8, 0).is_err());
    }

    #[test]
    fn len_matches_backing() {
        let data = vec![0u8; 256].into_boxed_slice();
        let region = MmioRegion::from_heap_slice_for_test(data);
        assert_eq!(region.len(), 256);
    }
}
