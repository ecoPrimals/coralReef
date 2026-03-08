// SPDX-License-Identifier: AGPL-3.0-only
//! GPU architecture targets — vendor-agnostic.
//!
//! [`GpuTarget`] is the top-level enum discriminating between GPU vendors.
//! Each vendor has its own architecture enum ([`NvArch`], [`AmdArch`],
//! [`IntelArch`]) that describes specific hardware generations.
//!
//! [`GpuArch`] is a convenience alias for [`NvArch`] to ease the
//! transition from the original NVIDIA-only codebase.

// ---------------------------------------------------------------------------
// Vendor-agnostic target
// ---------------------------------------------------------------------------

/// A GPU compilation target, discriminated by vendor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GpuTarget {
    /// NVIDIA GPU architecture (SM70+).
    Nvidia(NvArch),
    /// AMD GPU architecture (placeholder for future backend).
    Amd(AmdArch),
    /// Intel GPU architecture (placeholder for future backend).
    Intel(IntelArch),
}

impl Default for GpuTarget {
    fn default() -> Self {
        Self::Nvidia(NvArch::default())
    }
}

impl GpuTarget {
    /// The vendor name for this target.
    #[must_use]
    pub const fn vendor(&self) -> &'static str {
        match self {
            Self::Nvidia(_) => "nvidia",
            Self::Amd(_) => "amd",
            Self::Intel(_) => "intel",
        }
    }

    /// Unwrap as [`NvArch`], or `None` if this is a different vendor.
    #[must_use]
    pub const fn as_nvidia(&self) -> Option<NvArch> {
        match self {
            Self::Nvidia(arch) => Some(*arch),
            _ => None,
        }
    }

    /// Unwrap as [`AmdArch`], or `None` if this is a different vendor.
    #[must_use]
    pub const fn as_amd(&self) -> Option<AmdArch> {
        match self {
            Self::Amd(arch) => Some(*arch),
            _ => None,
        }
    }

    /// Unwrap as [`IntelArch`], or `None` if this is a different vendor.
    #[must_use]
    pub const fn as_intel(&self) -> Option<IntelArch> {
        match self {
            Self::Intel(arch) => Some(*arch),
            _ => None,
        }
    }
}

impl std::fmt::Display for GpuTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nvidia(arch) => write!(f, "{arch}"),
            Self::Amd(arch) => write!(f, "{arch}"),
            Self::Intel(arch) => write!(f, "{arch}"),
        }
    }
}

impl From<NvArch> for GpuTarget {
    fn from(arch: NvArch) -> Self {
        Self::Nvidia(arch)
    }
}

impl From<AmdArch> for GpuTarget {
    fn from(arch: AmdArch) -> Self {
        Self::Amd(arch)
    }
}

impl From<IntelArch> for GpuTarget {
    fn from(arch: IntelArch) -> Self {
        Self::Intel(arch)
    }
}

// ---------------------------------------------------------------------------
// NVIDIA architectures
// ---------------------------------------------------------------------------

/// NVIDIA GPU architecture (Shader Model).
///
/// This is also exported as [`GpuArch`] for backward compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum NvArch {
    /// Volta (Titan V, GV100) — first with independent thread scheduling.
    #[default]
    Sm70,
    /// Turing (RTX 20xx) — tensor cores, RT cores.
    Sm75,
    /// Ampere (A100, RTX 30xx) — 2nd gen tensor cores.
    Sm80,
    /// GA106 (RTX 3060) — Ampere consumer.
    Sm86,
    /// Ada Lovelace (RTX 40xx) — 4th gen tensor cores.
    Sm89,
}

impl NvArch {
    /// All supported NVIDIA architectures, ordered by SM version.
    pub const ALL: &[Self] = &[Self::Sm70, Self::Sm75, Self::Sm80, Self::Sm86, Self::Sm89];

