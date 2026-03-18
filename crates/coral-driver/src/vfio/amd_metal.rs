// SPDX-License-Identifier: AGPL-3.0-only
//! AMD Vega 20 (MI50/MI60, GFX906) `GpuMetal` implementation.
//!
//! EVOLUTION: Register offsets from AMD ISA docs — awaiting MI50 hardware validation.
//!
//! Register offsets derived from AMD's publicly documented MMIO layout
//! and Mesa amdgpu driver headers for Vega/GFX906. Key subsystems:
//!
//! - **SMC**: System Management Controller (power, thermal — indirect access)
//! - **GRBM**: Graphics Request Broker Manager (engine status, soft reset)
//! - **UMC**: Unified Memory Controller (HBM2 interface)
//! - **GFX**: Graphics Core (compute dispatch, shader execution)
//! - **SDMA**: System DMA engines (memory copy, fill)
//! - **MMHUB**: Memory Management Hub (GART, page tables)

use super::bar_cartography::DomainHint;
use super::gpu_vendor::*;
use super::pci_discovery::GpuVendor;

// ── AMD Vega 20 / GFX906 MMIO Register Offsets ───────────────────────
//
// Sources: AMD documentation, Mesa amdgpu driver headers (soc15, gfx_v9_0).
// Offsets are byte addresses within the relevant IP block's MMIO window.

// SMC (System Management Controller) — indirect register access
const SMC_IND_INDEX_11: usize = 0x01AC;
const SMC_IND_DATA_11: usize = 0x01AD;

// GRBM (Graphics Request Broker Manager)
const GRBM_STATUS: usize = 0x8010;
const GRBM_STATUS2: usize = 0x8008;
const GRBM_SOFT_RESET: usize = 0x8020;

// GFX / GC (Graphics Core) — memory config
const GB_ADDR_CONFIG: usize = 0x263E;

// UMC / MC (Memory Controller) — VRAM aperture
const MC_VM_FB_LOCATION_BASE: usize = 0x2023;
const MC_VM_FB_LOCATION_TOP: usize = 0x2024;

// SRBM, CP, SDMA, RLC, MMHUB
const SRBM_STATUS: usize = 0x0E50;
const CP_STAT: usize = 0x8680;
const SDMA0_BASE: usize = 0x4D00;
const SDMA1_BASE: usize = 0x5900;
const RLC_BASE: usize = 0xEC00;
const MMHUB_VM_BASE: usize = 0x0600;

const MI50_HBM2_SIZE: u64 = 16 * 1024 * 1024 * 1024;
const MI50_HBM2_STACKS: u32 = 4;
const MI50_L2_SIZE: u64 = 4 * 1024 * 1024;
const MI50_L2_SLICES: u32 = 16;
const BUSY_BIT_MASK: u32 = 0x8000_0000;

/// AMD Vega 20 (MI50) identity.
#[derive(Debug, Clone)]
pub struct AmdVegaIdentity {
    /// Raw identity register value (GC_CONFIG or PCI device ID).
    pub raw: u32,
}

impl GpuIdentity for AmdVegaIdentity {
    fn vendor(&self) -> GpuVendor {
        GpuVendor::Amd
    }
    fn chip_name(&self) -> &str {
        "Vega 20 (MI50)"
    }
    fn architecture(&self) -> &str {
        "GFX906"
    }
    fn implementation(&self) -> u8 {
        20
    }
    fn revision(&self) -> u8 {
        0
    }
    fn raw_id(&self) -> u32 {
        self.raw
    }
}

/// AMD Vega 20 `GpuMetal` — bare-metal register access for MI50/MI60.
#[derive(Debug)]
pub struct AmdVegaMetal {
    identity: AmdVegaIdentity,
    power_domains: Vec<PowerDomain>,
    memory_regions: Vec<MetalMemoryRegion>,
    engines: Vec<EngineInfo>,
}

impl AmdVegaMetal {
    /// Create a new Vega 20 metal instance with GFX906 register layout.
    pub fn new(raw_id: u32) -> Self {
        Self {
            identity: AmdVegaIdentity { raw: raw_id },
            power_domains: vec![
                PowerDomain {
                    name: "SMC",
                    enable_reg: Some(SMC_IND_INDEX_11),
                    enable_bit: None,
                    clock_reg: Some(SMC_IND_DATA_11),
                    state: DomainState::Unknown,
                },
                PowerDomain {
                    name: "GFX",
                    enable_reg: Some(GRBM_SOFT_RESET),
                    enable_bit: Some(0x01),
                    clock_reg: None,
                    state: DomainState::Unknown,
                },
                PowerDomain {
                    name: "GRBM",
                    enable_reg: Some(GRBM_STATUS),
                    enable_bit: None,
                    clock_reg: Some(GRBM_STATUS2),
                    state: DomainState::Unknown,
                },
                PowerDomain {
                    name: "UMC",
                    enable_reg: Some(MC_VM_FB_LOCATION_BASE),
                    enable_bit: None,
                    clock_reg: Some(MC_VM_FB_LOCATION_TOP),
                    state: DomainState::Unknown,
                },
                PowerDomain {
                    name: "SYS",
                    enable_reg: Some(SRBM_STATUS),
                    enable_bit: None,
                    clock_reg: None,
                    state: DomainState::Unknown,
                },
                PowerDomain {
                    name: "RLC",
                    enable_reg: None,
                    enable_bit: None,
                    clock_reg: None,
                    state: DomainState::Unknown,
                },
            ],
            memory_regions: vec![
                MetalMemoryRegion {
                    name: "HBM2_FB",
                    kind: MemoryKind::Vram,
                    control_base: Some(MC_VM_FB_LOCATION_BASE),
                    size: Some(MI50_HBM2_SIZE),
                    partitions: Some(MI50_HBM2_STACKS),
                },
                MetalMemoryRegion {
                    name: "GART",
                    kind: MemoryKind::SystemMemory,
                    control_base: Some(MMHUB_VM_BASE),
                    size: None,
                    partitions: None,
                },
                MetalMemoryRegion {
                    name: "L2_CACHE",
                    kind: MemoryKind::L2Cache,
                    control_base: None,
                    size: Some(MI50_L2_SIZE),
                    partitions: Some(MI50_L2_SLICES),
                },
            ],
            engines: vec![
                EngineInfo {
                    name: "GFX",
                    kind: EngineKind::Compute,
                    base_offset: CP_STAT,
                    has_firmware: true,
                    firmware_state: FirmwareState::NotLoaded,
                },
                EngineInfo {
                    name: "SDMA0",
                    kind: EngineKind::Copy,
                    base_offset: SDMA0_BASE,
                    has_firmware: true,
                    firmware_state: FirmwareState::NotLoaded,
                },
                EngineInfo {
                    name: "SDMA1",
                    kind: EngineKind::Copy,
                    base_offset: SDMA1_BASE,
                    has_firmware: true,
                    firmware_state: FirmwareState::NotLoaded,
                },
            ],
        }
    }
}

