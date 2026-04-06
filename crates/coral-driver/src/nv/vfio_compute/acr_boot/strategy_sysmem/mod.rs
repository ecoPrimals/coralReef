// SPDX-License-Identifier: AGPL-3.0-or-later

//! System-memory ACR boot (IOMMU DMA).

mod boot_config;
mod sysmem_impl;

pub use boot_config::BootConfig;

use crate::vfio::device::{DmaBackend, MappedBar};

use super::boot_result::AcrBootResult;
use super::firmware::AcrFirmwareSet;

/// System-memory ACR boot with skip-blob-DMA (Exp 095 style).
///
/// blob_size is zeroed so the ACR firmware skips its internal blob DMA.
/// This achieves HS mode but causes the firmware to exit immediately.
pub fn attempt_sysmem_acr_boot(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    container: DmaBackend,
) -> AcrBootResult {
    let config = BootConfig {
        pde_upper: true,
        acr_vram_pte: false,
        blob_size_zero: true,
        bind_vram: false,
        imem_preload: false,
        tlb_invalidate: true,
    };
    sysmem_impl::attempt_sysmem_acr_boot_inner(bar0, fw, container, &config)
}

/// System-memory ACR boot with blob DMA enabled — firmware attempts full init.
pub fn attempt_sysmem_acr_boot_full(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    container: DmaBackend,
) -> AcrBootResult {
    sysmem_impl::attempt_sysmem_acr_boot_inner(bar0, fw, container, &BootConfig::full_init())
}

/// System-memory ACR boot with caller-supplied configuration.
pub fn attempt_sysmem_acr_boot_with_config(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    container: DmaBackend,
    config: &BootConfig,
) -> AcrBootResult {
    sysmem_impl::attempt_sysmem_acr_boot_inner(bar0, fw, container, config)
}
