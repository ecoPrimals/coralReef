// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! AMD register file model — VGPR, SGPR, and special registers.
//!
//! AMD RDNA2 uses a fundamentally different register model from NVIDIA:
//!
//! | AMD | NVIDIA equivalent | Description |
//! |-----|-------------------|-------------|
//! | VGPR | GPR | Vector general-purpose (per-lane) |
//! | SGPR | UGPR | Scalar general-purpose (uniform across wave) |
//! | VCC | Pred (partial) | Vector condition code (exec mask for compares) |
//! | EXEC | (implicit) | Execution mask (active lanes) |
//! | SCC | Carry (partial) | Scalar condition code |
//! | M0 | (special) | Miscellaneous register |
//!
//! RDNA2 supports both wave32 and wave64 execution modes.
//! In wave32: VCC and EXEC are 32-bit. In wave64: they are 64-bit.

/// AMD register file categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmdRegFile {
    /// Vector General Purpose Register — per-lane data.
    /// RDNA2: up to 256 VGPRs per wave (v0–v255).
    Vgpr,
    /// Scalar General Purpose Register — uniform across wave.
    /// RDNA2: up to 106 SGPRs (s0–s105).
    Sgpr,
    /// Vector Condition Code — result of vector comparisons.
    /// 32-bit in wave32, 64-bit in wave64.
    Vcc,
    /// Scalar Condition Code — result of scalar ALU operations.
    Scc,
    /// Execution mask — controls which lanes are active.
    /// 32-bit in wave32, 64-bit in wave64.
    Exec,
    /// Miscellaneous register (M0) — used for LDS indexing, etc.
    M0,
}

impl AmdRegFile {
    /// Human-readable prefix for assembly output.
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Vgpr => "v",
            Self::Sgpr => "s",
            Self::Vcc => "vcc",
            Self::Scc => "scc",
            Self::Exec => "exec",
            Self::M0 => "m0",
        }
    }

    /// Whether this register file is vector (per-lane).
    pub const fn is_vector(self) -> bool {
        matches!(self, Self::Vgpr | Self::Vcc | Self::Exec)
    }

    /// Whether this register file is scalar (uniform).
    pub const fn is_scalar(self) -> bool {
        matches!(self, Self::Sgpr | Self::Scc | Self::M0)
    }
}

/// A reference to a specific AMD register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmdRegRef {
    /// Which register file this belongs to.
    pub file: AmdRegFile,
    /// Base register index within the file.
    pub index: u16,
    /// Number of consecutive registers (1 for scalar, 2 for 64-bit, etc.).
    pub count: u8,
}

impl AmdRegRef {
    /// Create a single VGPR reference.
    pub const fn vgpr(index: u16) -> Self {
        Self {
            file: AmdRegFile::Vgpr,
            index,
            count: 1,
        }
    }

    /// Create a single SGPR reference.
    pub const fn sgpr(index: u16) -> Self {
        Self {
            file: AmdRegFile::Sgpr,
            index,
            count: 1,
        }
    }

    /// Create a 64-bit VGPR pair (e.g. for f64 operations).
    pub const fn vgpr_pair(index: u16) -> Self {
        Self {
            file: AmdRegFile::Vgpr,
            index,
            count: 2,
        }
    }

    /// Create a 64-bit SGPR pair.
    pub const fn sgpr_pair(index: u16) -> Self {
        Self {
            file: AmdRegFile::Sgpr,
            index,
            count: 2,
        }
    }

    /// Hardware encoding value for this register in instruction fields.
    ///
    /// RDNA2 register encoding:
    /// - VGPR: 256 + index (in VOP3 SRC fields)
    /// - SGPR: index (0–105)
    /// - VCC_LO: 106, VCC_HI: 107
    /// - EXEC_LO: 126, EXEC_HI: 127
    /// - SCC: 253
    /// - Literal constant: 255
    /// - M0: 124
    pub const fn hw_encoding(self) -> u16 {
        match self.file {
            AmdRegFile::Vgpr => 256 + self.index,
            AmdRegFile::Sgpr => self.index,
            AmdRegFile::Vcc => 106,
            AmdRegFile::Scc => 253,
            AmdRegFile::Exec => 126,
            AmdRegFile::M0 => 124,
        }
    }
}

