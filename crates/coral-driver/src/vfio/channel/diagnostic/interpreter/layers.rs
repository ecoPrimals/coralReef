// SPDX-License-Identifier: AGPL-3.0-only
//! Typed layer outputs for the GPU interpreter.
//!
//! Each struct is produced by one layer and consumed by the next.
//! The Rust type system enforces ordering: you can't build a `ChannelConfig`
//! without first having an `EngineTopology`, which requires `PowerState`, etc.

use std::fmt;

/// Evidence of why a probe failed — carries the register state that
/// informed the failure so the next attempt can adapt.
#[derive(Debug, Clone)]
pub struct ProbeFailure {
    pub layer: &'static str,
    pub step: &'static str,
    pub evidence: Vec<(String, u32)>,
    pub message: String,
}

impl fmt::Display for ProbeFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}::{}] {}", self.layer, self.step, self.message)?;
        for (name, val) in &self.evidence {
            write!(f, "\n  {name} = {val:#010x}")?;
        }
        Ok(())
    }
}

// ─── Layer 0: BAR Topology ─────────────────────────────────────────────────

/// Result of probing BAR0 MMIO accessibility.
#[derive(Debug, Clone)]
pub struct BarTopology {
    /// BAR0 appears readable (not all 0xFF or all 0x00).
    pub bar0_readable: bool,
    /// BAR0 appears writable (write-readback succeeds on a safe register).
    pub bar0_writable: bool,
    /// Sentinel value read from offset 0 (BOOT0) before any writes.
    pub boot0_raw: u32,
    /// GPU in D3hot power state (all FFs).
    pub in_d3hot: bool,
}

// ─── Layer 1: GPU Identity ─────────────────────────────────────────────────

/// Decoded GPU identity from NV_PMC_BOOT_0.
#[derive(Debug, Clone)]
pub struct GpuIdentity {
    pub bar: BarTopology,
    pub boot0: u32,
    pub architecture: GpuArch,
    pub implementation: u8,
    pub revision: u8,
    /// PMC_BOOT_42 if present (extended ID for newer chips).
    pub boot42: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuArch {
    Tesla,     // NV50-NV90
    Fermi,     // NVC0-NVD0
    Kepler,    // NVE0-NV10x
    Maxwell,   // NV11x-NV12x
    Pascal,    // NV13x
    Volta,     // NV14x (GV100)
    Turing,    // NV16x
    Ampere,    // NV17x
    Ada,       // NV19x
    Blackwell, // NV1Bx
    Unknown(u8),
}

impl GpuArch {
    pub fn from_boot0(boot0: u32) -> Self {
        match (boot0 >> 20) & 0x1FF {
            0x050..=0x090 => Self::Tesla,
            0x0C0..=0x0D0 => Self::Fermi,
            0x0E0..=0x10F => Self::Kepler,
            0x110..=0x12F => Self::Maxwell,
            0x130..=0x13F => Self::Pascal,
            0x140..=0x14F => Self::Volta,
            0x160..=0x16F => Self::Turing,
            0x170..=0x17F => Self::Ampere,
            0x190..=0x19F => Self::Ada,
            0x1B0..=0x1BF => Self::Blackwell,
            other => Self::Unknown(other as u8),
        }
    }

    pub fn has_gsp(&self) -> bool {
        matches!(
            self,
            Self::Turing | Self::Ampere | Self::Ada | Self::Blackwell
        )
    }

    pub fn has_hw_scheduler(&self) -> bool {
        matches!(
            self,
            Self::Kepler
                | Self::Maxwell
                | Self::Pascal
                | Self::Volta
                | Self::Turing
                | Self::Ampere
                | Self::Ada
                | Self::Blackwell
        )
    }

    /// Number of V2 MMU page table levels (5 for GP100+).
    pub fn mmu_levels(&self) -> Option<u8> {
        match self {
            Self::Pascal
            | Self::Volta
            | Self::Turing
            | Self::Ampere
            | Self::Ada
            | Self::Blackwell => Some(5),
            Self::Fermi | Self::Kepler | Self::Maxwell => Some(2),
            _ => None,
        }
    }
}

impl fmt::Display for GpuArch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tesla => write!(f, "Tesla"),
            Self::Fermi => write!(f, "Fermi"),
            Self::Kepler => write!(f, "Kepler"),
            Self::Maxwell => write!(f, "Maxwell"),
            Self::Pascal => write!(f, "Pascal"),
            Self::Volta => write!(f, "Volta"),
            Self::Turing => write!(f, "Turing"),
            Self::Ampere => write!(f, "Ampere"),
            Self::Ada => write!(f, "Ada"),
            Self::Blackwell => write!(f, "Blackwell"),
            Self::Unknown(v) => write!(f, "Unknown({v:#x})"),
        }
    }
}

// ─── Layer 2: Power State ──────────────────────────────────────────────────

