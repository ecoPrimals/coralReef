// SPDX-License-Identifier: AGPL-3.0-only
//! AMD Vega/MI50 `GpuMetal` implementation stub.
//!
//! This will be filled when the MI50 HBM2 cards arrive. The register
//! layout follows AMD's MMIO documentation: GC (Graphics Core), SDMA,
//! UMC (Unified Memory Controller), THM (Thermal), CLK_MGR.
//!
//! For now, this provides the type and placeholder so the trait system
//! compiles and the vendor-agnostic pipeline is ready.

use super::bar_cartography::DomainHint;
use super::gpu_vendor::*;
use super::pci_discovery::GpuVendor;

/// AMD Vega 20 (MI50) identity placeholder.
#[derive(Debug, Clone)]
pub struct AmdVegaIdentity {
    /// Raw identity register value.
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
        "Vega"
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

/// AMD Vega `GpuMetal` stub — will be populated with real register offsets
/// when hardware is available for probing.
#[derive(Debug)]
pub struct AmdVegaMetal {
    identity: AmdVegaIdentity,
}

impl AmdVegaMetal {
    #[allow(missing_docs)]
    pub fn new(raw_id: u32) -> Self {
        Self {
            identity: AmdVegaIdentity { raw: raw_id },
        }
    }
}

impl GpuMetal for AmdVegaMetal {
    fn identity(&self) -> &dyn GpuIdentity {
        &self.identity
    }

    fn power_domains(&self) -> &[PowerDomain] {
        &[] // TODO: SMC, GRBM, SRBM power domains
    }

    fn memory_regions(&self) -> &[MetalMemoryRegion] {
        &[] // TODO: UMC (HBM2 controller), GC L2 cache
    }

    fn engine_list(&self) -> &[EngineInfo] {
        &[] // TODO: GFX, SDMA, VCN engines
    }

    fn register_domain(&self, _name: &str) -> Option<(usize, usize)> {
        None // TODO: AMD MMIO register domains
    }

    fn domain_hints(&self) -> Vec<DomainHint> {
        vec![] // TODO: AMD BAR0 domain map
    }

    fn warmup_sequence(&self) -> Vec<WarmupStep> {
        vec![] // TODO: AMD power-on sequence
    }

    fn boot0_offset(&self) -> usize {
        0x0 // AMD uses different identity registers
    }

    fn pmc_enable_offset(&self) -> usize {
        0x0 // AMD uses GRBM for engine control
    }

    fn pbdma_map_offset(&self) -> Option<usize> {
        None // AMD uses SDMA, not PBDMA
    }

    fn pramin_base_offset(&self) -> Option<usize> {
        None // AMD uses different VRAM access mechanism
    }

    fn bar2_block_offset(&self) -> Option<usize> {
        None // AMD uses different page table setup
    }
}
