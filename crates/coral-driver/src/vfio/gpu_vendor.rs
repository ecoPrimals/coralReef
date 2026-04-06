// SPDX-License-Identifier: AGPL-3.0-or-later
//! Vendor-agnostic GPU metal interface.
//!
//! The `GpuMetal` trait abstracts over NVIDIA, AMD, and Intel GPU register
//! layouts so that GlowPlug, the diagnostic interpreter, and the BAR
//! cartography system can work on any GPU without hardcoded register offsets.
//!
//! Each vendor implementation provides:
//! - Identity decoding (chip name, architecture, revision)
//! - Power domain register map (which bits control which engines)
//! - Memory region topology (VRAM, caches, apertures)
//! - Engine enumeration (compute, copy, display, video)
//! - Register domain boundaries for cartography
//! - Warm-up sequence steps for GlowPlug

use std::fmt;

use super::bar_cartography::DomainHint;
use super::pci_discovery::GpuVendor;

// ── GPU Identity ────────────────────────────────────────────────────────

/// Decoded GPU identity from BAR0 register reads.
pub trait GpuIdentity: fmt::Debug {
    /// Vendor (NVIDIA, AMD, Intel).
    fn vendor(&self) -> GpuVendor;
    /// Human-readable chip name (e.g., "GV100", "Vega 20").
    fn chip_name(&self) -> &str;
    /// Architecture generation name (e.g., "Volta", "Vega").
    fn architecture(&self) -> &str;
    /// Implementation number within the architecture.
    fn implementation(&self) -> u8;
    /// Silicon revision.
    fn revision(&self) -> u8;
    /// Raw identity register value (BOOT0 for NVIDIA, etc.).
    fn raw_id(&self) -> u32;
}

// ── Power Domain ────────────────────────────────────────────────────────

/// State of a power/clock domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainState {
    /// Domain is powered on and clocked.
    Active,
    /// Domain is powered but clock is gated.
    ClockGated,
    /// Domain is fully powered off.
    PowerGated,
    /// State cannot be determined.
    Unknown,
}

/// A power/clock domain in the GPU.
#[derive(Debug, Clone)]
pub struct PowerDomain {
    /// Human-readable domain name (e.g., "GR", "PFIFO", "CE0").
    pub name: &'static str,
    /// Register offset for the enable/status register (if known).
    pub enable_reg: Option<usize>,
    /// Bit in the enable register that controls this domain.
    pub enable_bit: Option<u32>,
    /// Clock register offset (if separate from enable).
    pub clock_reg: Option<usize>,
    /// Current state (probed, not static).
    pub state: DomainState,
}

// ── Memory Region ───────────────────────────────────────────────────────

/// A memory region accessible through the GPU.
#[derive(Debug, Clone)]
pub struct MetalMemoryRegion {
    /// Human-readable name (e.g., "VRAM", "L2 Cache", "PRAMIN").
    pub name: &'static str,
    /// Type of memory.
    pub kind: MemoryKind,
    /// Base BAR0 offset for control registers (if applicable).
    pub control_base: Option<usize>,
    /// Size in bytes (if known).
    pub size: Option<u64>,
    /// Number of partitions (for HBM2 stacks, L2 slices, etc.).
    pub partitions: Option<u32>,
}

/// Memory region type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKind {
    /// Video RAM (HBM2, GDDR6, etc.).
    Vram,
    /// L1 cache (per-SM or per-TPC).
    L1Cache,
    /// L2 cache (shared, partitioned).
    L2Cache,
    /// BAR aperture for CPU access to VRAM.
    Aperture,
    /// Register file or scratchpad.
    RegisterFile,
    /// DMA-accessible system memory region.
    SystemMemory,
}

// ── Engine Info ──────────────────────────────────────────────────────────

/// A GPU engine or functional unit.
#[derive(Debug, Clone)]
pub struct EngineInfo {
    /// Human-readable engine name.
    pub name: &'static str,
    /// Engine type.
    pub kind: EngineKind,
    /// Base BAR0 register offset for this engine.
    pub base_offset: usize,
    /// Whether this engine has dedicated firmware.
    pub has_firmware: bool,
    /// Current firmware state.
    pub firmware_state: FirmwareState,
}

