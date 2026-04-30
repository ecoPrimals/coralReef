// SPDX-License-Identifier: AGPL-3.0-or-later

use super::metal::NvVoltaMetal;

use super::super::gpu_vendor::GpuMetal;
use super::super::pci_discovery::GpuVendor;

/// Detect which `GpuMetal` implementation to use from a BOOT0 value.
///
/// Lives under [`super::metal`] (`nv_metal`) for VFIO bootstrap layout but selects
/// implementations by vendor: NVIDIA BAR0 decode vs AMD `amd_metal`. Returns `Some(metal)`
/// for supported NVIDIA architectures (Volta and later) and AMD GFX906 (Vega 20 / MI50/MI60).
/// Returns `None` for Intel and other vendors. Future: Turing, Ampere, Ada variants; Intel Arc/Xe.
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
            // EVOLUTION (multi-vendor): AMD path delegates to `amd_metal`; register offsets come
            // from AMD ISA documentation (GFX906 / Vega 20), not NVIDIA NV/mmio maps.
            Some(Box::new(super::super::amd_metal::AmdVegaMetal::new(boot0)))
        }
        _ => None,
    }
}
