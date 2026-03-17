// SPDX-License-Identifier: AGPL-3.0-only
//! AMD Vega 20 (MI50/MI60, GFX906) `GpuMetal` implementation.
//!
//! Register offsets derived from AMD's publicly documented MMIO layout
//! for Vega architecture. Key subsystems:
//!
//! - **GRBM**: Graphics Request Broker Manager (engine status, soft reset)
//! - **SRBM**: System Request Broker Manager (system-level status)
//! - **UMC**: Unified Memory Controller (HBM2 interface)
//! - **SDMA**: System DMA engines (memory copy, fill)
//! - **GC**: Graphics Core (compute dispatch, shader execution)
//! - **MMHUB**: Memory Management Hub (GART, page tables)

use super::bar_cartography::DomainHint;
use super::gpu_vendor::*;
use super::pci_discovery::GpuVendor;

// ── AMD Vega 20 MMIO Register Offsets ────────────────────────────────

const GRBM_STATUS: usize = 0x8010;
const GRBM_SOFT_RESET: usize = 0x8020;
const SRBM_STATUS: usize = 0x0E50;
const CP_STAT: usize = 0x8680;
const SDMA0_BASE: usize = 0x4D00;
const SDMA1_BASE: usize = 0x5900;
const RLC_BASE: usize = 0xEC00;
const MMHUB_VM_BASE: usize = 0x0600;
const MC_VM_FB_LOCATION_BASE: usize = 0x0928;

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
                    name: "GFX",
                    enable_reg: Some(GRBM_SOFT_RESET),
                    enable_bit: Some(0x01),
                    clock_reg: None,
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
            "GRBM" => Some((0x8000, 0x8FFF)),
            "SRBM" => Some((0x0E00, 0x0EFF)),
            "CP" => Some((0x8600, 0x86FF)),
            "SDMA0" => Some((0x4D00, 0x4DFF)),
            "SDMA1" => Some((0x5900, 0x59FF)),
            "RLC" => Some((0xEC00, 0xECFF)),
            "MMHUB" => Some((0x0600, 0x0AFF)),
            "GC" => Some((0x2000, 0x3FFF)),
            _ => None,
        }
    }

    fn domain_hints(&self) -> Vec<DomainHint> {
        vec![
            DomainHint {
                name: "GRBM",
                start: 0x8000,
                end: 0x8FFF,
            },
            DomainHint {
                name: "SRBM",
                start: 0x0E00,
                end: 0x0EFF,
            },
            DomainHint {
                name: "CP",
                start: 0x8600,
                end: 0x86FF,
            },
            DomainHint {
                name: "SDMA0",
                start: 0x4D00,
                end: 0x4DFF,
            },
            DomainHint {
                name: "SDMA1",
                start: 0x5900,
                end: 0x59FF,
            },
            DomainHint {
                name: "RLC",
                start: RLC_BASE,
                end: 0xECFF,
            },
            DomainHint {
                name: "MMHUB",
                start: MMHUB_VM_BASE,
                end: 0x0AFF,
            },
        ]
    }

    fn warmup_sequence(&self) -> Vec<WarmupStep> {
        vec![WarmupStep {
            description: "Read GRBM and SRBM status registers",
            writes: vec![],
            delay_ms: 0,
            verify: vec![
                RegisterVerify {
                    offset: GRBM_STATUS,
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
