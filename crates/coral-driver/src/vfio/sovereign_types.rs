// SPDX-License-Identifier: AGPL-3.0-or-later
//! Public types for the sovereign GPU initialization pipeline.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Which stage to halt before (for debugging partial pipelines).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HaltBefore {
    /// Halt before master clock/engine enable.
    PmcEnable,
    /// Halt before HBM2 memory controller bring-up.
    Hbm2Training,
    /// Halt before falcon (SEC2/ACR/FECS) boot.
    FalconBoot,
    /// Halt before GR engine register programming.
    GrInit,
    /// Halt before final VRAM/PTIMER verification.
    Verify,
}

/// Options controlling the sovereign init pipeline.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SovereignInitOptions {
    /// Halt the pipeline before this stage (for experiments).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub halt_before: Option<HaltBefore>,
    /// Golden register captures for differential HBM2 replay.
    #[serde(skip)]
    pub golden_state: Option<Vec<(usize, u32)>>,
    /// File path to a JSON golden-state capture (loaded by the RPC handler).
    /// Format: array of `[offset, value]` pairs, or a `TrainingRecipe` JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub golden_state_path: Option<String>,
    /// Explicit VBIOS ROM bytes (otherwise read from PROM/sysfs).
    #[serde(skip)]
    pub vbios_rom: Option<Vec<u8>>,
    /// File path to a raw VBIOS ROM dump (loaded by the RPC handler).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vbios_rom_path: Option<String>,
    /// Number of FBPA partitions (auto-detected if None).
    pub fbpa_count: Option<usize>,
    /// SM version for GR init (70 = GV100, 75 = TU102, etc.).
    pub sm_version: Option<u32>,
    /// Skip GR init even if falcon boot succeeds.
    #[serde(default)]
    pub skip_gr_init: bool,
    /// DMA backend for system-memory ACR boot (IOMMU-mapped buffers).
    /// When provided, the ACR boot solver can use strategies that place
    /// the WPR in system memory rather than VRAM-only paths.
    #[serde(skip)]
    pub dma_backend: Option<crate::vfio::device::DmaBackend>,
}

/// Outcome of a single pipeline stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResult {
    /// Stage identifier (e.g. `"bar0_probe"`, `"hbm2_training"`).
    pub name: String,
    /// Whether the stage passed, was skipped, or failed.
    pub status: StageStatus,
    /// Human-readable detail about the stage outcome.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
}

/// Status of a sovereign init stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    /// Stage completed successfully.
    Ok,
    /// Stage was not needed or halted by request.
    Skipped,
    /// Stage failed (see `StageResult::detail`).
    Failed,
}

/// Full result of the sovereign init pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SovereignInitResult {
    /// PCI BDF address of the device.
    pub bdf: String,
    /// Decoded chip ID from BOOT0 (e.g. 0x140 for GV100).
    pub chip_id: u32,
    /// Raw BOOT0 register value.
    pub boot0: u32,
    /// True if every executed stage passed.
    pub all_ok: bool,
    /// True if the full pipeline completed and GPU is compute-ready.
    pub compute_ready: bool,
    /// Stage name at which the pipeline was halted (by request or failure).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub halted_at: Option<String>,
    /// Per-stage results in execution order.
    pub stages: Vec<StageResult>,
    /// Total pipeline wall-clock time in milliseconds.
    pub total_ms: u64,
    /// Number of HBM2 training register writes (if training ran).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hbm2_writes: Option<usize>,
    /// Whether the GPU was detected as warm (HBM2 training skipped/reduced).
    #[serde(default)]
    pub warm_detected: bool,
}

impl fmt::Display for SovereignInitResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.compute_ready {
            "COMPUTE_READY"
        } else if let Some(h) = &self.halted_at {
            return write!(f, "HALTED@{h} ({}ms)", self.total_ms);
        } else {
            "INCOMPLETE"
        };
        write!(
            f,
            "{status} chip=0x{:03x} stages={}/{} ({}ms)",
            self.chip_id,
            self.stages
                .iter()
                .filter(|s| s.status == StageStatus::Ok)
                .count(),
            self.stages.len(),
            self.total_ms,
        )
    }
}
