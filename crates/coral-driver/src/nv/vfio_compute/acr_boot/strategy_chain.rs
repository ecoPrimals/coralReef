// SPDX-License-Identifier: AGPL-3.0-only
//! Legacy ACR boot strategies — re-exports from focused submodules.
//!
//! Split for maintainability:
//! - [`strategy_chain_dma`]: DMA-backed BL chain
//! - [`strategy_chain_direct`]: Direct IMEM ACR load (no BL DMA)
//! - [`strategy_chain_pio`]: PIO with VRAM/sysmem WPR

pub use super::strategy_chain_dma::attempt_acr_chain;
pub use super::strategy_chain_direct::attempt_direct_acr_load;
pub use super::strategy_chain_pio::{
    attempt_pio_acr_with_sysmem_wpr, attempt_pio_acr_with_vram_wpr,
};