    /// Parse an architecture string (`"sm_70"`, `"sm70"`, etc.).
    ///
    /// Returns `None` if the string doesn't match any known architecture.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|a| {
                let sm = a.sm();
                s == format!("sm_{sm}") || s == format!("sm{sm}")
            })
            .copied()
    }

    /// Shader model number.
    #[must_use]
    pub const fn sm(self) -> u32 {
        match self {
            Self::Sm70 => 70,
            Self::Sm75 => 75,
            Self::Sm80 => 80,
            Self::Sm86 => 86,
            Self::Sm89 => 89,
        }
    }

    /// Shader model version as u8 (for `ShaderModelInfo`, etc.).
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "sm() is 70-89, always fits in u8"
    )]
    pub const fn sm_version(self) -> u8 {
        self.sm() as u8
    }

    /// Whether this arch supports DFMA (double-precision FMA) natively.
    #[must_use]
    pub const fn has_dfma(self) -> bool {
        true // All SM70+ support DFMA (DADD, DFMA, DMUL)
    }

    /// Whether this arch has fast f64 throughput (1:2 vs 1:32 of f32).
    #[must_use]
    pub const fn has_fast_fp64(self) -> bool {
        matches!(self, Self::Sm70 | Self::Sm80)
    }

    /// Hardware f64 transcendental seed availability (rcp64h / rsq64h).
    #[must_use]
    pub const fn has_transcendental_64h(self) -> bool {
        true // All SM70+ have RCP64H and RSQ64H
    }

    /// Maximum registers per thread.
    #[must_use]
    pub const fn max_reg_count(self) -> u32 {
        255
    }

    /// Maximum shared memory per block (bytes).
    #[must_use]
    pub const fn max_shared_mem(self) -> u32 {
        match self {
            Self::Sm70 | Self::Sm75 => 49_152,
            Self::Sm80 | Self::Sm86 | Self::Sm89 => 102_400,
        }
    }

    /// Maximum concurrent warps per SM for this architecture.
    #[must_use]
    pub const fn max_warps_per_sm(self) -> u32 {
        match self {
            Self::Sm70 | Self::Sm80 => 64,
            Self::Sm75 => 32,
            Self::Sm86 | Self::Sm89 => 48,
        }
    }

    /// Total register file size (32-bit registers per SM).
    #[must_use]
    pub const fn total_reg_file(self) -> u32 {
        65_536
    }

    /// Warp size (threads per warp).
    #[must_use]
    pub const fn warp_size(self) -> u32 {
        32
    }
}

impl std::str::FromStr for NvArch {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| {
            let valid: Vec<String> = Self::ALL
                .iter()
                .map(std::string::ToString::to_string)
                .collect();
            format!("unknown architecture '{s}', valid: {}", valid.join(", "))
        })
    }
}

impl std::fmt::Display for NvArch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "sm_{}", self.sm())
    }
}

/// Backward-compatible alias.
pub type GpuArch = NvArch;

// ---------------------------------------------------------------------------
// AMD architectures (placeholder)
// ---------------------------------------------------------------------------

/// AMD GPU architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AmdArch {
    /// RDNA 2 (RX 6000 series — GFX1030+).
    Rdna2,
    /// RDNA 3 (RX 7000 series — GFX1100+).
    Rdna3,
    /// RDNA 4 (RX 9000 series).
    Rdna4,
}

impl AmdArch {
    /// All supported AMD architectures, ordered by generation.
    pub const ALL: &[Self] = &[Self::Rdna2, Self::Rdna3, Self::Rdna4];

    /// GFX version major number for this architecture.
    #[must_use]
    pub const fn gfx_major(self) -> u8 {
        match self {
            Self::Rdna2 => 10,
            Self::Rdna3 => 11,
            Self::Rdna4 => 12,
        }
    }

    /// Default wave size for this architecture.
    #[must_use]
    pub const fn default_wave_size(self) -> u8 {
        32
    }

    /// Whether this architecture supports wave64 execution.
    #[must_use]
    pub const fn supports_wave64(self) -> bool {
        true
    }

