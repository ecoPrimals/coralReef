// SPDX-License-Identifier: AGPL-3.0-or-later

/// Parameterized boot configuration for the SEC2 ACR boot matrix.
///
/// Each field controls one variable in the boot sequence. The 12-combination
/// matrix (Exp 110) sweeps these to find which achieve HS mode.
#[derive(Debug, Clone)]
pub struct BootConfig {
    /// `true` = correct GV100 upper-8-byte PDE slot, `false` = legacy lower-8
    pub pde_upper: bool,
    /// `true` = VRAM aperture PTEs for ACR code pages, `false` = all SYS_MEM
    pub acr_vram_pte: bool,
    /// `true` = zero blob_size so ACR skips its internal DMA (Exp 095 style)
    pub blob_size_zero: bool,
    /// `true` = VRAM bind target (0), `false` = SYS_MEM bind target (2)
    pub bind_vram: bool,
    /// `true` = PIO pre-load ACR code to IMEM before STARTCPU
    pub imem_preload: bool,
    /// `true` = flush TLB after binding
    pub tlb_invalidate: bool,
}

impl BootConfig {
    /// Exp 095 baseline: skip blob DMA, legacy PDEs, all SYS_MEM.
    pub fn exp095_baseline() -> Self {
        Self {
            pde_upper: false,
            acr_vram_pte: false,
            blob_size_zero: true,
            bind_vram: false,
            imem_preload: false,
            tlb_invalidate: false,
        }
    }

    /// Full-init path: correct PDEs, VRAM code PTEs, TLB flush.
    pub fn full_init() -> Self {
        Self {
            pde_upper: true,
            acr_vram_pte: true,
            blob_size_zero: false,
            bind_vram: false,
            imem_preload: false,
            tlb_invalidate: true,
        }
    }

    /// Short label for matrix output.
    pub fn label(&self) -> String {
        format!(
            "pde={} vram_pte={} blob0={} bind={} imem={} tlb={}",
            if self.pde_upper { "upper" } else { "lower" },
            self.acr_vram_pte,
            self.blob_size_zero,
            if self.bind_vram { "VRAM" } else { "SYS" },
            self.imem_preload,
            self.tlb_invalidate,
        )
    }
}

impl std::fmt::Display for BootConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}
