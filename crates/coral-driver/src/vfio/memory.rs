// SPDX-License-Identifier: AGPL-3.0-only
#![allow(missing_docs)]
//! Unified memory abstraction for GPU/CPU bidirectional topology.
//!
//! Models GPU and CPU memory as a graph of regions connected by access paths.
//! Every read/write is both an operation and an observation — writing a sentinel
//! from one side and reading from the other reveals the GPU's internal state.
//!
//! Three implementations unify the previously separate memory access patterns:
//! - [`DmaRegion`] — system memory (CPU alloc, IOMMU-mapped, GPU reads via MMU)
//! - [`PraminRegion`] — VRAM via the 64KB BAR0 PRAMIN window
//! - [`MmioRegion`] — BAR0 register space (volatile MMIO)

use std::fmt;
use std::time::Instant;

use super::device::MappedBar;
use super::dma::DmaBuffer;

// ─── Core Types ─────────────────────────────────────────────────────────────

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

// ─── MemoryRegion Trait ─────────────────────────────────────────────────────

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

// ─── DmaRegion ──────────────────────────────────────────────────────────────

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
            slice[offset..offset + 4].try_into().unwrap(),
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
            super::cache_ops::clflush_range(self.buf.as_slice());
            super::cache_ops::memory_fence();
        }
    }
}

// ─── PraminRegion ───────────────────────────────────────────────────────────

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

// ─── MmioRegion ─────────────────────────────────────────────────────────────

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

// ─── Access Path Types ──────────────────────────────────────────────────────

/// Method by which a memory path operates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathMethod {
    /// BAR0 PRAMIN window → VRAM.
    Pramin,
    /// IOMMU DMA → system memory (coherent).
    DmaCoherent,
    /// IOMMU DMA → system memory (non-coherent).
    DmaNonCoherent,
    /// BAR1 aperture → VRAM (requires page tables).
    Bar1,
    /// BAR2 aperture → VRAM (requires page tables).
    Bar2,
    /// GPU MMU identity map: GPU VA = IOVA.
    GpuMmuIdentity,
    /// PBDMA reads RAMFC from VRAM (physical addressing, no MMU).
    PbdmaRamfc,
    /// PBDMA fetches GPFIFO entries through GPU MMU.
    PbdmaGpfifo,
}

impl fmt::Display for PathMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pramin => write!(f, "PRAMIN"),
            Self::DmaCoherent => write!(f, "DMA_COH"),
            Self::DmaNonCoherent => write!(f, "DMA_NCOH"),
            Self::Bar1 => write!(f, "BAR1"),
            Self::Bar2 => write!(f, "BAR2"),
            Self::GpuMmuIdentity => write!(f, "GPU_MMU_ID"),
            Self::PbdmaRamfc => write!(f, "PBDMA_RAMFC"),
            Self::PbdmaGpfifo => write!(f, "PBDMA_GPFIFO"),
        }
    }
}

/// A discovered access path between CPU and a memory location.
#[derive(Debug, Clone)]
pub struct AccessPath {
    /// Source of the access ("cpu", "pbdma", "gpu_mmu").
    pub from: &'static str,
    /// Destination aperture.
    pub to: Aperture,
    /// Transport mechanism.
    pub method: PathMethod,
    /// Result of probing this path.
    pub status: PathStatus,
    /// What must be configured before this path works.
    pub prerequisites: Vec<&'static str>,
}

// ─── Memory Topology ────────────────────────────────────────────────────────

/// Complete memory topology — all discovered regions and their access paths.
///
/// This is the output of a systematic memory probe. Higher interpreter layers
/// use it to decide which memory placement strategies are available.
#[derive(Debug, Clone)]
pub struct MemoryTopology {
    /// PRAMIN can access VRAM (at least one offset returned valid data).
    pub vram_accessible: bool,
    /// Highest VRAM address that returned valid data via PRAMIN probe.
    pub vram_size_probed: u64,
    /// DMA buffer allocation + IOMMU mapping works.
    pub sysmem_dma_ok: bool,
    /// BAR2 block is configured (page table setup done).
    pub bar2_configured: bool,
    /// All discovered access paths with their probe status.
    pub paths: Vec<AccessPath>,
    /// Raw evidence collected during probing (register name → value).
    pub evidence: Vec<(String, u32)>,
}

