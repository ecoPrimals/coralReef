// SPDX-License-Identifier: AGPL-3.0-only
//! DMA buffer management for VFIO GPU dispatch.
//!
//! Provides page-aligned, mlock'd, IOMMU-mapped memory buffers for
//! zero-copy data transfer between host and GPU via VFIO.

use crate::error::DriverError;
use rustix::mm::{mlock, munlock};
use std::borrow::Cow;
use std::os::fd::{BorrowedFd, RawFd};

use super::ioctl;
use super::types::ioctls;
use super::types::{VfioDmaMap, VfioDmaUnmap};

const PAGE_SIZE: usize = 4096;

/// IOMMU-mapped DMA buffer for VFIO GPU operations.
///
/// Allocated page-aligned, mlock'd to prevent swapping, and mapped through the
/// IOMMU so the GPU can read/write via the assigned IOVA. Automatically
/// unmapped and freed on drop.
#[derive(Debug)]
pub struct DmaBuffer {
    vaddr: *mut u8,
    iova: u64,
    size: usize,
    container_fd: RawFd,
}

impl DmaBuffer {
    /// Allocate a new DMA buffer mapped for device access.
    ///
    /// `container_fd` must be an open VFIO container fd with an IOMMU attached.
    /// `size` is rounded up to page alignment internally.
    ///
    /// # Errors
    ///
    /// Returns error if size is zero, allocation fails, mlock fails, or IOMMU
    /// DMA mapping fails.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "struct sizes and page-aligned sizes always fit u32/u64"
    )]
    pub(crate) fn new(container_fd: RawFd, size: usize, iova: u64) -> Result<Self, DriverError> {
        if size == 0 {
            return Err(DriverError::MmapFailed(
                "DMA buffer size must be > 0".into(),
            ));
        }

        let aligned_size = size.div_ceil(PAGE_SIZE) * PAGE_SIZE;

        let layout = std::alloc::Layout::from_size_align(aligned_size, PAGE_SIZE).map_err(|e| {
            DriverError::MmapFailed(Cow::Owned(format!("Invalid DMA buffer layout: {e}")))
        })?;

        // SAFETY: Layout validated above (size>0, align=4096 power-of-two).
        // Returns null on OOM (checked below). Dealloc'd in Drop with same layout.
        let vaddr = unsafe { std::alloc::alloc_zeroed(layout) };
        if vaddr.is_null() {
            return Err(DriverError::MmapFailed(
                "Failed to allocate DMA buffer".into(),
            ));
        }

        // SAFETY: mlock prevents page-out, required for VFIO DMA correctness.
        // vaddr valid for aligned_size bytes from alloc above.
        if let Err(e) = unsafe { mlock(vaddr.cast(), aligned_size) } {
            // SAFETY: Cleanup — vaddr allocated above with same layout.
            unsafe { std::alloc::dealloc(vaddr, layout) };
            return Err(DriverError::MmapFailed(Cow::Owned(format!(
                "Failed to lock DMA memory: {e}"
            ))));
        }

        let dma_map_arg = VfioDmaMap {
            argsz: std::mem::size_of::<VfioDmaMap>() as u32,
            flags: ioctls::VFIO_DMA_MAP_FLAG_READ | ioctls::VFIO_DMA_MAP_FLAG_WRITE,
            vaddr: vaddr as u64,
            iova,
            size: aligned_size as u64,
        };

        tracing::debug!(
            vaddr = format_args!("{vaddr:p}"),
            iova = format_args!("{iova:#x}"),
            size = format_args!("{aligned_size:#x}"),
            "VFIO DMA map attempt"
        );

        // SAFETY: container_fd is valid from VFIO open; borrow_raw requires valid fd.
        let container_borrowed = unsafe { BorrowedFd::borrow_raw(container_fd) };
        if let Err(e) = ioctl::dma_map(container_borrowed, &dma_map_arg) {
            tracing::warn!("VFIO DMA map failed: {e}");
            // SAFETY: Cleanup — vaddr allocated and mlock'd above.
            unsafe {
                let _ = munlock(vaddr.cast(), aligned_size);
                std::alloc::dealloc(vaddr, layout);
            };
            return Err(e);
        }

        tracing::debug!(
            vaddr = format_args!("{vaddr:p}"),
            iova = format_args!("{iova:#x}"),
            size = format_args!("{aligned_size:#x}"),
            "VFIO DMA buffer created"
        );

        Ok(Self {
            vaddr,
            iova,
            size: aligned_size,
            container_fd,
        })
    }

    /// Immutable slice view of the buffer contents.
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: vaddr from alloc_zeroed in new() (null checked before use); valid for size
        // bytes; &self prevents concurrent mutation. DmaBuffer is only constructible via new().
        assert!(
            !self.vaddr.is_null(),
            "DmaBuffer vaddr is null (invalid state)"
        );
        assert!(self.size > 0, "DmaBuffer size is 0 (invalid state)");
        unsafe { std::slice::from_raw_parts(self.vaddr, self.size) }
    }

    /// Mutable slice view for writing data into the buffer.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: vaddr valid for size bytes; &mut self guarantees exclusive access.
        // Same invariants as as_slice.
        assert!(
            !self.vaddr.is_null(),
            "DmaBuffer vaddr is null (invalid state)"
        );
        assert!(self.size > 0, "DmaBuffer size is 0 (invalid state)");
        unsafe { std::slice::from_raw_parts_mut(self.vaddr, self.size) }
    }

    /// Device-visible I/O virtual address.
    #[must_use]
    pub const fn iova(&self) -> u64 {
        self.iova
    }

    /// Buffer size in bytes (page-aligned).
    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }

    /// Host virtual address pointer (for volatile MMIO writes referencing this buffer).
    #[must_use]
    pub const fn vaddr(&self) -> *const u8 {
        self.vaddr
    }
}

