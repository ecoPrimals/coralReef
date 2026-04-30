// SPDX-License-Identifier: AGPL-3.0-or-later
//! Intel GPU generation profiles — single source of truth for per-generation knowledge.
//!
//! Parallel to AMD's [`amd::generation::AmdGenerationProfile`] and NVIDIA's
//! [`nv::generation::GenerationProfile`].

use crate::hardware::MemoryType;

/// Consolidated per-generation Intel GPU knowledge.
#[derive(Debug, Clone)]
pub struct IntelGenerationProfile {
    /// Human-readable generation name.
    pub name: &'static str,
    /// Generation number (12=Alchemist/DG2, 13=Battlemage/BMG).
    pub gfx_ver: u8,
    /// EU (Execution Unit) SIMD width.
    pub simd_width: u32,
    /// Video memory technology.
    pub memory_type: MemoryType,
    /// Whether the ALU supports IEEE 754 binary64 natively.
    pub has_hardware_f64: bool,
}

/// Gen 12.7 — Arc Alchemist (A770, A750, A380).
pub const ALCHEMIST: IntelGenerationProfile = IntelGenerationProfile {
    name: "Arc Alchemist (DG2)",
    gfx_ver: 12,
    simd_width: 16,
    memory_type: MemoryType::Gddr6,
    has_hardware_f64: false,
};

/// Gen 13 — Battlemage (B580, B570).
pub const BATTLEMAGE: IntelGenerationProfile = IntelGenerationProfile {
    name: "Battlemage (BMG)",
    gfx_ver: 13,
    simd_width: 16,
    memory_type: MemoryType::Gddr6,
    has_hardware_f64: false,
};

const ALL_PROFILES: &[&IntelGenerationProfile] = &[&ALCHEMIST, &BATTLEMAGE];

/// Look up the Intel generation profile.
///
/// Falls back to Alchemist (Gen 12) for unrecognized versions.
#[must_use]
pub fn profile_for_gen(gfx_ver: u8) -> &'static IntelGenerationProfile {
    for profile in ALL_PROFILES {
        if profile.gfx_ver == gfx_ver {
            return profile;
        }
    }
    &ALCHEMIST
}

impl IntelGenerationProfile {
    /// Build vendor-agnostic [`HardwareCapabilities`] from this Intel profile.
    #[must_use]
    pub fn to_capabilities(&self) -> crate::HardwareCapabilities {
        use crate::hardware::{CompletionStyle, Vendor, WaveSize};
        crate::HardwareCapabilities {
            vendor: Vendor::Intel,
            device_name: self.name,
            generation_name: self.name,
            has_hardware_f64: self.has_hardware_f64,
            has_hardware_f64_rcp: false,
            has_full_rate_fp64: false,
            native_wave_size: WaveSize::Wave32,
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
    fn alchemist_profile() {
        let p = profile_for_gen(12);
        assert_eq!(p.name, "Arc Alchemist (DG2)");
        assert_eq!(p.simd_width, 16);
        assert!(!p.has_hardware_f64);
    }

    #[test]
    fn battlemage_profile() {
        let p = profile_for_gen(13);
        assert_eq!(p.name, "Battlemage (BMG)");
    }

    #[test]
    fn unknown_falls_back_to_alchemist() {
        let p = profile_for_gen(99);
        assert_eq!(p.gfx_ver, 12);
    }

    #[test]
    fn capabilities_from_profile() {
        let caps = ALCHEMIST.to_capabilities();
        assert_eq!(caps.vendor, crate::hardware::Vendor::Intel);
        assert!(!caps.has_hardware_f64);
    }
}
