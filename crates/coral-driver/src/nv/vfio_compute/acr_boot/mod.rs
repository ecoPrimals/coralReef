// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign SEC2/ACR falcon boot chain — the gateway to FECS.
//!
//! Three strategies for getting FECS/GPCCS running on GV100:
//!
//! 1. **EMEM boot** (cold VFIO, HS-locked SEC2): Write signed ACR bootloader
//!    into SEC2 EMEM, PMC-reset SEC2, ROM boots from EMEM, ACR loads FECS.
//!
//! 2. **Direct IMEM boot** (post-driver-reset, HS-cleared SEC2): Load ACR
//!    firmware directly into SEC2 IMEM/DMEM, set BOOTVEC, start CPU.
//!
//! 3. **Warm handoff** (nouveau oracle): nouveau boots everything, GlowPlug
//!    swaps to VFIO preserving state.
//!
//! Both EMEM and IMEM paths need a WPR (Write-Protected Region) in DMA memory
//! containing the FECS/GPCCS firmware images for ACR to load.
//!
//! ## Architecture
//!
//! ```text
//! Host builds WPR in DMA memory
//!   → SEC2 boots (via EMEM or IMEM path)
//!     → SEC2 runs ACR firmware
//!       → ACR reads WPR, verifies LS images
//!         → ACR DMA-loads FECS firmware into FECS IMEM
//!           → ACR releases FECS HRESET
//!             → FECS starts, signals mailbox0
//!               → GR engine ready for dispatch
//! ```

mod boot_diagnostics;
mod boot_result;
pub mod fecs_method;
mod firmware;
mod instance_block;
mod sec2_hal;
pub mod sec2_queue;
mod solver;
mod strategy_chain;
mod strategy_hybrid;
mod strategy_mailbox;
mod strategy_sysmem;
mod strategy_vram;
mod sysmem_iova;
mod wpr;

pub use boot_result::{AcrBootResult, BootJournal};
pub use firmware::{
    AcrFirmwareSet, GrBlFirmware, HsBlDesc, HsBlDescriptor, HsHeader, HsLoadHeader, NvFwBinHeader,
    ParsedAcrFirmware,
};
pub use instance_block::{
    FALCON_INST_VRAM, FALCON_PD0_VRAM, FALCON_PD1_VRAM, FALCON_PD2_VRAM, FALCON_PD3_VRAM,
    FALCON_PT0_VRAM, build_vram_falcon_inst_block, encode_bind_inst, encode_sysmem_pte,
    encode_vram_pde, falcon_bind_context,
};
pub use sec2_hal::{
    Sec2Probe, Sec2State, falcon_dmem_upload, falcon_engine_reset, falcon_imem_upload_nouveau,
    falcon_start_cpu, reset_sec2, sec2_emem_read, sec2_emem_verify, sec2_emem_write,
    sec2_exit_diagnostics, sec2_prepare_direct_boot, sec2_prepare_physical_first,
    sec2_tracepc_dump,
};
pub use solver::{BootStrategy, FalconBootSolver, FalconProbe, FecsState};
pub use strategy_chain::{attempt_acr_chain, attempt_direct_acr_load};
pub use strategy_hybrid::attempt_hybrid_acr_boot;
pub use strategy_mailbox::{
    FalconBootvecOffsets, attempt_acr_mailbox_command, attempt_direct_falcon_upload,
    attempt_direct_fecs_boot, attempt_direct_hreset, attempt_emem_boot, attempt_nouveau_boot,
};
pub use strategy_sysmem::{
    BootConfig, attempt_sysmem_acr_boot, attempt_sysmem_acr_boot_full,
    attempt_sysmem_acr_boot_with_config,
};
pub use strategy_vram::{
    DualPhaseConfig, attempt_dual_phase_boot, attempt_dual_phase_boot_cfg, attempt_vram_acr_boot,
    attempt_vram_native_acr_boot,
};
pub use wpr::{AcrDmaContext, build_bl_dmem_desc, build_wpr, falcon_id, patch_acr_desc};