    /// Whether this architecture has native f64 instructions.
    #[must_use]
    pub const fn has_native_f64(self) -> bool {
        true
    }

    /// Native f64 rate relative to f32 (denominator: 1/N of f32 rate).
    #[must_use]
    pub const fn f64_rate_divisor(self) -> u32 {
        16
    }

    /// Maximum VGPRs per wave.
    #[must_use]
    pub const fn max_vgprs(self) -> u32 {
        256
    }

    /// Maximum SGPRs per wave.
    #[must_use]
    pub const fn max_sgprs(self) -> u32 {
        106
    }

    /// Maximum shared memory (LDS) per workgroup in bytes.
    #[must_use]
    pub const fn max_lds(self) -> u32 {
        65_536
    }

    /// Parse an architecture string (`"rdna2"`, `"gfx1030"`, etc.).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "rdna2" | "gfx1030" | "gfx1031" | "gfx1032" => Some(Self::Rdna2),
            "rdna3" | "gfx1100" | "gfx1101" | "gfx1102" => Some(Self::Rdna3),
            "rdna4" | "gfx1200" => Some(Self::Rdna4),
            _ => None,
        }
    }
}

impl std::str::FromStr for AmdArch {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| {
            let valid: Vec<&str> = Self::ALL
                .iter()
                .map(|a| match a {
                    Self::Rdna2 => "rdna2",
                    Self::Rdna3 => "rdna3",
                    Self::Rdna4 => "rdna4",
                })
                .collect();
            format!(
                "unknown AMD architecture '{s}', valid: {}",
                valid.join(", ")
            )
        })
    }
}

impl std::fmt::Display for AmdArch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rdna2 => write!(f, "rdna2"),
            Self::Rdna3 => write!(f, "rdna3"),
            Self::Rdna4 => write!(f, "rdna4"),
        }
    }
}

// ---------------------------------------------------------------------------
// Intel architectures (placeholder)
// ---------------------------------------------------------------------------

/// Intel GPU architecture (future).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum IntelArch {
    /// Xe-HPG (Arc A-series).
    XeHpg,
    /// Xe2-HPG (Battlemage).
    Xe2Hpg,
}