impl GpuMetal for AmdVegaMetal {
    fn identity(&self) -> &dyn GpuIdentity {
        &self.identity
    }

    fn power_domains(&self) -> &[PowerDomain] {
        &self.power_domains
    }

    fn memory_regions(&self) -> &[MetalMemoryRegion] {
        &self.memory_regions
    }

    fn engine_list(&self) -> &[EngineInfo] {
        &self.engines
    }

    fn register_domain(&self, name: &str) -> Option<(usize, usize)> {
        match name {
            "SMC" => Some((SMC_IND_INDEX_11, SMC_IND_DATA_11 + 4)),
            "GRBM" => Some((0x8000, 0x8FFF)),
            "GRBM_STATUS" => Some((GRBM_STATUS, GRBM_STATUS + 4)),
            "GRBM_STATUS2" => Some((GRBM_STATUS2, GRBM_STATUS2 + 4)),
            "GFX" | "GC" => Some((0x2000, 0x3FFF)),
            "GB_ADDR_CONFIG" => Some((GB_ADDR_CONFIG, GB_ADDR_CONFIG + 4)),
            "UMC" | "MC" => Some((MC_VM_FB_LOCATION_BASE, MC_VM_FB_LOCATION_TOP + 4)),
            "SRBM" => Some((0x0E00, 0x0EFF)),
            "CP" => Some((0x8600, 0x86FF)),
            "SDMA0" => Some((0x4D00, 0x4DFF)),
            "SDMA1" => Some((0x5900, 0x59FF)),
            "RLC" => Some((0xEC00, 0xECFF)),
            "MMHUB" => Some((0x0600, 0x0AFF)),
            _ => None,
        }
    }

    fn domain_hints(&self) -> Vec<DomainHint> {
        vec![
            DomainHint {
                start: SMC_IND_INDEX_11,
                end: SMC_IND_DATA_11 + 4,
                name: "SMC",
            },
            DomainHint {
                start: 0x8000,
                end: 0x8FFF,
                name: "GRBM",
            },
            DomainHint {
                start: 0x0E00,
                end: 0x0EFF,
                name: "SRBM",
            },
            DomainHint {
                start: 0x8600,
                end: 0x86FF,
                name: "CP",
            },
            DomainHint {
                start: 0x4D00,
                end: 0x4DFF,
                name: "SDMA0",
            },
            DomainHint {
                start: 0x5900,
                end: 0x59FF,
                name: "SDMA1",
            },
            DomainHint {
                start: RLC_BASE,
                end: 0xECFF,
                name: "RLC",
            },
            DomainHint {
                start: MMHUB_VM_BASE,
                end: 0x0AFF,
                name: "MMHUB",
            },
            DomainHint {
                start: 0x2000,
                end: 0x3FFF,
                name: "GFX",
            },
            DomainHint {
                start: MC_VM_FB_LOCATION_BASE,
                end: MC_VM_FB_LOCATION_TOP + 4,
                name: "UMC",
            },
            DomainHint {
                start: GB_ADDR_CONFIG,
                end: GB_ADDR_CONFIG + 4,
                name: "GB_ADDR_CONFIG",
            },
        ]
    }

    fn warmup_sequence(&self) -> Vec<WarmupStep> {
        vec![WarmupStep {
            description: "Read GRBM_STATUS and GRBM_STATUS2 (GFX engine idle)",
            writes: vec![],
            delay_ms: 0,
            verify: vec![
                RegisterVerify {
                    offset: GRBM_STATUS,
                    expected: 0,
                    mask: BUSY_BIT_MASK,
                },
                RegisterVerify {
                    offset: GRBM_STATUS2,
                    expected: 0,
                    mask: BUSY_BIT_MASK,
                },
                RegisterVerify {
                    offset: SRBM_STATUS,
                    expected: 0,
                    mask: BUSY_BIT_MASK,
                },
            ],
        }]
    }

    fn boot0_offset(&self) -> usize {
        GRBM_STATUS
    }

    fn pmc_enable_offset(&self) -> usize {
        GRBM_SOFT_RESET
    }

    fn pbdma_map_offset(&self) -> Option<usize> {
        None
    }

    fn pramin_base_offset(&self) -> Option<usize> {
        None
    }

    fn bar2_block_offset(&self) -> Option<usize> {
        None
    }
}
