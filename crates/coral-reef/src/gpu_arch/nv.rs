// SPDX-License-Identifier: AGPL-3.0-or-later
//! NVIDIA GPU architectures (Shader Model).

/// NVIDIA GPU architecture (Shader Model).
///
/// This is also exported as [`GpuArch`](super::GpuArch) for backward compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum NvArch {
    /// Kepler (K80/GK210, Tesla K40) — no firmware signing, legacy nouveau.
    Sm35,
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
    /// Blackwell (RTX 50xx, `GB20x`) — 5th gen tensor cores.
    Sm120,
}

impl NvArch {
    /// All supported NVIDIA architectures, ordered by SM version.
    pub const ALL: &[Self] = &[
        Self::Sm35,
        Self::Sm70,
        Self::Sm75,
        Self::Sm80,
        Self::Sm86,
        Self::Sm89,
        Self::Sm120,
    ];

    /// Parse an architecture string (`"sm_70"`, `"sm70"`, etc.).
    ///
    /// Returns `None` if the string doesn't match any known architecture.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "sm_35" | "sm35" => Some(Self::Sm35),
            "sm_70" | "sm70" => Some(Self::Sm70),
            "sm_75" | "sm75" => Some(Self::Sm75),
            "sm_80" | "sm80" => Some(Self::Sm80),
            "sm_86" | "sm86" => Some(Self::Sm86),
            "sm_89" | "sm89" => Some(Self::Sm89),
            "sm_120" | "sm120" => Some(Self::Sm120),
            _ => None,
        }
    }

    /// Short architecture identifier (e.g. `"sm70"`, `"sm86"`).
    #[must_use]
    pub const fn short_name(self) -> &'static str {
        match self {
            Self::Sm35 => "sm35",
            Self::Sm70 => "sm70",
            Self::Sm75 => "sm75",
            Self::Sm80 => "sm80",
            Self::Sm86 => "sm86",
            Self::Sm89 => "sm89",
            Self::Sm120 => "sm120",
        }
    }

    /// Shader model number.
    #[must_use]
    pub const fn sm(self) -> u32 {
        match self {
            Self::Sm35 => 35,
            Self::Sm70 => 70,
            Self::Sm75 => 75,
            Self::Sm80 => 80,
            Self::Sm86 => 86,
            Self::Sm89 => 89,
            Self::Sm120 => 120,
        }
    }

    /// Shader model version as u8 (for `ShaderModelInfo`, etc.).
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "sm() is 35-120, always fits in u8"
    )]
    pub const fn sm_version(self) -> u8 {
        self.sm() as u8
    }

    /// Whether this arch supports DFMA (double-precision FMA) natively.
    #[must_use]
    pub const fn has_dfma(self) -> bool {
        true // All SM35+ support DFMA (DADD, DFMA, DMUL)
    }

    /// Whether this arch has fast f64 throughput (1:2 vs 1:32 of f32).
    #[must_use]
    pub const fn has_fast_fp64(self) -> bool {
        matches!(self, Self::Sm35 | Self::Sm70 | Self::Sm80)
    }

    /// Native f64 rate relative to f32 (denominator: 1/N of f32 rate).
    #[must_use]
    pub const fn f64_rate_divisor(self) -> u32 {
        match self {
            Self::Sm35 => 3,
            _ if self.has_fast_fp64() => 2,
            _ => 32,
        }
    }

    /// Hardware f64 transcendental seed availability (rcp64h / rsq64h).
    ///
    /// SM50-SM89: RCP64H and RSQ64H produce correct hardware seeds.
    /// SM35 (Kepler): no 64-bit MUFU — uses f32 MUFU seed path.
    /// SM120 (Blackwell): RCP64H/RSQ64H emit but produce incorrect results
    /// (Exp 177 finding). The lowering in `newton.rs` must use the f32 MUFU
    /// seed path for Blackwell, matching `GenerationProfile::has_hardware_f64_rcp`.
    #[must_use]
    pub const fn has_transcendental_64h(self) -> bool {
        !matches!(self, Self::Sm35 | Self::Sm120)
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
            Self::Sm35 | Self::Sm70 | Self::Sm75 => 49_152,
            Self::Sm80 | Self::Sm86 | Self::Sm89 | Self::Sm120 => 102_400,
        }
    }

    /// Maximum concurrent warps per SM for this architecture.
    #[must_use]
    pub const fn max_warps_per_sm(self) -> u32 {
        match self {
            Self::Sm35 | Self::Sm70 | Self::Sm80 => 64,
            Self::Sm75 => 32,
            Self::Sm86 | Self::Sm89 | Self::Sm120 => 48,
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
