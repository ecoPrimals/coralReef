// SPDX-License-Identifier: AGPL-3.0-or-later

/// BOOTVEC offsets for FECS/GPCCS, derived from firmware metadata.
///
/// When ACR loads firmware via DMA, it should set BOOTVEC to the BL entry
/// point. If it doesn't (BOOTVEC reads as 0), the falcon starts executing
/// at `IMEM[0]` instead of the BL entry — immediate exception. These offsets
/// are passed from `AcrFirmwareSet`'s parsed [`GrBlFirmware`](crate::nv::vfio_compute::acr_boot::firmware::GrBlFirmware) metadata.
pub struct FalconBootvecOffsets {
    /// GPCCS BL IMEM byte offset (typically `0x3400` for GV100).
    pub gpccs: u32,
    /// FECS BL IMEM byte offset (typically `0x7E00` for GV100).
    pub fecs: u32,
}
