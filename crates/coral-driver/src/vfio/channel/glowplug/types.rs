// SPDX-License-Identifier: AGPL-3.0-or-later
#![expect(missing_docs, reason = "GlowPlug types; full docs planned")]

use crate::vfio::bar_cartography;
use crate::vfio::memory::MemoryTopology;

/// Current state of the GPU as understood by the glowplug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuThermalState {
    /// GPU is in D3hot (PCIe sleep) — BAR0 reads 0xFFFFFFFF.
    D3Hot,
    /// GPU responds to BAR0 but engines are clock-gated (PMC = 0x40000020).
    ColdGated,
    /// PMC engines clocked but PFIFO not functional.
    EnginesClocked,
    /// PFIFO alive but VRAM returns error patterns (FB controller not initialized).
    PfifoAliveVramDead,
    /// PFIFO alive, VRAM accessible, BAR2 not configured.
    VramAliveBar2Dead,
    /// Fully warm — PFIFO, VRAM, BAR2 all functional.
    Warm,
}

/// A register snapshot taken before/after a warm-up step.
#[derive(Debug, Clone)]
pub struct StepSnapshot {
    /// Step description.
    pub step: String,
    /// Register values before the step: (offset, value).
    pub before: Vec<(usize, u32)>,
    /// Register values after the step: (offset, value).
    pub after: Vec<(usize, u32)>,
}

impl StepSnapshot {
    /// Registers that changed during this step.
    pub fn deltas(&self) -> Vec<(usize, u32, u32)> {
        bar_cartography::diff_snapshots(&self.before, &self.after)
    }
}

/// Result of a glowplug warm-up attempt.
#[derive(Debug)]
pub struct WarmResult {
    /// State before warm-up.
    pub initial_state: GpuThermalState,
    /// State after warm-up.
    pub final_state: GpuThermalState,
    /// Whether the GPU reached a fully warm state.
    pub success: bool,
    /// Memory topology after warm-up (if probed).
    pub memory: Option<MemoryTopology>,
    /// Diagnostic messages collected during warm-up.
    pub log: Vec<String>,
    /// Per-step register snapshots for forensics.
    pub step_snapshots: Vec<StepSnapshot>,
}

/// Snapshot of GPU health at a point in time, for the listener/watchdog.
#[derive(Debug, Clone)]
pub struct HealthSnapshot {
    pub thermal_state: GpuThermalState,
    pub domains_alive: usize,
    pub domains_faulted: usize,
    pub vram_accessible: bool,
    pub pmc_enable: u32,
    pub log: Vec<String>,
}
