// SPDX-License-Identifier: AGPL-3.0-only
//! DMA buffer management for VFIO GPU dispatch.
//!
//! Provides page-aligned, mlock'd, IOMMU-mapped memory buffers for
//! zero-copy data transfer between host and GPU via VFIO.
//!
//! # Unsafe blocks — why they must remain
//!
//! - **alloc/dealloc**: VFIO DMA map requires page-aligned (4096-byte) virtual
//!   addresses. `Vec` and `Box` do not guarantee alignment; `alloc_zeroed` with
//!   `Layout::from_size_align(_, 4096)` is required.
//! - **mlock/munlock**: rustix exposes these as `unsafe` (raw pointer + length).
//!   No safe wrapper exists; invariants are documented at each call site.
//! - **Container fd**: Each buffer holds an [`Arc`] clone of the VFIO container
//!   [`OwnedFd`]; ioctls use [`AsFd::as_fd`] (no `borrow_raw`).

use crate::error::DriverError;
use rustix::mm::{mlock, munlock};
use std::borrow::Cow;
use std::os::fd::{AsFd, OwnedFd};
use std::sync::Arc;

use super::ioctl;
use super::types::ioctls;
use super::types::{VfioDmaMap, VfioDmaUnmap};

const PAGE_SIZE: usize = 4096;

/// IOMMU-mapped DMA buffer for VFIO GPU operations.
///
/// Allocated page-aligned, mlock'd to prevent swapping, and mapped through the
/// IOMMU so the GPU can read/write via the assigned IOVA. Automatically
/// unmapped and freed on drop.
pub struct DmaBuffer {
    vaddr: *mut u8,
    iova: u64,
    size: usize,
    /// Shared handle to the VFIO container fd (`VfioDevice` holds the primary `Arc`).
    container: Arc<OwnedFd>,
}

impl DmaBuffer {
    /// Allocate a new DMA buffer mapped for device access.
    ///
    /// `container` must be a shared handle to an open VFIO container fd with an
    /// IOMMU attached (same `Arc` as [`crate::vfio::device::VfioDevice::container_shared`]).
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
    pub(crate) fn new(
        container: Arc<OwnedFd>,
        size: usize,
        iova: u64,
    ) -> Result<Self, DriverError> {
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
        // Note: Vec<u8> cannot replace this — VFIO DMA map requires page-aligned
        // (4096) virtual addresses; Vec does not guarantee alignment.
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

        let container_borrowed = container.as_fd();
        if let Err(e) = ioctl::dma_map(container_borrowed, &dma_map_arg) {
            // EEXIST: stale mapping from a previous consumer on a shared container
            // (common with ember fd sharing). Unmap first, then retry.
            if e.to_string().contains("File exists") {
                tracing::info!(
                    iova = format_args!("{iova:#x}"),
                    "VFIO IOVA already mapped — unmapping stale entry and retrying"
                );
                let dma_unmap = VfioDmaUnmap {
                    argsz: std::mem::size_of::<VfioDmaUnmap>() as u32,
                    flags: 0,
                    iova,
                    size: aligned_size as u64,
                };
                let _ = ioctl::dma_unmap(container_borrowed, &dma_unmap);
                if let Err(e2) = ioctl::dma_map(container_borrowed, &dma_map_arg) {
                    tracing::warn!("VFIO DMA map retry failed: {e2}");
                    // SAFETY: Cleanup — vaddr was allocated via alloc_zeroed and mlock'd above.
                    unsafe {
                        let _ = munlock(vaddr.cast(), aligned_size);
                        std::alloc::dealloc(vaddr, layout);
                    };
                    return Err(e2);
                }
            } else {
                tracing::warn!("VFIO DMA map failed: {e}");
                // SAFETY: Cleanup — vaddr allocated and mlock'd above.
                unsafe {
                    let _ = munlock(vaddr.cast(), aligned_size);
                    std::alloc::dealloc(vaddr, layout);
                };
                return Err(e);
            }
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
            container,
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

        // `Arc` keeps the container fd open until the last buffer and `VfioDevice` drop.
        let container_borrowed = self.container.as_fd();
        let _ = ioctl::dma_unmap(container_borrowed, &dma_unmap);

        // SAFETY: self.size and PAGE_SIZE are identical to those used in new().
        // size is from aligned_size (div_ceil * PAGE_SIZE); PAGE_SIZE is 4096.
        // Layout::from_size_align(size, 4096) cannot fail for this combination.
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

// SAFETY: The raw pointer (`vaddr`) is obtained from a dedicated allocation, is only
// accessed through `&self`/`&mut self` (Rust borrow rules apply), and is freed in
// Drop. The shared container handle is thread-safe.
unsafe impl Send for DmaBuffer {}

// SAFETY: The raw pointer (`vaddr`) is obtained from a dedicated allocation, is only
// accessed through `&self`/`&mut self` (Rust borrow rules apply), and is freed in
// Drop. The shared container handle is thread-safe.
unsafe impl Sync for DmaBuffer {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_size_zero_returns_error() {
        let file = std::fs::File::open("/dev/null").expect("open /dev/null");
        let container = Arc::new(OwnedFd::from(file));
        let result = DmaBuffer::new(container, 0, 0);
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("expected size-zero error"),
        };
        assert!(err.to_string().contains("size must be > 0"));
    }

    #[test]
    fn layout_alignment_4096() {
        let layout = std::alloc::Layout::from_size_align(4096, PAGE_SIZE);
        assert!(layout.is_ok());
        let layout = layout.expect("4096-byte page-aligned layout is valid");
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
