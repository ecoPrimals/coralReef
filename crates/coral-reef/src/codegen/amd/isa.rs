// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! AMD ISA types — encoding formats, instruction categories, and opcode tables.
//!
//! These types model the RDNA2 (GFX1030) instruction set architecture.
//! Encoding field layouts are derived from AMD's machine-readable ISA XML
//! specifications (MIT license).
//!
//! ## Encoding Format Summary (RDNA2)
//!
//! | Format | Bits | Description |
//! |--------|------|-------------|
//! | SOP1 | 32 | Scalar ALU, 1 source |
//! | SOP2 | 32 | Scalar ALU, 2 sources |
//! | SOPC | 32 | Scalar comparison |
//! | SOPK | 32 | Scalar with 16-bit immediate |
//! | SOPP | 32 | Scalar program control (branch, barrier, etc.) |
//! | SMEM | 64 | Scalar memory load/store |
//! | VOP1 | 32 | Vector ALU, 1 source |
//! | VOP2 | 32 | Vector ALU, 2 sources |
//! | VOP3 | 64 | Vector ALU, 3 sources (full modifier support) |
//! | VOP3P | 64 | Packed math (f16x2) |
//! | VOPC | 32 | Vector comparison (writes VCC) |
//! | DS | 64 | Data share (LDS) operations |
//! | FLAT | 64 | Flat memory (global/scratch) |
//! | MUBUF | 64 | Typed buffer operations |
//! | MTBUF | 64 | Typed buffer with format |
//! | MIMG | 64+ | Image/texture operations |
//! | EXP | 64 | Export (vertex/pixel output) |

/// RDNA2 instruction encoding formats.
///
/// Each variant corresponds to a distinct binary encoding layout.
/// Instructions may support multiple encodings (e.g. VOP2 instructions
/// can also be encoded as VOP3 for full modifier access).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EncodingFormat {
    /// Scalar ALU, 1 source — 32-bit.
    Sop1,
    /// Scalar ALU, 2 sources — 32-bit.
    Sop2,
    /// Scalar comparison — 32-bit.
    Sopc,
    /// Scalar with 16-bit inline constant — 32-bit.
    Sopk,
    /// Scalar program control (branch, barrier, endpgm) — 32-bit.
    Sopp,
    /// Scalar memory — 64-bit.
    Smem,
    /// Vector ALU, 1 source — 32-bit.
    Vop1,
    /// Vector ALU, 2 sources — 32-bit.
    Vop2,
    /// Vector ALU, 3 sources with full modifiers — 64-bit.
    Vop3,
    /// Packed math (f16x2 operations) — 64-bit.
    Vop3p,
    /// Vector comparison (writes VCC) — 32-bit.
    Vopc,
    /// Data share (LDS) operations — 64-bit.
    Ds,
    /// Flat memory (global + scratch) — 64-bit.
    Flat,
    /// Flat global memory — 64-bit.
    FlatGlobal,
    /// Flat scratch memory — 64-bit.
    FlatScratch,
    /// Typed buffer operations — 64-bit.
    Mubuf,
    /// Typed buffer with format conversion — 64-bit.
    Mtbuf,
    /// Image/texture operations — 64-bit+.
    Mimg,
    /// Export (vertex/pixel output) — 64-bit.
    Exp,
    /// Interpolation — 32-bit.
    Vintrp,
}

impl EncodingFormat {
    /// Base instruction size in bits.
    pub const fn bit_count(self) -> u32 {
        match self {
            Self::Sop1
            | Self::Sop2
            | Self::Sopc
            | Self::Sopk
            | Self::Sopp
            | Self::Vop1
            | Self::Vop2
            | Self::Vopc
            | Self::Vintrp => 32,

            Self::Smem
            | Self::Vop3
            | Self::Vop3p
            | Self::Ds
            | Self::Flat
            | Self::FlatGlobal
            | Self::FlatScratch
            | Self::Mubuf
            | Self::Mtbuf
            | Self::Mimg
            | Self::Exp => 64,
        }
    }

