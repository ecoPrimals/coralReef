// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt;
use std::fmt::Write as FmtWrite;

use super::core::{Aperture, PathStatus};

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

    /// Emit a human-readable memory topology summary via `tracing`.
    pub fn print_summary(&self) {
        let mut s = String::new();
        writeln!(
            &mut s,
            "╠══ Memory Topology ═══════════════════════════════╣"
        )
        .expect("writing to String is infallible");
        writeln!(
            &mut s,
            "║ VRAM: accessible={} probed_size={:#x}",
            self.vram_accessible, self.vram_size_probed
        )
        .expect("writing to String is infallible");
        writeln!(
            &mut s,
            "║ SysMem: dma_ok={}  BAR2: configured={}",
            self.sysmem_dma_ok, self.bar2_configured
        )
        .expect("writing to String is infallible");
        for path in &self.paths {
            let status_str = match &path.status {
                PathStatus::Working { latency_us } => format!("OK ({latency_us}us)"),
                PathStatus::Corrupted { wrote, read } => {
                    format!("CORRUPT (wrote={wrote:#x} read={read:#x})")
                }
                PathStatus::ErrorPattern { pattern } => format!("ERROR ({pattern:#010x})"),
                PathStatus::Untested => "UNTESTED".to_string(),
            };
            writeln!(
                &mut s,
                "║   {} → {} via {}: {}",
                path.from, path.to, path.method, status_str
            )
            .expect("writing to String is infallible");
        }
        tracing::info!(summary = %s, "memory topology");
    }
}

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