impl MemoryTopology {
    /// Find all working paths using a specific method.
    pub fn working_paths(&self, method: PathMethod) -> Vec<&AccessPath> {
        self.paths
            .iter()
            .filter(|p| p.method == method && p.status.is_working())
            .collect()
    }

    /// True if any PRAMIN path is working (CPU can read/write VRAM).
    pub fn pramin_works(&self) -> bool {
        !self.working_paths(PathMethod::Pramin).is_empty()
    }

    /// True if any DMA path is working (GPU can access system memory).
    pub fn dma_works(&self) -> bool {
        !self.working_paths(PathMethod::DmaCoherent).is_empty()
            || !self.working_paths(PathMethod::DmaNonCoherent).is_empty()
    }

    /// Print a human-readable summary to stderr.
    pub fn print_summary(&self) {
        eprintln!("╠══ Memory Topology ═══════════════════════════════╣");
        eprintln!(
            "║ VRAM: accessible={} probed_size={:#x}",
            self.vram_accessible, self.vram_size_probed
        );
        eprintln!(
            "║ SysMem: dma_ok={}  BAR2: configured={}",
            self.sysmem_dma_ok, self.bar2_configured
        );
        for path in &self.paths {
            let status_str = match &path.status {
                PathStatus::Working { latency_us } => format!("OK ({latency_us}us)"),
                PathStatus::Corrupted { wrote, read } => {
                    format!("CORRUPT (wrote={wrote:#x} read={read:#x})")
                }
                PathStatus::ErrorPattern { pattern } => format!("ERROR ({pattern:#010x})"),
                PathStatus::Untested => "UNTESTED".to_string(),
            };
            eprintln!(
                "║   {} → {} via {}: {}",
                path.from, path.to, path.method, status_str
            );
        }
    }
}

// ─── Memory Delta (for differential probing) ────────────────────────────────

/// Snapshot of memory accessibility change caused by a register write.
///
/// Used by the FB init investigation: write a register, re-probe memory,
/// and record what changed. Each delta that gains paths becomes an
/// `InitStep::RegisterWrite` in an ecosystem init recipe.
#[derive(Debug, Clone)]
pub struct MemoryDelta {
    /// The register write that was applied (offset, value).
    pub register_write: (usize, u32),
    /// Memory topology before the write.
    pub before: MemoryTopology,
    /// Memory topology after the write.
    pub after: MemoryTopology,
    /// Paths that became working (were not working before).
    pub paths_gained: Vec<AccessPath>,
    /// Paths that stopped working (were working before).
    pub paths_lost: Vec<AccessPath>,
}

impl MemoryDelta {
    /// Compute the delta between two topologies after a register write.
    pub fn compute(
        register_write: (usize, u32),
        before: MemoryTopology,
        after: MemoryTopology,
    ) -> Self {
        let mut paths_gained = Vec::new();
        let mut paths_lost = Vec::new();

        for after_path in &after.paths {
            if after_path.status.is_working() {
                let was_working = before.paths.iter().any(|bp| {
                    bp.method == after_path.method
                        && bp.to == after_path.to
                        && bp.status.is_working()
                });
                if !was_working {
                    paths_gained.push(after_path.clone());
                }
            }
        }

        for before_path in &before.paths {
            if before_path.status.is_working() {
                let still_working = after.paths.iter().any(|ap| {
                    ap.method == before_path.method
                        && ap.to == before_path.to
                        && ap.status.is_working()
                });
                if !still_working {
                    paths_lost.push(before_path.clone());
                }
            }
        }

        Self {
            register_write,
            before,
            after,
            paths_gained,
            paths_lost,
        }
    }

