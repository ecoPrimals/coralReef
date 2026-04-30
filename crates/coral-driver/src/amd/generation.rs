// SPDX-License-Identifier: AGPL-3.0-or-later
//! AMD GPU generation profiles — single source of truth for per-generation knowledge.
//!
//! Parallel to the NVIDIA [`nv::generation::GenerationProfile`](crate::nv::generation::GenerationProfile),
//! this module centralizes all AMD-specific per-generation hardware knowledge
//! into [`AmdGenerationProfile`]. All code that previously branched on raw
//! `gfx_major` numbers should consult [`profile_for_gfx`] instead.
//!
//! Adding a new AMD generation = one new `const AmdGenerationProfile`, zero
//! new match arms scattered across the codebase.

use crate::hardware::MemoryType;

/// PM4 cache coherence method.
///
/// GFX9 uses `CP_COHER_CNTL` fields; GFX10+ uses the GCR (Global Cache
/// Register) bits in `ACQUIRE_MEM`. This determines which PM4 encoding
/// path to use for pre-dispatch invalidation and post-dispatch writeback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMethod {
    /// GFX9 (GCN5): `CP_COHER_CNTL` with `TC_ACTION_ENA`, `TCL1_ACTION_ENA`, `TC_WB_ACTION_ENA`.
    CpCoher,
    /// GFX10+ (RDNA): GCR bits (`GL2_INV`, `GL2_WB`, `GL1_INV`, etc.) in `ACQUIRE_MEM` body.
    Gcr,
}

/// Consolidated per-generation AMD GPU knowledge.
///
/// Every property that varies by AMD GPU generation is collected here.
/// Use [`profile_for_gfx`] to look up the profile for a given GFX major version.
#[derive(Debug, Clone)]
pub struct AmdGenerationProfile {
    /// Human-readable generation name.
    pub name: &'static str,
    /// GFX IP major version (9=GCN5/Vega, 10=RDNA1/2, 11=RDNA3, 12=RDNA4).
    pub gfx_major: u8,
    /// Native wavefront width (64 for GCN, 32 for RDNA wave32).
    pub wave_size: u32,
    /// VGPR allocation granularity for `COMPUTE_PGM_RSRC1` encoding.
    /// 4 for GCN5 wave64, 8 for RDNA wave32.
    pub vgpr_granularity: u32,
    /// Video memory technology.
    pub memory_type: MemoryType,
    /// PM4 cache coherence encoding.
    pub cache_method: CacheMethod,
    /// RDNA WGP mode (`COMPUTE_PGM_RSRC1` bit 29): use full Work Group Processor.
    pub has_wgp_mode: bool,
    /// `COMPUTE_PGM_RSRC1` bit 30: stores complete before S_ENDPGM.
    pub has_mem_ordered: bool,
    /// Whether the ALU supports IEEE 754 binary64 natively.
    pub has_hardware_f64: bool,
    /// Maximum waves per shader engine (for `COMPUTE_RESOURCE_LIMITS`).
    pub max_waves_per_sh: u32,
}

/// GFX9 — GCN5 / Vega (MI50, MI60, Vega 56/64).
pub const GFX9: AmdGenerationProfile = AmdGenerationProfile {
    name: "GCN5 Vega",
    gfx_major: 9,
    wave_size: 64,
    vgpr_granularity: 4,
    memory_type: MemoryType::Hbm2,
    cache_method: CacheMethod::CpCoher,
    has_wgp_mode: false,
    has_mem_ordered: false,
    has_hardware_f64: true,
    max_waves_per_sh: 600,
};

/// GFX10 — RDNA 1/2 (RX 5700, RX 6800, MI100).
pub const GFX10: AmdGenerationProfile = AmdGenerationProfile {
    name: "RDNA2",
    gfx_major: 10,
    wave_size: 32,
    vgpr_granularity: 8,
    memory_type: MemoryType::Gddr6,
    cache_method: CacheMethod::Gcr,
    has_wgp_mode: true,
    has_mem_ordered: true,
    has_hardware_f64: true,
    max_waves_per_sh: 512,
};

