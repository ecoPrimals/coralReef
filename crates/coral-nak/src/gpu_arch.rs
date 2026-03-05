// SPDX-License-Identifier: AGPL-3.0-only
//! GPU architecture targets.

/// NVIDIA GPU architecture (Shader Model).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GpuArch {
    /// Volta (Titan V, GV100) — first with independent thread scheduling.
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

impl GpuArch {
    /// All supported architectures, ordered by SM version.
    pub const ALL: &[GpuArch] = &[Self::Sm70, Self::Sm75, Self::Sm80, Self::Sm86, Self::Sm89];

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

    /// Shader model version as u8 (for ShaderModelInfo, etc.).
    #[must_use]
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

    /// MUFU.RCP64H / MUFU.RSQ64H availability (initial f64 approximation).
    #[must_use]
    pub const fn has_mufu_64h(self) -> bool {
        true // All SM70+ have RCP64H and RSQ64H
    }

    /// Maximum registers per thread.
    #[must_use]
    pub const fn max_regs(self) -> u32 {
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
}

impl Default for GpuArch {
    fn default() -> Self {
        Self::Sm70
    }
}

impl std::str::FromStr for GpuArch {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| {
            let valid: Vec<String> = Self::ALL.iter().map(|a| a.to_string()).collect();
            format!("unknown architecture '{s}', valid: {}", valid.join(", "))
        })
    }
}

impl std::fmt::Display for GpuArch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "sm_{}", self.sm())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_archs() -> &'static [GpuArch] {
        GpuArch::ALL
    }

    #[test]
    fn test_sm_numbers() {
        assert_eq!(GpuArch::Sm70.sm(), 70);
        assert_eq!(GpuArch::Sm75.sm(), 75);
        assert_eq!(GpuArch::Sm80.sm(), 80);
        assert_eq!(GpuArch::Sm86.sm(), 86);
        assert_eq!(GpuArch::Sm89.sm(), 89);
    }

    #[test]
    fn test_sm_version() {
        assert_eq!(GpuArch::Sm70.sm_version(), 70);
        assert_eq!(GpuArch::Sm75.sm_version(), 75);
        assert_eq!(GpuArch::Sm80.sm_version(), 80);
        assert_eq!(GpuArch::Sm86.sm_version(), 86);
        assert_eq!(GpuArch::Sm89.sm_version(), 89);
    }

    #[test]
    fn test_all_have_dfma() {
        for &arch in all_archs() {
            assert!(arch.has_dfma(), "{arch} should have DFMA");
        }
    }

    #[test]
    fn test_all_have_mufu_64h() {
        for &arch in all_archs() {
            assert!(arch.has_mufu_64h(), "{arch} should have MUFU 64H");
        }
    }

    #[test]
    fn test_fast_fp64() {
        assert!(GpuArch::Sm70.has_fast_fp64());
        assert!(!GpuArch::Sm75.has_fast_fp64());
        assert!(GpuArch::Sm80.has_fast_fp64());
        assert!(!GpuArch::Sm86.has_fast_fp64());
        assert!(!GpuArch::Sm89.has_fast_fp64());
    }

    #[test]
    fn test_max_regs() {
        for &arch in all_archs() {
            assert_eq!(arch.max_regs(), 255);
        }
    }

    #[test]
    fn test_shared_mem_volta_turing() {
        assert_eq!(GpuArch::Sm70.max_shared_mem(), 49_152);
        assert_eq!(GpuArch::Sm75.max_shared_mem(), 49_152);
    }

    #[test]
    fn test_shared_mem_ampere_plus() {
        assert_eq!(GpuArch::Sm80.max_shared_mem(), 102_400);
        assert_eq!(GpuArch::Sm86.max_shared_mem(), 102_400);
        assert_eq!(GpuArch::Sm89.max_shared_mem(), 102_400);
    }

    #[test]
    fn test_display_all() {
        assert_eq!(GpuArch::Sm70.to_string(), "sm_70");
        assert_eq!(GpuArch::Sm75.to_string(), "sm_75");
        assert_eq!(GpuArch::Sm80.to_string(), "sm_80");
        assert_eq!(GpuArch::Sm86.to_string(), "sm_86");
        assert_eq!(GpuArch::Sm89.to_string(), "sm_89");
    }

    #[test]
    fn test_clone_copy_eq_hash() {
        use std::collections::HashSet;

        let a = GpuArch::Sm70;
        let b = a;
        assert_eq!(a, b);

        let mut set = HashSet::new();
        set.insert(GpuArch::Sm70);
        set.insert(GpuArch::Sm70);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn test_debug() {
        assert_eq!(format!("{:?}", GpuArch::Sm70), "Sm70");
    }

    #[test]
    fn test_default() {
        let d = GpuArch::default();
        assert_eq!(d, GpuArch::Sm70);
    }

    #[test]
    fn test_all_matches_known_archs() {
        assert_eq!(GpuArch::ALL.len(), 5);
        assert_eq!(GpuArch::ALL[0], GpuArch::Sm70);
        assert_eq!(GpuArch::ALL[4], GpuArch::Sm89);
    }

    #[test]
    fn test_parse_underscore_format() {
        for &arch in all_archs() {
            let s = format!("sm_{}", arch.sm());
            assert_eq!(GpuArch::parse(&s), Some(arch));
        }
    }

    #[test]
    fn test_parse_compact_format() {
        for &arch in all_archs() {
            let s = format!("sm{}", arch.sm());
            assert_eq!(GpuArch::parse(&s), Some(arch));
        }
    }

    #[test]
    fn test_parse_invalid() {
        assert_eq!(GpuArch::parse("sm_99"), None);
        assert_eq!(GpuArch::parse(""), None);
        assert_eq!(GpuArch::parse("cuda"), None);
    }

    #[test]
    fn test_from_str_valid() {
        let arch: GpuArch = "sm_70".parse().unwrap();
        assert_eq!(arch, GpuArch::Sm70);
    }

    #[test]
    fn test_from_str_invalid_has_valid_list() {
        let err = "sm_99".parse::<GpuArch>().unwrap_err();
        assert!(
            err.contains("sm_70"),
            "error should list valid archs: {err}"
        );
    }

    #[test]
    fn test_roundtrip_display_parse() {
        for &arch in all_archs() {
            let s = arch.to_string();
            assert_eq!(GpuArch::parse(&s), Some(arch));
        }
    }
}
