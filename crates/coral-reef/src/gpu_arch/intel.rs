// SPDX-License-Identifier: AGPL-3.0-or-later
//! Intel GPU architectures (future — backend not yet implemented).
//!
//! Xe-HPG (DG2/Alchemist) and Xe2-HPG (Battlemage) — register addresses TBD.

/// Intel GPU architecture (future).
///
/// Xe-HPG (Arc A-series discrete — DG2/Alchemist) and Xe2-HPG (Battlemage)
/// — register addresses TBD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum IntelArch {
    /// Xe-HPG (Arc A-series discrete — DG2/Alchemist).
    XeHpg,
    /// Xe-HPG DG2/Alchemist (Arc A770, A750, A380, etc.).
    Dg2Alchemist,
    /// Xe2-HPG (Battlemage — next-gen Arc discrete).
    Xe2Hpg,
    /// Xe-LPG (Meteor Lake / Arrow Lake integrated graphics).
    XeLpg,
}

impl IntelArch {
    /// All known Intel architectures (for iteration; backend not implemented).
    pub const ALL: &[Self] = &[Self::XeHpg, Self::Dg2Alchemist, Self::Xe2Hpg, Self::XeLpg];

    /// Short architecture identifier (e.g. `"xe_hpg"`, `"dg2_alchemist"`).
    #[must_use]
    pub const fn short_name(self) -> &'static str {
        match self {
            Self::XeHpg => "xe_hpg",
            Self::Dg2Alchemist => "dg2_alchemist",
            Self::Xe2Hpg => "xe2_hpg",
            Self::XeLpg => "xe_lpg",
        }
    }
}

impl std::fmt::Display for IntelArch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.short_name())
    }
}