/// Result of power/engine bring-up probing.
#[derive(Debug, Clone)]
pub struct PowerState {
    pub identity: GpuIdentity,
    /// PMC_ENABLE before any writes.
    pub pmc_enable_initial: u32,
    /// PMC_ENABLE after bring-up attempt.
    pub pmc_enable_final: u32,
    /// Engines present (bitmask from PMC_ENABLE readback).
    pub engines_present: u32,
    /// PFIFO_ENABLE status after bring-up.
    pub pfifo_enabled: bool,
    /// PFIFO_ENABLE value (raw register read).
    pub pfifo_enable_raw: u32,
    /// Method used to achieve power state.
    pub method: PowerMethod,
    /// Whether PTIMER is ticking (DMA timeouts depend on this).
    pub ptimer_ticking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerMethod {
    /// GPU was already warm (from nouveau or prior session).
    AlreadyWarm,
    /// Warmed via PMC_ENABLE write only.
    PmcEnableOnly,
    /// Warmed via PMC bit toggle (clear/set engine bit) + PFIFO enable.
    PmcResetCycle,
    /// Warmed via full glow plug sequence (BAR2 page tables etc).
    GlowPlug,
    /// Could not achieve warm state.
    Failed,
}

// ─── Layer 3: Engine Topology ──────────────────────────────────────────────

/// Discovered PBDMA and runlist topology.
#[derive(Debug, Clone)]
pub struct EngineTopology {
    pub power: PowerState,
    /// PBDMAs present (bitmask).
    pub pbdma_map: u32,
    /// Mapping from PBDMA index to runlist ID.
    pub pbdma_to_runlist: Vec<(usize, u32)>,
    /// GR (graphics) engine's runlist ID.
    pub gr_runlist: Option<u32>,
    /// Primary PBDMA serving the GR runlist.
    pub gr_pbdma: Option<usize>,
    /// Secondary PBDMA for the GR runlist (if dual-PBDMA).
    pub alt_pbdma: Option<usize>,
    /// BAR1_BLOCK register value.
    pub bar1_block: u32,
    /// BAR2_BLOCK register value.
    pub bar2_block: u32,
    /// Whether BAR2 page tables were set up by this probe.
    pub bar2_setup_needed: bool,
}

// ─── Layer 4: DMA Capability ───────────────────────────────────────────────

/// Result of testing whether the GPU can DMA to/from system memory.
#[derive(Debug, Clone)]
pub struct DmaCapability {
    pub engines: EngineTopology,
    /// Can the GPU read from a known IOVA (verified via PBDMA context load)?
    pub gpu_can_read_sysmem: bool,
    /// Can the GPU write to a known IOVA (verified via GP_GET writeback)?
    pub gpu_can_write_sysmem: bool,
    /// IOMMU mapping verified (DMA buffer allocation succeeded).
    pub iommu_mapping_ok: bool,
    /// Page tables set up and TLB flushed.
    pub page_tables_ok: bool,
    /// Instance block accessible from PBDMA (RAMFC values load correctly).
    pub instance_block_accessible: bool,
    /// Evidence: what the PBDMA CTX registers showed after context load attempt.
    pub ctx_evidence: Vec<(String, u32)>,
}

// ─── Layer 5: Channel Config ───────────────────────────────────────────────

/// Discovered working channel configuration.
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub dma: DmaCapability,
    /// PCCSR INST target that worked (0=VRAM, 2=COH, 3=NCOH).
    pub working_inst_target: u32,
    /// Runlist USERD target that worked.
    pub working_userd_target: u32,
    /// Whether instance block must be in VRAM.
    pub instance_requires_vram: bool,
    /// Whether USERD must be in VRAM.
    pub userd_requires_vram: bool,
    /// Whether INST_BIND is needed (vs direct PCCSR write).
    pub inst_bind_needed: bool,
    /// Whether runlist ACK protocol (BIT30) fires.
    pub runlist_ack_works: bool,
    /// Method that achieved SCHEDULED state.
    pub scheduling_method: SchedulingMethod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulingMethod {
    /// Hardware scheduler processes runlist automatically.
    HardwareScheduler,
    /// Manual PCCSR SCHED bit + direct PBDMA programming.
    ManualPccsrSched,
    /// RAMFC mirror registers + SCHED bit.
    RamfcMirrorSched,
    /// Could not achieve scheduling.
    None,
}

// ─── Layer 6: Dispatch Result ──────────────────────────────────────────────

/// Final capability assessment — can we execute GPU commands?
#[derive(Debug, Clone)]
pub struct DispatchCapability {
    pub channel: ChannelConfig,
    /// GP_GET advanced after submitting a NOP GPFIFO entry.
    pub gpfifo_consumed: bool,
    /// GPU executed the NOP push buffer method.
    pub nop_executed: bool,
    /// Full dispatch pipeline works (can submit arbitrary work).
    pub dispatch_ready: bool,
    /// Remaining blockers, if any.
    pub blockers: Vec<String>,
}
