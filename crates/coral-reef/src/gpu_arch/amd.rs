// SPDX-License-Identifier: AGPL-3.0-or-later
//! AMD GPU architectures (GCN / RDNA).

/// AMD GPU architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AmdArch {
    /// GCN 5th gen / Vega (Radeon VII / MI50 — GFX900/GFX906).
    Gcn5,
    /// RDNA 2 (RX 6000 series — GFX1030+).
    Rdna2,
    /// RDNA 3 (RX 7000 series — GFX1100+).
    Rdna3,
    /// RDNA 4 (RX 9000 series).
    Rdna4,
}

impl AmdArch {
    /// All supported AMD architectures, ordered by generation.
    pub const ALL: &[Self] = &[Self::Gcn5, Self::Rdna2, Self::Rdna3, Self::Rdna4];

    /// Short architecture identifier (e.g. `"gcn5"`, `"rdna2"`).
    #[must_use]
    pub const fn short_name(self) -> &'static str {
        match self {
            Self::Gcn5 => "gcn5",
            Self::Rdna2 => "rdna2",
            Self::Rdna3 => "rdna3",
            Self::Rdna4 => "rdna4",
        }
    }

    /// GFX version major number for this architecture.
    #[must_use]
    pub const fn gfx_major(self) -> u8 {
        match self {
            Self::Gcn5 => 9,
            Self::Rdna2 => 10,
            Self::Rdna3 => 11,
            Self::Rdna4 => 12,
        }
    }

    /// Default wave size for this architecture.
    #[must_use]
    pub const fn default_wave_size(self) -> u8 {
        match self {
            Self::Gcn5 => 64,
            Self::Rdna2 | Self::Rdna3 | Self::Rdna4 => 32,
        }
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
        match self {
            Self::Gcn5 => 4,
            Self::Rdna2 | Self::Rdna3 | Self::Rdna4 => 16,
        }
    }

    /// Maximum VGPRs per wave.
    #[must_use]
    pub const fn max_vgprs(self) -> u32 {
        256
    }

    /// Maximum SGPRs per wave.
    #[must_use]
    pub const fn max_sgprs(self) -> u32 {
        match self {
            Self::Gcn5 => 102,
            Self::Rdna2 | Self::Rdna3 | Self::Rdna4 => 106,
        }
    }

    /// Maximum shared memory (LDS) per workgroup in bytes.
    #[must_use]
    pub const fn max_lds(self) -> u32 {
        65_536
    }

    /// Whether FLAT instructions support an inline offset field.
    /// GFX9 (GCN5) has no FLAT offset; GFX10+ (RDNA) has 12-bit signed offset.
    #[must_use]
    pub const fn has_flat_offset(self) -> bool {
        match self {
            Self::Gcn5 => false,
            Self::Rdna2 | Self::Rdna3 | Self::Rdna4 => true,
        }
    }

    /// Parse an architecture string (`"gcn5"`, `"rdna2"`, `"gfx906"`, etc.).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "gcn5" | "vega" | "vega20" | "gfx900" | "gfx906" | "gfx908" => Some(Self::Gcn5),
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
            let valid: Vec<&str> = Self::ALL.iter().map(|a| a.short_name()).collect();
            format!(
                "unknown AMD architecture '{s}', valid: {}",
                valid.join(", ")
            )
        })
    }
}

impl std::fmt::Display for AmdArch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.short_name())
    }
}
