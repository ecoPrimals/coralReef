// SPDX-License-Identifier: AGPL-3.0-or-later
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
//!
//! ## GV100 (Titan V) ACR Audit — May 2026
//!
//! ### Firmware Status
//! All required firmware blobs are present at `/lib/firmware/nvidia/gv100/`:
//! - `acr/bl.bin` (symlink to gp102), `acr/ucode_load.bin`
//! - `sec2/desc.bin`, `sec2/image.bin`, `sec2/sig.bin`
//! - `gr/fecs_bl.bin`, `gr/fecs_inst.bin`, `gr/fecs_data.bin`, `gr/fecs_sig.bin`
//! - `gr/gpccs_bl.bin` (symlink to gp107), `gr/gpccs_inst.bin`, `gr/gpccs_data.bin`, `gr/gpccs_sig.bin`
//!
//! ### SEC2 HAL Status
//! - `Sec2Probe::capture()`: probes `CPUCTL`, `SCTL`, `BOOTVEC`, `HWCFG`, mailboxes
//! - `reset_sec2()`: PMC bit discovery + engine reset
//! - `falcon_engine_reset()`: per-falcon ENGCTL-based reset
//! - `falcon_imem_upload_nouveau()`: PIO IMEM upload with correct BIT(24) format
//! - `falcon_dmem_upload()`: PIO DMEM upload
//! - `sec2_emem_write/read/verify`: EMEM (extended memory) access
//! - `falcon_start_cpu()`: BOOTVEC + STARTCPU with ALIAS_EN awareness
//!
//! ### GV100 Key Properties (CPU-RM generation)
//! - **No hardware WPR barriers**: ACR constructs a "virtual WPR" in sysmem DMA
//! - **SEC2 in LS mode**: `SCTL` reports `0x3000` (fuse-enforced LS) — PIO works
//! - **FECS/GPCCS in HS mode**: require SEC2-authenticated boot via ACR chain
//! - **PMU firmware missing in nouveau**: fundamental limitation for long-running
//!   PM, but not blocking for FECS boot (SEC2 is the relevant falcon)
//!
//! ### Current Strategies Tried (from solver cascade)
//! 1. Nouveau-style SEC2 boot — STATUS: attempts but SEC2 does not signal completion
//! 2. Physical-first SEC2 boot — STATUS: attempts, same stall
//! 3. VRAM-based ACR boot — STATUS: PRAMIN writes land, SEC2 stalls
//! 4. System-memory ACR boot — STATUS: DMA setup succeeds, SEC2 stalls at PC=0
//! 5. Various fallbacks (5–9) — STATUS: none succeed
//!
//! ### Remaining Investigation
//! - SEC2 stalls: mailbox0 never transitions, PC stays at 0 or very low offset.
//!   Likely cause: BL DMEM descriptor mismatch (code/data base addresses may need
//!   adjustment for the DMA mode and instance block binding).
//! - The `BootConfig` matrix (12 combinations) has been swept; `pde_upper=true`
//!   achieves HS mode but ACR exits without loading FECS when `blob_size_zero=true`.
//!   With `blob_size_zero=false`, the firmware attempts internal DMA but stalls —
//!   likely needs correct WPR region physical addresses in the ACR blob descriptor.
//! - Warm handoff (nouveau boots FECS → ember swap) remains the validated interim.

mod boot_diagnostics;
mod boot_result;
pub mod fecs_method;
mod firmware;
mod instance_block;
pub mod nvdec_scrubber;
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
    Sec2Probe, Sec2State, falcon_configure_fbif_with_instance_block, falcon_dmem_upload,
    falcon_engine_reset, falcon_imem_upload_nouveau, falcon_start_cpu, reset_sec2,
    sec2_emem_read, sec2_emem_verify, sec2_emem_write, sec2_exit_diagnostics,
    sec2_prepare_direct_boot, sec2_prepare_physical_first, sec2_tracepc_dump,
};
pub use solver::{BootStrategy, FalconBootSolver, FalconProbe, FecsState, GpuGeneration};
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