/// GFX11 — RDNA 3 (RX 7900, MI300).
pub const GFX11: AmdGenerationProfile = AmdGenerationProfile {
    name: "RDNA3",
    gfx_major: 11,
    wave_size: 32,
    vgpr_granularity: 8,
    memory_type: MemoryType::Gddr6,
    cache_method: CacheMethod::Gcr,
    has_wgp_mode: true,
    has_mem_ordered: true,
    has_hardware_f64: true,
    max_waves_per_sh: 512,
};

/// GFX12 — RDNA 4 (RX 9070).
pub const GFX12: AmdGenerationProfile = AmdGenerationProfile {
    name: "RDNA4",
    gfx_major: 12,
    wave_size: 32,
    vgpr_granularity: 8,
    memory_type: MemoryType::Gddr6,
    cache_method: CacheMethod::Gcr,
    has_wgp_mode: true,
    has_mem_ordered: true,
    has_hardware_f64: false,
    max_waves_per_sh: 512,
};

/// All known AMD generation profiles.
const ALL_PROFILES: &[&AmdGenerationProfile] = &[&GFX9, &GFX10, &GFX11, &GFX12];

/// Look up the AMD generation profile for a given GFX major version.
///
/// Falls back to GFX10 (RDNA2) for unrecognized versions — the most
/// common contemporary AMD compute target.
#[must_use]
pub fn profile_for_gfx(major: u8) -> &'static AmdGenerationProfile {
    for profile in ALL_PROFILES {
        if profile.gfx_major == major {
            return profile;
        }
    }
    &GFX10
}

impl AmdGenerationProfile {
    /// Build vendor-agnostic [`HardwareCapabilities`] from this AMD profile.
    #[must_use]
    pub fn to_capabilities(&self) -> crate::HardwareCapabilities {
        use crate::hardware::{CompletionStyle, Vendor, WaveSize};
        crate::HardwareCapabilities {
            vendor: Vendor::Amd,
            device_name: self.name,
            generation_name: self.name,
            has_hardware_f64: self.has_hardware_f64,
            has_hardware_f64_rcp: self.has_hardware_f64,
            has_full_rate_fp64: false,
            native_wave_size: if self.wave_size == 64 {
                WaveSize::Wave64
            } else {
                WaveSize::Wave32
            },
            memory_type: self.memory_type,
            completion_style: CompletionStyle::DeviceFence,
            max_shared_mem_bytes: 65536,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gfx9_profile() {
        let p = profile_for_gfx(9);
        assert_eq!(p.name, "GCN5 Vega");
        assert_eq!(p.wave_size, 64);
        assert_eq!(p.vgpr_granularity, 4);
        assert_eq!(p.cache_method, CacheMethod::CpCoher);
        assert!(!p.has_wgp_mode);
        assert!(!p.has_mem_ordered);
        assert!(p.has_hardware_f64);
    }

    #[test]
    fn gfx10_profile() {
        let p = profile_for_gfx(10);
        assert_eq!(p.name, "RDNA2");
        assert_eq!(p.wave_size, 32);
        assert_eq!(p.vgpr_granularity, 8);
        assert_eq!(p.cache_method, CacheMethod::Gcr);
        assert!(p.has_wgp_mode);
        assert!(p.has_mem_ordered);
    }

    #[test]
    fn gfx11_profile() {
        let p = profile_for_gfx(11);
        assert_eq!(p.name, "RDNA3");
        assert_eq!(p.gfx_major, 11);
    }

    #[test]
    fn gfx12_profile() {
        let p = profile_for_gfx(12);
        assert_eq!(p.name, "RDNA4");
        assert!(!p.has_hardware_f64);
    }

    #[test]
    fn unknown_gfx_falls_back_to_rdna2() {
        let p = profile_for_gfx(99);
        assert_eq!(p.name, "RDNA2");
    }

    #[test]
    fn all_profiles_covered() {
        for major in [9, 10, 11, 12] {
            let p = profile_for_gfx(major);
            assert_eq!(p.gfx_major, major);
        }
    }

    #[test]
    fn capabilities_from_profile() {
        let caps = GFX9.to_capabilities();
        assert_eq!(caps.vendor, crate::hardware::Vendor::Amd);
        assert!(caps.has_hardware_f64);
        assert_eq!(caps.native_wave_size, crate::hardware::WaveSize::Wave64);

        let caps10 = GFX10.to_capabilities();
        assert_eq!(caps10.native_wave_size, crate::hardware::WaveSize::Wave32);
    }
}