impl std::fmt::Display for AmdRegRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.file {
            AmdRegFile::Vgpr if self.count > 1 => {
                write!(
                    f,
                    "v[{}:{}]",
                    self.index,
                    self.index + u16::from(self.count) - 1
                )
            }
            AmdRegFile::Vgpr => write!(f, "v{}", self.index),
            AmdRegFile::Sgpr if self.count > 1 => {
                write!(
                    f,
                    "s[{}:{}]",
                    self.index,
                    self.index + u16::from(self.count) - 1
                )
            }
            AmdRegFile::Sgpr => write!(f, "s{}", self.index),
            AmdRegFile::Vcc => write!(f, "vcc"),
            AmdRegFile::Scc => write!(f, "scc"),
            AmdRegFile::Exec => write!(f, "exec"),
            AmdRegFile::M0 => write!(f, "m0"),
        }
    }
}

/// RDNA2 hardware limits.
pub mod limits {
    /// Maximum VGPR count per wave (RDNA2).
    pub const MAX_VGPRS: u16 = 256;

    /// Maximum SGPR count per wave (RDNA2).
    /// Hardware supports 106 SGPRs (s0–s105). s106/s107 = VCC.
    pub const MAX_SGPRS: u16 = 106;

    /// Wave size options for RDNA2.
    pub const WAVE32: u8 = 32;
    pub const WAVE64: u8 = 64;

    /// Maximum shared memory (LDS) per workgroup (RDNA2), in bytes.
    pub const MAX_LDS_SIZE: u32 = 65_536;

    /// Maximum workgroup size (threads).
    pub const MAX_WORKGROUP_SIZE: u32 = 1024;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vgpr_display() {
        assert_eq!(AmdRegRef::vgpr(0).to_string(), "v0");
        assert_eq!(AmdRegRef::vgpr(42).to_string(), "v42");
    }

    #[test]
    fn sgpr_display() {
        assert_eq!(AmdRegRef::sgpr(0).to_string(), "s0");
        assert_eq!(AmdRegRef::sgpr(105).to_string(), "s105");
    }

    #[test]
    fn pair_display() {
        assert_eq!(AmdRegRef::vgpr_pair(4).to_string(), "v[4:5]");
        assert_eq!(AmdRegRef::sgpr_pair(10).to_string(), "s[10:11]");
    }

    #[test]
    fn hw_encoding_vgpr() {
        assert_eq!(AmdRegRef::vgpr(0).hw_encoding(), 256);
        assert_eq!(AmdRegRef::vgpr(127).hw_encoding(), 383);
    }

    #[test]
    fn hw_encoding_sgpr() {
        assert_eq!(AmdRegRef::sgpr(0).hw_encoding(), 0);
        assert_eq!(AmdRegRef::sgpr(105).hw_encoding(), 105);
    }

    #[test]
    fn hw_encoding_special() {
        assert_eq!(
            AmdRegRef {
                file: AmdRegFile::Vcc,
                index: 0,
                count: 1
            }
            .hw_encoding(),
            106
        );
        assert_eq!(
            AmdRegRef {
                file: AmdRegFile::Exec,
                index: 0,
                count: 1
            }
            .hw_encoding(),
            126
        );
        assert_eq!(
            AmdRegRef {
                file: AmdRegFile::Scc,
                index: 0,
                count: 1
            }
            .hw_encoding(),
            253
        );
        assert_eq!(
            AmdRegRef {
                file: AmdRegFile::M0,
                index: 0,
                count: 1
            }
            .hw_encoding(),
            124
        );
    }

    #[test]
    fn register_file_classification() {
        assert!(AmdRegFile::Vgpr.is_vector());
        assert!(!AmdRegFile::Vgpr.is_scalar());
        assert!(AmdRegFile::Sgpr.is_scalar());
        assert!(!AmdRegFile::Sgpr.is_vector());
    }
}