    /// True if this register write made new memory paths available.
    pub fn unlocked_memory(&self) -> bool {
        !self.paths_gained.is_empty()
    }

    /// True if this register write broke existing memory paths.
    pub fn broke_memory(&self) -> bool {
        !self.paths_lost.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_status_sentinel_match() {
        let status = PathStatus::from_sentinel_test(0xDEAD_BEEF, 0xDEAD_BEEF, 42);
        assert!(status.is_working());
        assert_eq!(status, PathStatus::Working { latency_us: 42 });
    }

    #[test]
    fn path_status_sentinel_bad0() {
        let status = PathStatus::from_sentinel_test(0xDEAD_BEEF, 0xBAD0_AC00, 0);
        assert!(status.is_error_pattern());
    }

    #[test]
    fn path_status_sentinel_ffff() {
        let status = PathStatus::from_sentinel_test(0xDEAD_BEEF, 0xFFFF_FFFF, 0);
        assert!(status.is_error_pattern());
    }

    #[test]
    fn path_status_sentinel_corrupt() {
        let status = PathStatus::from_sentinel_test(0xDEAD_BEEF, 0x1234_5678, 0);
        assert!(!status.is_working());
        assert!(!status.is_error_pattern());
        assert_eq!(
            status,
            PathStatus::Corrupted {
                wrote: 0xDEAD_BEEF,
                read: 0x1234_5678
            }
        );
    }

    #[test]
    fn aperture_display() {
        let a = Aperture::SystemMemory {
            iova: 0x1000,
            coherent: true,
        };
        assert!(format!("{a}").contains("0x1000"));

        let b = Aperture::VideoMemory {
            vram_offset: 0x20000,
        };
        assert!(format!("{b}").contains("VRAM"));

        let c = Aperture::RegisterSpace { offset: 0x200 };
        assert!(format!("{c}").contains("MMIO"));
    }

    #[test]
    fn memory_error_display() {
        let e = MemoryError::OutOfBounds {
            offset: 0x100,
            size: 0x80,
        };
        assert!(format!("{e}").contains("out of bounds"));
    }

    #[test]
    fn memory_topology_working_paths_empty() {
        let topo = MemoryTopology {
            vram_accessible: false,
            vram_size_probed: 0,
            sysmem_dma_ok: false,
            bar2_configured: false,
            paths: vec![],
            evidence: vec![],
        };
        assert!(topo.working_paths(PathMethod::Pramin).is_empty());
        assert!(!topo.pramin_works());
        assert!(!topo.dma_works());
    }

    #[test]
    fn memory_delta_compute_gains() {
        let before = MemoryTopology {
            vram_accessible: false,
            vram_size_probed: 0,
            sysmem_dma_ok: false,
            bar2_configured: false,
            paths: vec![AccessPath {
                from: "cpu",
                to: Aperture::VideoMemory { vram_offset: 0 },
                method: PathMethod::Pramin,
                status: PathStatus::ErrorPattern {
                    pattern: 0xBAD0_AC00,
                },
                prerequisites: vec![],
            }],
            evidence: vec![],
        };

        let after = MemoryTopology {
            vram_accessible: true,
            vram_size_probed: 0x1000,
            sysmem_dma_ok: false,
            bar2_configured: false,
            paths: vec![AccessPath {
                from: "cpu",
                to: Aperture::VideoMemory { vram_offset: 0 },
                method: PathMethod::Pramin,
                status: PathStatus::Working { latency_us: 5 },
                prerequisites: vec![],
            }],
            evidence: vec![],
        };

        let delta = MemoryDelta::compute((0x200, 0xFFFF_FFFF), before, after);
        assert!(delta.unlocked_memory());
        assert!(!delta.broke_memory());
        assert_eq!(delta.paths_gained.len(), 1);
        assert_eq!(delta.paths_gained[0].method, PathMethod::Pramin);
    }
}
