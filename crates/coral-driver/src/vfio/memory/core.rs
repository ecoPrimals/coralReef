// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt;

/// Where a memory region physically resides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Aperture {
    /// Host system memory, accessible via IOMMU DMA.
    SystemMemory { iova: u64, coherent: bool },
    /// GPU VRAM, accessible via PRAMIN or BAR1/BAR2.
    VideoMemory { vram_offset: u64 },
    /// GPU register space (BAR0 MMIO).
    RegisterSpace { offset: usize },
}

impl fmt::Display for Aperture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SystemMemory { iova, coherent } => {
                write!(f, "SysMem(iova={iova:#x}, coh={coherent})")
            }
            Self::VideoMemory { vram_offset } => write!(f, "VRAM({vram_offset:#x})"),
            Self::RegisterSpace { offset } => write!(f, "MMIO({offset:#x})"),
        }
    }
}

/// Result of probing a memory path via sentinel write/readback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathStatus {
    /// Write/read sentinel matched — path is working.
    Working { latency_us: u64 },
    /// Write succeeded but read returned different value.
    Corrupted { wrote: u32, read: u32 },
    /// Read returned a known GPU error pattern.
    ErrorPattern { pattern: u32 },
    /// Path not yet tested.
    Untested,
}

impl PathStatus {
    /// True if the path is confirmed working.
    pub fn is_working(&self) -> bool {
        matches!(self, Self::Working { .. })
    }

    /// True if the read returned a GPU error pattern (0xBAD0ACxx, 0xFFFFFFFF).
    pub fn is_error_pattern(&self) -> bool {
        matches!(self, Self::ErrorPattern { .. })
    }

    /// Classify a readback value against the sentinel that was written.
    pub fn from_sentinel_test(wrote: u32, read: u32, elapsed_us: u64) -> Self {
        if read == wrote {
            Self::Working {
                latency_us: elapsed_us,
            }
        } else if read == 0xFFFF_FFFF || (read >> 16) == 0xBAD0 {
            Self::ErrorPattern { pattern: read }
        } else {
            Self::Corrupted { wrote, read }
        }
    }
}

/// Error from a memory region operation.
#[derive(Debug, Clone)]
pub enum MemoryError {
    OutOfBounds { offset: usize, size: usize },
    NotAccessible { reason: String },
    IoError { detail: String },
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds { offset, size } => {
                write!(f, "offset {offset:#x} out of bounds (size {size:#x})")
            }
            Self::NotAccessible { reason } => write!(f, "not accessible: {reason}"),
            Self::IoError { detail } => write!(f, "I/O error: {detail}"),
        }
    }
}

impl std::error::Error for MemoryError {}