impl Drop for DmaBuffer {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "struct sizes always fit u32"
    )]
    fn drop(&mut self) {
        // SAFETY: munlock matches mlock from new(); must unlock before dealloc.
        unsafe {
            let _ = munlock(self.vaddr.cast(), self.size);
        };

        let dma_unmap = VfioDmaUnmap {
            argsz: std::mem::size_of::<VfioDmaUnmap>() as u32,
            flags: 0,
            iova: self.iova,
            size: self.size as u64,
        };

        // SAFETY: container_fd still valid — DmaBuffer is dropped before the
        // VfioDevice (field order in VfioDevice ensures this).
        let container_borrowed = unsafe { BorrowedFd::borrow_raw(self.container_fd) };
        let _ = ioctl::dma_unmap(container_borrowed, &dma_unmap);

        let layout = std::alloc::Layout::from_size_align(self.size, PAGE_SIZE)
            .expect("Layout valid: matches alloc in new()");
        // SAFETY: dealloc matches alloc_zeroed from new(); layout identical.
        unsafe { std::alloc::dealloc(self.vaddr, layout) };

        tracing::debug!(
            iova = format_args!("{:#x}", self.iova),
            "VFIO DMA buffer freed"
        );
    }
}

// SAFETY: DmaBuffer owns its allocation exclusively — no shared mutable state.
unsafe impl Send for DmaBuffer {}

// SAFETY: Reads via &self are safe from multiple threads; writes require &mut self.
unsafe impl Sync for DmaBuffer {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_size_zero_returns_error() {
        let result = DmaBuffer::new(-1, 0, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("size must be > 0"));
    }

    #[test]
    fn layout_alignment_4096() {
        let layout = std::alloc::Layout::from_size_align(4096, PAGE_SIZE);
        assert!(layout.is_ok());
        let layout = layout.unwrap();
        assert_eq!(layout.size(), 4096);
        assert_eq!(layout.align(), 4096);
    }

    #[test]
    fn layout_invalid_align_zero() {
        let layout = std::alloc::Layout::from_size_align(4096, 0);
        assert!(layout.is_err());
    }

    #[test]
    fn layout_invalid_align_non_power_of_two() {
        let layout = std::alloc::Layout::from_size_align(4096, 3000);
        assert!(layout.is_err());
    }

    #[test]
    fn alignment_math_1_byte() {
        let aligned = 1usize.div_ceil(PAGE_SIZE) * PAGE_SIZE;
        assert_eq!(aligned, 4096);
    }

    #[test]
    fn alignment_math_exact_page() {
        let aligned = 8192usize.div_ceil(PAGE_SIZE) * PAGE_SIZE;
        assert_eq!(aligned, 8192);
    }

    #[test]
    fn alignment_math_multiple_pages() {
        let aligned = 16_384usize.div_ceil(PAGE_SIZE) * PAGE_SIZE;
        assert_eq!(aligned, 16_384);
    }

    #[test]
    fn dma_map_argsz_layout() {
        assert!(
            std::mem::size_of::<VfioDmaMap>() >= 32,
            "VfioDmaMap kernel ABI expects at least 32 bytes"
        );
    }

    #[test]
    fn dma_unmap_argsz_layout() {
        assert!(
            std::mem::size_of::<VfioDmaUnmap>() >= 24,
            "VfioDmaUnmap kernel ABI expects at least 24 bytes"
        );
    }
}
