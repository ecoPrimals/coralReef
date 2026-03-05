// SPDX-License-Identifier: AGPL-3.0-only
//! NAK Intermediate Representation (IR) — stub module.
//!
//! This module will contain the full NAK IR once Mesa sources are extracted.
//! The IR represents NVIDIA GPU instructions before encoding.
//!
//! Key types from the original NAK:
//!
//! - `RegFile`: Register file (GPR, UGPR, Pred, `UPred`, Carry, Bar, Mem)
//! - `Src` / `Dst`: Source and destination operands
//! - `Op`: Instruction opcode enum (~200 variants)
//! - `Instr`: A single instruction with srcs, dsts, and predicate
//! - `BasicBlock`: Sequence of instructions
//! - `Function`: CFG of basic blocks
//! - `Shader`: Top-level shader with functions and metadata
//!
//! ## Extraction Status
//!
//! The original NAK IR is defined in `mesa-nak/src/nouveau/compiler/nak/ir.rs`
//! (~5800 lines).  It depends on:
//!
//! - `bitview` (bit-level register packing)
//! - `nak_ir_proc` (derive macros: `SrcsAsSlice`, `DstsAsSlice`, `DisplayOp`)
//! - `compiler::cfg` (Mesa CFG types)
//! - `compiler::smallvec`
//!
//! The `coral-reef-stubs` crate provides replacements for the Mesa dependencies.

/// Register file identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegFile {
    /// General Purpose Register (32-bit).
    Gpr,
    /// Uniform GPR (shared across warp).
    Ugpr,
    /// Predicate register (1-bit).
    Pred,
    /// Uniform predicate.
    Upred,
    /// Carry flag.
    Carry,
    /// Barrier register.
    Bar,
    /// Memory (spill).
    Mem,
}

impl RegFile {
    /// Number of bits per register in this file.
    #[must_use]
    pub const fn bits(self) -> u32 {
        match self {
            Self::Gpr | Self::Ugpr | Self::Mem => 32,
            Self::Pred | Self::Upred | Self::Carry | Self::Bar => 1,
        }
    }
}

/// Floating-point rounding mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FRndMode {
    /// Round to nearest even (IEEE default).
    NearestEven,
    /// Round toward zero.
    Zero,
    /// Round toward +inf.
    PositiveInf,
    /// Round toward -inf.
    NegativeInf,
}

/// Multi-Function Unit operation (MUFU).
///
/// These are the hardware transcendental approximations on NVIDIA GPUs.
/// They are **f32-only** — this is the root cause of the f64 gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuFuOp {
    /// cos(x) — f32 only
    Cos,
    /// sin(x) — f32 only
    Sin,
    /// 2^x — f32 only
    Exp2,
    /// log2(x) — f32 only
    Log2,
    /// 1/x — f32 only
    Rcp,
    /// 1/sqrt(x) — f32 only
    Rsq,
    /// High 32 bits of f64 reciprocal (initial approximation).
    Rcp64H,
    /// High 32 bits of f64 rsqrt (initial approximation).
    Rsq64H,
    /// sqrt(x) — f32 only
    Sqrt,
    /// tanh(x) — f32 only, SM80+
    Tanh,
}

impl MuFuOp {
    /// Whether this op supports f64 operands.
    #[must_use]
    pub const fn supports_f64(self) -> bool {
        matches!(self, Self::Rcp64H | Self::Rsq64H)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reg_file_bits() {
        assert_eq!(RegFile::Gpr.bits(), 32);
        assert_eq!(RegFile::Pred.bits(), 1);
    }

    #[test]
    fn test_mufu_f64_support() {
        assert!(!MuFuOp::Sin.supports_f64());
        assert!(!MuFuOp::Cos.supports_f64());
        assert!(!MuFuOp::Exp2.supports_f64());
        assert!(MuFuOp::Rcp64H.supports_f64());
        assert!(MuFuOp::Rsq64H.supports_f64());
    }
}