/// Engine classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    /// General-purpose compute (GR/GFX on NVIDIA, GC on AMD).
    Compute,
    /// DMA copy engine (CE on NVIDIA, SDMA on AMD).
    Copy,
    /// Display engine.
    Display,
    /// Video encode/decode (NVENC/NVDEC, VCN).
    Video,
    /// Hardware scheduler (Host/PFIFO/RLCP).
    Scheduler,
    /// Memory controller.
    MemoryController,
    /// Unknown or unclassified engine.
    Unknown,
}

/// Firmware status on a microcontroller (FALCON, MES, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareState {
    /// Firmware is loaded and executing.
    Running,
    /// Firmware is loaded but halted.
    Halted,
    /// No firmware loaded.
    NotLoaded,
    /// Firmware is encrypted (needs hardware decryption).
    Encrypted,
    /// Engine has no firmware capability.
    NotPresent,
}

// ── Warm-up Sequence ────────────────────────────────────────────────────

/// A step in the GPU warm-up sequence.
#[derive(Debug, Clone)]
pub struct WarmupStep {
    /// Human-readable step description.
    pub description: &'static str,
    /// Register writes to perform.
    pub writes: Vec<RegisterWrite>,
    /// Delay after writes (milliseconds).
    pub delay_ms: u64,
    /// Register reads to verify success.
    pub verify: Vec<RegisterVerify>,
}

/// A register write as part of a warm-up step.
#[derive(Debug, Clone)]
pub struct RegisterWrite {
    /// BAR0 offset.
    pub offset: usize,
    /// Value to write.
    pub value: u32,
    /// Optional mask for read-modify-write.
    pub mask: Option<u32>,
}

/// A register read-and-check for step verification.
#[derive(Debug, Clone)]
pub struct RegisterVerify {
    /// BAR0 offset to read.
    pub offset: usize,
    /// Expected value (after masking).
    pub expected: u32,
    /// Mask to apply before comparison (0xFFFFFFFF = exact match).
    pub mask: u32,
}

// ── The Trait ────────────────────────────────────────────────────────────

/// Vendor-agnostic GPU metal interface.
///
/// Implementations provide register maps, domain layouts, and warm-up
/// sequences for a specific GPU vendor and architecture. GlowPlug and
/// the diagnostic interpreter use this trait instead of hardcoded offsets.
pub trait GpuMetal: fmt::Debug + Send + Sync {
    /// GPU identity (chip, arch, revision).
    fn identity(&self) -> &dyn GpuIdentity;

    /// Power/clock domains and their control registers.
    fn power_domains(&self) -> &[PowerDomain];

    /// Memory regions (VRAM, caches, apertures).
    fn memory_regions(&self) -> &[MetalMemoryRegion];

    /// Engines (compute, copy, display, video, scheduler).
    fn engine_list(&self) -> &[EngineInfo];

    /// BAR0 offset range for a named register domain.
    fn register_domain(&self, name: &str) -> Option<(usize, usize)>;

    /// Domain hints for BAR cartography.
    fn domain_hints(&self) -> Vec<DomainHint>;

    /// Ordered warm-up steps to bring the GPU from cold to operational.
    fn warmup_sequence(&self) -> Vec<WarmupStep>;

    /// BOOT0 register offset (0x0 for NVIDIA, varies for AMD).
    fn boot0_offset(&self) -> usize;

    /// PMC/engine enable register offset.
    fn pmc_enable_offset(&self) -> usize;

    /// PFIFO PBDMA map register offset (for detecting active PBDMAs).
    fn pbdma_map_offset(&self) -> Option<usize>;

    /// PRAMIN window base offset (for CPU access to VRAM).
    fn pramin_base_offset(&self) -> Option<usize>;

    /// BAR2 block register offset (for page table configuration).
    fn bar2_block_offset(&self) -> Option<usize>;
}

/// Result of probing power state boundaries empirically.
#[derive(Debug, Clone, Default)]
pub struct PowerBounds {
    /// What state survives D3hot → D0 transition.
    pub d3hot_survives: Vec<String>,
    /// What state is lost in D3hot.
    pub d3hot_lost: Vec<String>,
    /// What state survives D3cold power cycle.
    pub d3cold_survives: Vec<String>,
    /// What state is lost in D3cold.
    pub d3cold_lost: Vec<String>,
    /// What state survives clock gating.
    pub clock_gate_survives: Vec<String>,
    /// What state is lost in clock gating.
    pub clock_gate_lost: Vec<String>,
}
