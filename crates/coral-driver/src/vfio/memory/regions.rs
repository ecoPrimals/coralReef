// SPDX-License-Identifier: AGPL-3.0-or-later

use std::time::Instant;

use crate::vfio::device::MappedBar;
use crate::vfio::dma::DmaBuffer;

use super::core::{Aperture, MemoryError, PathStatus};

/// A region of memory accessible from the CPU side.
///
/// Unifies DMA buffers (system memory), PRAMIN windows (VRAM), and
/// BAR0 MMIO (register space) behind a single interface. Each region
/// knows its aperture, supports read/write, and can flush CPU caches.
pub trait MemoryRegion {
    /// Where this region physically resides.
    fn aperture(&self) -> Aperture;

    /// Size of this region in bytes.
    fn size(&self) -> usize;

    /// Read a 32-bit value at the given byte offset within this region.
    fn read_u32(&self, offset: usize) -> Result<u32, MemoryError>;

    /// Write a 32-bit value at the given byte offset within this region.
    fn write_u32(&mut self, offset: usize, val: u32) -> Result<(), MemoryError>;

    /// Flush CPU caches for this region to ensure DMA coherence.
    /// Default is a no-op; overridden for DMA regions on non-coherent platforms.
    fn flush(&self) {}

    /// Probe this region with a sentinel write/readback test at the given offset.
    fn probe_sentinel(&mut self, offset: usize, sentinel: u32) -> PathStatus {
        let start = Instant::now();
        if self.write_u32(offset, sentinel).is_err() {
            return PathStatus::ErrorPattern { pattern: 0 };
        }
        self.flush();
        match self.read_u32(offset) {
            Ok(read) => {
                PathStatus::from_sentinel_test(sentinel, read, start.elapsed().as_micros() as u64)
            }
            Err(_) => PathStatus::ErrorPattern {
                pattern: 0xDEAD_DEAD,
            },
        }
    }
}

/// System memory region backed by a VFIO IOMMU-mapped DMA buffer.
///
/// Wraps [`DmaBuffer`] with the [`MemoryRegion`] trait. The GPU accesses
/// this region via IOVA through the IOMMU; the CPU accesses it directly.
pub struct DmaRegion {
    buf: DmaBuffer,
    coherent: bool,
}

impl DmaRegion {
    pub fn new(buf: DmaBuffer, coherent: bool) -> Self {
        Self { buf, coherent }
    }

    /// The underlying DMA buffer (for operations that need raw access).
    pub fn buffer(&self) -> &DmaBuffer {
        &self.buf
    }

    /// Mutable access to the underlying buffer.
    pub fn buffer_mut(&mut self) -> &mut DmaBuffer {
        &mut self.buf
    }

    /// IOVA of this region (device-visible address).
    pub fn iova(&self) -> u64 {
        self.buf.iova()
    }
}

impl MemoryRegion for DmaRegion {
    fn aperture(&self) -> Aperture {
        Aperture::SystemMemory {
            iova: self.buf.iova(),
            coherent: self.coherent,
        }
    }

    fn size(&self) -> usize {
        self.buf.size()
    }

    fn read_u32(&self, offset: usize) -> Result<u32, MemoryError> {
        let slice = self.buf.as_slice();
        if offset + 4 > slice.len() {
            return Err(MemoryError::OutOfBounds {
                offset,
                size: slice.len(),
            });
        }
        Ok(u32::from_le_bytes(
            slice[offset..offset + 4]
                .try_into()
                .expect("4-byte slice always fits [u8; 4]"),
        ))
    }

    fn write_u32(&mut self, offset: usize, val: u32) -> Result<(), MemoryError> {
        let slice = self.buf.as_mut_slice();
        if offset + 4 > slice.len() {
            return Err(MemoryError::OutOfBounds {
                offset,
                size: slice.len(),
            });
        }
        slice[offset..offset + 4].copy_from_slice(&val.to_le_bytes());
        Ok(())
    }

    fn flush(&self) {
        #[cfg(target_arch = "x86_64")]
        {
            crate::vfio::cache_ops::clflush_range(self.buf.as_slice());
            crate::vfio::cache_ops::memory_fence();
        }
    }
}

/// VRAM region accessed through the BAR0 PRAMIN window.
///
/// The PRAMIN window is a 64KB aperture at BAR0 + 0x700000 that maps to a
/// configurable VRAM offset via the BAR0_WINDOW register (0x1700). This
/// implementation provides RAII window management — it saves the BAR0_WINDOW
/// value on creation and restores it on drop.
///
/// The VRAM offset must fall within a single 64KB-aligned window. For regions
/// that span multiple windows, create multiple `PraminRegion` instances.
pub struct PraminRegion<'a> {
    bar0: &'a MappedBar,
    vram_base: u32,
    window_offset: usize,
    region_size: usize,
    saved_window: u32,
}

const PRAMIN_BASE: usize = 0x0070_0000;
const BAR0_WINDOW: usize = 0x0000_1700;
const PRAMIN_WINDOW_SIZE: usize = 0x1_0000; // 64KB