impl std::fmt::Display for IntelArch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::XeHpg => write!(f, "xe_hpg"),
            Self::Xe2Hpg => write!(f, "xe2_hpg"),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sm_numbers() {
        assert_eq!(NvArch::Sm70.sm(), 70);
        assert_eq!(NvArch::Sm75.sm(), 75);
        assert_eq!(NvArch::Sm80.sm(), 80);
        assert_eq!(NvArch::Sm86.sm(), 86);
        assert_eq!(NvArch::Sm89.sm(), 89);
    }

    #[test]
    fn test_gpu_target_vendor() {
        let nv = GpuTarget::Nvidia(NvArch::Sm70);
        assert_eq!(nv.vendor(), "nvidia");
        let amd = GpuTarget::Amd(AmdArch::Rdna2);
        assert_eq!(amd.vendor(), "amd");
        let intel = GpuTarget::Intel(IntelArch::XeHpg);
        assert_eq!(intel.vendor(), "intel");
    }

    #[test]
    fn test_gpu_target_default_is_nvidia() {
        let t = GpuTarget::default();
        assert!(t.as_nvidia().is_some());
        assert_eq!(t.as_nvidia(), Some(NvArch::Sm70));
    }

    #[test]
    fn test_gpu_target_from_nv_arch() {
        let t: GpuTarget = NvArch::Sm89.into();
        assert_eq!(t, GpuTarget::Nvidia(NvArch::Sm89));
    }

    #[test]
    fn test_gpu_target_display() {
        assert_eq!(GpuTarget::Nvidia(NvArch::Sm70).to_string(), "sm_70");
        assert_eq!(GpuTarget::Amd(AmdArch::Rdna2).to_string(), "rdna2");
        assert_eq!(GpuTarget::Amd(AmdArch::Rdna3).to_string(), "rdna3");
        assert_eq!(GpuTarget::Intel(IntelArch::XeHpg).to_string(), "xe_hpg");
    }

    #[test]
    fn test_gpu_arch_alias_works() {
        let a: GpuArch = GpuArch::Sm70;
        assert_eq!(a.sm(), 70);
    }

    #[test]
    fn test_nv_arch_parse() {
        assert_eq!(NvArch::parse("sm_70"), Some(NvArch::Sm70));
        assert_eq!(NvArch::parse("sm89"), Some(NvArch::Sm89));
        assert_eq!(NvArch::parse("rdna3"), None);
    }

    #[test]
    fn test_nv_arch_roundtrip() {
        for &arch in NvArch::ALL {
            let s = arch.to_string();
            assert_eq!(NvArch::parse(&s), Some(arch));
        }
    }

    #[test]
    fn test_fast_fp64() {
        assert!(NvArch::Sm70.has_fast_fp64());
        assert!(!NvArch::Sm75.has_fast_fp64());
        assert!(NvArch::Sm80.has_fast_fp64());
        assert!(!NvArch::Sm86.has_fast_fp64());
        assert!(!NvArch::Sm89.has_fast_fp64());
    }

    #[test]
    fn test_shared_mem() {
        assert_eq!(NvArch::Sm70.max_shared_mem(), 49_152);
        assert_eq!(NvArch::Sm80.max_shared_mem(), 102_400);
    }

    #[test]
    fn test_unwrap_helpers() {
        let nv = GpuTarget::Nvidia(NvArch::Sm80);
        assert!(nv.as_nvidia().is_some());
        assert!(nv.as_amd().is_none());
        assert!(nv.as_intel().is_none());

        let amd = GpuTarget::Amd(AmdArch::Rdna4);
        assert!(amd.as_nvidia().is_none());
        assert!(amd.as_amd().is_some());
    }

    #[test]
    fn test_amd_arch_parse() {
        assert_eq!(AmdArch::parse("rdna2"), Some(AmdArch::Rdna2));
        assert_eq!(AmdArch::parse("gfx1030"), Some(AmdArch::Rdna2));
        assert_eq!(AmdArch::parse("rdna3"), Some(AmdArch::Rdna3));
        assert_eq!(AmdArch::parse("gfx1100"), Some(AmdArch::Rdna3));
        assert_eq!(AmdArch::parse("rdna4"), Some(AmdArch::Rdna4));
        assert_eq!(AmdArch::parse("sm_70"), None);
    }

    #[test]
    fn test_amd_arch_roundtrip() {
        for &arch in AmdArch::ALL {
            let s = arch.to_string();
            assert_eq!(AmdArch::parse(&s), Some(arch));
        }
    }

    #[test]
    fn test_amd_arch_properties() {
        assert_eq!(AmdArch::Rdna2.gfx_major(), 10);
        assert_eq!(AmdArch::Rdna3.gfx_major(), 11);
        assert_eq!(AmdArch::Rdna4.gfx_major(), 12);
        assert!(AmdArch::Rdna2.has_native_f64());
        assert_eq!(AmdArch::Rdna2.f64_rate_divisor(), 16);
        assert_eq!(AmdArch::Rdna2.max_vgprs(), 256);
        assert_eq!(AmdArch::Rdna2.max_sgprs(), 106);
        assert_eq!(AmdArch::Rdna2.default_wave_size(), 32);
        assert!(AmdArch::Rdna2.supports_wave64());
        assert_eq!(AmdArch::Rdna2.max_lds(), 65_536);
    }

    #[test]
    fn test_nv_arch_hw_properties() {
        for &arch in NvArch::ALL {
            assert!(arch.has_dfma(), "{arch} should support DFMA");
        }

        assert_eq!(NvArch::Sm70.warp_size(), 32);
        assert_eq!(NvArch::Sm89.warp_size(), 32);

        assert_eq!(NvArch::Sm70.sm_version(), 70);
        assert_eq!(NvArch::Sm89.sm_version(), 89);

        assert!(NvArch::Sm70.max_reg_count() > 0);
        assert!(NvArch::Sm89.max_reg_count() >= NvArch::Sm70.max_reg_count());

        assert!(NvArch::Sm70.max_warps_per_sm() > 0);
        assert!(NvArch::Sm70.total_reg_file() > 0);
    }

    #[test]
    fn test_nv_arch_fromstr_error() {
        let result: Result<NvArch, _> = "not_a_gpu".parse();
        assert!(result.is_err());
        let err_msg = result.unwrap_err();
        assert!(err_msg.contains("unknown"));
    }

    #[test]
    fn test_amd_arch_fromstr_error() {
        let result: Result<AmdArch, _> = "not_a_gpu".parse();
        assert!(result.is_err());
        let err_msg = result.unwrap_err();
        assert!(err_msg.contains("unknown"));
    }

    #[test]
    fn test_gpu_target_from_amd() {
        let t: GpuTarget = AmdArch::Rdna3.into();
        assert_eq!(t, GpuTarget::Amd(AmdArch::Rdna3));
        assert!(t.as_amd().is_some());
    }

    #[test]
    fn test_gpu_target_from_intel() {
        let t: GpuTarget = IntelArch::XeHpg.into();
        assert_eq!(t, GpuTarget::Intel(IntelArch::XeHpg));
        assert!(t.as_intel().is_some());
    }

    #[test]
    fn test_intel_display() {
        assert_eq!(IntelArch::XeHpg.to_string(), "xe_hpg");
        assert_eq!(IntelArch::Xe2Hpg.to_string(), "xe2_hpg");
    }

    #[test]
    fn test_nv_arch_parse_both_formats() {
        assert_eq!(NvArch::parse("sm_70"), Some(NvArch::Sm70));
        assert_eq!(NvArch::parse("sm70"), Some(NvArch::Sm70));
    }

    #[test]
    fn test_amd_arch_parse_case_insensitive() {
        assert_eq!(AmdArch::parse("RDNA2"), Some(AmdArch::Rdna2));
        assert_eq!(AmdArch::parse("gfx1031"), Some(AmdArch::Rdna2));
    }

    #[test]
    fn test_nv_arch_has_transcendental_64h() {
        for &arch in NvArch::ALL {
            assert!(arch.has_transcendental_64h());
        }
    }

    #[test]
    fn test_nv_arch_max_warps_per_sm_variants() {
        assert_eq!(NvArch::Sm70.max_warps_per_sm(), 64);
        assert_eq!(NvArch::Sm75.max_warps_per_sm(), 32);
        assert_eq!(NvArch::Sm89.max_warps_per_sm(), 48);
    }

    #[test]
    fn test_gpu_target_vendor_display() {
        let nv = GpuTarget::Nvidia(NvArch::Sm70);
        assert_eq!(format!("{nv}"), "sm_70");
        let amd = GpuTarget::Amd(AmdArch::Rdna3);
        assert_eq!(format!("{amd}"), "rdna3");
    }

    #[test]
    fn test_amd_arch_gfx_variants() {
        assert_eq!(AmdArch::parse("gfx1032"), Some(AmdArch::Rdna2));
        assert_eq!(AmdArch::parse("gfx1101"), Some(AmdArch::Rdna3));
        assert_eq!(AmdArch::parse("gfx1102"), Some(AmdArch::Rdna3));
        assert_eq!(AmdArch::parse("gfx1200"), Some(AmdArch::Rdna4));
    }

    #[test]
    fn test_nv_arch_fromstr_valid() {
        let arch: NvArch = "sm_70".parse().unwrap();
        assert_eq!(arch, NvArch::Sm70);
    }

    #[test]
    fn test_amd_arch_fromstr_valid() {
        let arch: AmdArch = "rdna2".parse().unwrap();
        assert_eq!(arch, AmdArch::Rdna2);
    }
}