    /// Base instruction size in 32-bit words.
    pub const fn word_count(self) -> u32 {
        self.bit_count() / 32
    }

    /// Whether this encoding may be followed by a 32-bit literal constant.
    pub const fn can_have_literal(self) -> bool {
        matches!(
            self,
            Self::Sop1
                | Self::Sop2
                | Self::Sopc
                | Self::Sopk
                | Self::Vop1
                | Self::Vop2
                | Self::Vopc
                | Self::Vop3
                | Self::Vop3p
        )
    }
}

/// Instruction functional group — high-level classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionalGroup {
    /// Scalar ALU operations.
    Salu,
    /// Scalar memory operations.
    Smem,
    /// Vector ALU operations.
    Valu,
    /// Vector memory operations (buffer, image, flat).
    Vmem,
    /// Data share (LDS) operations.
    Ds,
    /// Export operations.
    Export,
    /// Flow control (branch, call, barrier).
    FlowControl,
}

/// An AMD instruction opcode within a specific encoding format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmdOpcode {
    /// The encoding format this opcode belongs to.
    pub format: EncodingFormat,
    /// The numeric opcode value within the encoding.
    pub opcode: u16,
}

// Re-export authoritative opcode tables from XML-generated module.
// These are the machine-readable ISA values — always prefer these over
// hand-coded constants. See tools/amd-isa-gen/gen_rdna2_opcodes.py.
//
// All encoding modules are exported even if not yet consumed internally;
// they form the public API for the AMD backend's ISA layer.
#[allow(unused_imports)]
pub use super::isa_generated::{
    ds, flat, flat_glbl, flat_scratch, mimg, mtbuf, mubuf, smem, sop1, sop2, sopc, sopk, sopp,
    vop1, vop2, vop3, vop3p, vopc,
};

/// RDNA2 GFX version identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GfxVersion {
    pub major: u8,
    pub minor: u8,
    pub stepping: u8,
}

impl GfxVersion {
    /// GFX 10.3.0 — Navi 21 (RX 6800/6900/6950 XT).
    pub const GFX1030: Self = Self {
        major: 10,
        minor: 3,
        stepping: 0,
    };

    /// GFX 10.3.1 — Navi 22 (RX 6700 XT).
    pub const GFX1031: Self = Self {
        major: 10,
        minor: 3,
        stepping: 1,
    };

    /// GFX 10.3.2 — Navi 23 (RX 6600 XT).
    pub const GFX1032: Self = Self {
        major: 10,
        minor: 3,
        stepping: 2,
    };
}

impl std::fmt::Display for GfxVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "gfx{}{}{}", self.major, self.minor, self.stepping)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_sizes() {
        assert_eq!(EncodingFormat::Sop1.bit_count(), 32);
        assert_eq!(EncodingFormat::Vop3.bit_count(), 64);
        assert_eq!(EncodingFormat::Smem.bit_count(), 64);
        assert_eq!(EncodingFormat::Ds.bit_count(), 64);
    }

    #[test]
    fn encoding_word_count() {
        assert_eq!(EncodingFormat::Vop2.word_count(), 1);
        assert_eq!(EncodingFormat::Vop3.word_count(), 2);
    }

    #[test]
    fn gfx_version_display() {
        assert_eq!(GfxVersion::GFX1030.to_string(), "gfx1030");
        assert_eq!(GfxVersion::GFX1031.to_string(), "gfx1031");
    }

    #[test]
    fn literal_support() {
        assert!(EncodingFormat::Vop2.can_have_literal());
        assert!(EncodingFormat::Vop3.can_have_literal());
        assert!(!EncodingFormat::Ds.can_have_literal());
        assert!(!EncodingFormat::Flat.can_have_literal());
    }
}