/// Compute 64KB PRAMIN window base and offset for a VRAM range (pure, no MMIO).
///
/// Returns `(window_aligned_base, offset_within_window)` or an error if the
/// region crosses a 64KB boundary.
#[must_use = "PRAMIN window placement must be checked before MMIO access"]
pub(crate) fn pramin_window_layout(
    vram_base: u32,
    size: usize,
) -> Result<(u32, usize), MemoryError> {
    let window_base = vram_base & !0xFFFF;
    let offset_in_window = (vram_base & 0xFFFF) as usize;

    if offset_in_window + size > PRAMIN_WINDOW_SIZE {
        return Err(MemoryError::NotAccessible {
            reason: format!(
                "PRAMIN region {vram_base:#x}+{size:#x} spans window boundary \
                 (window={window_base:#x}, offset={offset_in_window:#x})"
            ),
        });
    }

    Ok((window_base, offset_in_window))
}

impl<'a> PraminRegion<'a> {
    /// Create a PRAMIN region for the given VRAM address range.
    ///
    /// Saves the current BAR0_WINDOW, steers it to cover `vram_base`, and
    /// calculates the offset within the 64KB window. Restores on drop.
    ///
    /// # Errors
    ///
    /// Returns error if the requested region spans more than one 64KB window.
    pub fn new(bar0: &'a MappedBar, vram_base: u32, size: usize) -> Result<Self, MemoryError> {
        let (window_base, offset_in_window) = pramin_window_layout(vram_base, size)?;

        let saved_window = bar0.read_u32(BAR0_WINDOW).unwrap_or(0);
        let _ = bar0.write_u32(BAR0_WINDOW, window_base >> 16);

        Ok(Self {
            bar0,
            vram_base,
            window_offset: offset_in_window,
            region_size: size,
            saved_window,
        })
    }

    /// The VRAM base address this region covers.
    pub fn vram_base(&self) -> u32 {
        self.vram_base
    }

    /// Probe a range of VRAM offsets for accessibility.
    ///
    /// Writes and reads back a sentinel at each `stride`-byte offset,
    /// returning the status of each probe.
    pub fn probe_range(
        bar0: &MappedBar,
        start: u32,
        end: u32,
        stride: u32,
        sentinel: u32,
    ) -> Vec<(u32, PathStatus)> {
        let mut results = Vec::new();
        let mut addr = start;
        while addr < end {
            let status = if let Ok(mut region) = PraminRegion::new(bar0, addr, 4) {
                region.probe_sentinel(0, sentinel)
            } else {
                PathStatus::ErrorPattern { pattern: 0 }
            };
            results.push((addr, status));
            addr = addr.saturating_add(stride);
        }
        results
    }
}

impl Drop for PraminRegion<'_> {
    fn drop(&mut self) {
        let _ = self.bar0.write_u32(BAR0_WINDOW, self.saved_window);
    }
}

impl MemoryRegion for PraminRegion<'_> {
    fn aperture(&self) -> Aperture {
        Aperture::VideoMemory {
            vram_offset: u64::from(self.vram_base),
        }
    }

    fn size(&self) -> usize {
        self.region_size
    }

    fn read_u32(&self, offset: usize) -> Result<u32, MemoryError> {
        if offset + 4 > self.region_size {
            return Err(MemoryError::OutOfBounds {
                offset,
                size: self.region_size,
            });
        }
        self.bar0
            .read_u32(PRAMIN_BASE + self.window_offset + offset)
            .map_err(|e| MemoryError::IoError {
                detail: e.to_string(),
            })
    }

    fn write_u32(&mut self, offset: usize, val: u32) -> Result<(), MemoryError> {
        if offset + 4 > self.region_size {
            return Err(MemoryError::OutOfBounds {
                offset,
                size: self.region_size,
            });
        }
        self.bar0
            .write_u32(PRAMIN_BASE + self.window_offset + offset, val)
            .map_err(|e| MemoryError::IoError {
                detail: e.to_string(),
            })
    }
}

/// BAR0 register space region accessed via volatile MMIO.
///
/// Wraps a sub-range of [`MappedBar`] as a [`MemoryRegion`]. Unlike PRAMIN
/// (which accesses VRAM through a window), this accesses GPU registers directly.
pub struct MmioRegion<'a> {
    bar0: &'a MappedBar,
    base_offset: usize,
    region_size: usize,
}

impl<'a> MmioRegion<'a> {
    /// Create an MMIO region covering BAR0 offsets `base..base+size`.
    pub fn new(bar0: &'a MappedBar, base_offset: usize, size: usize) -> Self {
        Self {
            bar0,
            base_offset,
            region_size: size,
        }
    }
}

impl MemoryRegion for MmioRegion<'_> {
    fn aperture(&self) -> Aperture {
        Aperture::RegisterSpace {
            offset: self.base_offset,
        }
    }

    fn size(&self) -> usize {
        self.region_size
    }

    fn read_u32(&self, offset: usize) -> Result<u32, MemoryError> {
        if offset + 4 > self.region_size {
            return Err(MemoryError::OutOfBounds {
                offset,
                size: self.region_size,
            });
        }
        self.bar0
            .read_u32(self.base_offset + offset)
            .map_err(|e| MemoryError::IoError {
                detail: e.to_string(),
            })
    }

    fn write_u32(&mut self, offset: usize, val: u32) -> Result<(), MemoryError> {
        if offset + 4 > self.region_size {
            return Err(MemoryError::OutOfBounds {
                offset,
                size: self.region_size,
            });
        }
        self.bar0
            .write_u32(self.base_offset + offset, val)
            .map_err(|e| MemoryError::IoError {
                detail: e.to_string(),
            })
    }
}
