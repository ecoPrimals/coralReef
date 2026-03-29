// SPDX-License-Identifier: AGPL-3.0-only

//! VRAM-based ACR boot strategies.
//!
//! Two strategies:
//! - `attempt_vram_acr_boot`: Legacy physical DMA path (FBIF override, no bind).
//! - `attempt_vram_native_acr_boot`: **Exp 111** — VRAM page tables + virtual DMA
//!   via instance block bind. Addresses the HS+MMU paradox from Exp 110.

mod dual_phase;
mod legacy_acr;
mod native;
mod pramin_write;

pub use dual_phase::{DualPhaseConfig, attempt_dual_phase_boot, attempt_dual_phase_boot_cfg};
pub use legacy_acr::attempt_vram_acr_boot;
pub use native::attempt_vram_native_acr_boot;
