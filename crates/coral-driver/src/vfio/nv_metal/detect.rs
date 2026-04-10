// SPDX-License-Identifier: AGPL-3.0-or-later

use super::metal::NvVoltaMetal;

use super::super::gpu_vendor::GpuMetal;
use super::super::pci_discovery::GpuVendor;

/// Detect which `GpuMetal` implementation to use from a BOOT0 value.
///
/// Returns `Some(metal)` for supported NVIDIA architectures (Volta and later)
/// and AMD GFX906 (Vega 20 / MI50/MI60). Returns `None` for Intel and other
/// vendors. Future: add Turing, Ampere, Ada variants; Intel Arc/Xe.
pub fn detect_gpu_metal(vendor: GpuVendor, boot0: u32) -> Option<Box<dyn GpuMetal>> {
    match vendor {
        GpuVendor::Nvidia => {
            let arch_code = ((boot0 >> 20) & 0x1FF) as u16;
            match arch_code {
                0x140..=0x14F => Some(Box::new(NvVoltaMetal::from_boot0(boot0))),
                // Future: Turing, Ampere, Ada...
                _ => Some(Box::new(NvVoltaMetal::from_boot0(boot0))),
            }
        }
        GpuVendor::Amd => {
            // EVOLUTION: Register offsets from AMD ISA docs — awaiting MI50 hardware validation.
            // GFX906 (Vega 20 / MI50/MI60) metal with SMC, GRBM, UMC, GFX power domains.
            Some(Box::new(super::super::amd_metal::AmdVegaMetal::new(boot0)))
        }
        _ => None,
    }
}
