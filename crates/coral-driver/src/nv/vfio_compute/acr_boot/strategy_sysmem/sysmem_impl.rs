// SPDX-License-Identifier: AGPL-3.0-or-later

//! Orchestrates system-memory ACR boot: probe → allocate → WPR/MMU → SEC2 run.

use crate::vfio::device::{DmaBackend, MappedBar};

use super::super::boot_result::AcrBootResult;
use super::super::firmware::AcrFirmwareSet;
use super::super::sec2_hal::Sec2Probe;
use super::boot_config::BootConfig;
use super::sysmem_boot_finish;
use super::sysmem_prepare;
use super::sysmem_wpr_mmu;

/// Full sysmem ACR boot sequence (IOMMU-backed DMA, Nouveau-style SEC2 bring-up).
pub(super) fn attempt_sysmem_acr_boot_inner(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    container: DmaBackend,
    config: &BootConfig,
) -> AcrBootResult {
    let mut notes = Vec::new();
    let skip_blob_dma = config.blob_size_zero;
    notes.push(format!("BootConfig: {config}"));
    let sec2_before = Sec2Probe::capture(bar0);
    notes.push(format!("SEC2 state: {:?}", sec2_before.state));

    sysmem_prepare::probe_vram_and_mc(bar0, &mut notes);

    let parsed = match sysmem_prepare::parse_acr_firmware(fw, &mut notes, &sec2_before, bar0) {
        Ok(p) => p,
        Err(e) => return e,
    };
    sysmem_prepare::push_firmware_layout_notes(&parsed, &mut notes);

    let mut dma = match sysmem_prepare::allocate_dma(
        container,
        fw,
        &parsed,
        &mut notes,
        &sec2_before,
        bar0,
    ) {
        Ok(d) => d,
        Err(e) => return e,
    };

    let payload_patched = sysmem_wpr_mmu::fill_wpr_patch_acr_and_setup_mmu(
        bar0,
        &mut dma,
        &parsed,
        config,
        &mut notes,
        skip_blob_dma,
    );

    sysmem_boot_finish::sec2_reset_bind_load_and_poll(
        bar0,
        &mut dma,
        &parsed,
        &payload_patched,
        config,
        &mut notes,
        sec2_before,
    )
}
