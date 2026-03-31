// SPDX-License-Identifier: AGPL-3.0-only
//! Pure-Rust evaluation of individual `CoralIR` operations.
//!
//! Each function here corresponds to a `translate_*` in the JIT, but evaluates
//! immediately instead of emitting CLIF instructions. This is the ground truth
//! for numerical semantics.

use coral_reef::codegen::ir::{FRndMode, FloatCmpOp, IntCmpOp, TranscendentalOp};

use crate::types::CpuError;

/// Evaluate an f32 comparison.
///
/// These are exact GPU-semantics comparisons (IEEE 754 bit-exact equality for
/// `OrdEq`/`OrdNe`). Using approximate comparison here would be incorrect —
/// tolerance belongs in the validation layer, not the executor.
#[must_use]
#[expect(clippy::float_cmp, reason = "GPU comparison semantics require exact IEEE 754 equality")]
pub fn float_cmp(a: f32, b: f32, cmp: FloatCmpOp) -> bool {
    match cmp {
        FloatCmpOp::OrdEq => a == b,
        FloatCmpOp::OrdNe => a != b,
        FloatCmpOp::OrdLt => a < b,
        FloatCmpOp::OrdLe => a <= b,
        FloatCmpOp::OrdGt => a > b,
        FloatCmpOp::OrdGe => a >= b,
        FloatCmpOp::UnordEq => a.is_nan() || b.is_nan() || a == b,
        FloatCmpOp::UnordNe => a.is_nan() || b.is_nan() || a != b,
        FloatCmpOp::UnordLt => a.is_nan() || b.is_nan() || a < b,
        FloatCmpOp::UnordLe => a.is_nan() || b.is_nan() || a <= b,
        FloatCmpOp::UnordGt => a.is_nan() || b.is_nan() || a > b,
        FloatCmpOp::UnordGe => a.is_nan() || b.is_nan() || a >= b,
        FloatCmpOp::IsNum => !a.is_nan() && !b.is_nan(),
        FloatCmpOp::IsNan => a.is_nan() || b.is_nan(),
    }
}

/// Evaluate an f64 comparison.
#[must_use]
#[expect(clippy::float_cmp, reason = "GPU comparison semantics require exact IEEE 754 equality")]
pub fn float_cmp_f64(a: f64, b: f64, cmp: FloatCmpOp) -> bool {
    match cmp {
        FloatCmpOp::OrdEq => a == b,
        FloatCmpOp::OrdNe => a != b,
        FloatCmpOp::OrdLt => a < b,
        FloatCmpOp::OrdLe => a <= b,
        FloatCmpOp::OrdGt => a > b,
        FloatCmpOp::OrdGe => a >= b,
        FloatCmpOp::UnordEq => a.is_nan() || b.is_nan() || a == b,
        FloatCmpOp::UnordNe => a.is_nan() || b.is_nan() || a != b,
        FloatCmpOp::UnordLt => a.is_nan() || b.is_nan() || a < b,
        FloatCmpOp::UnordLe => a.is_nan() || b.is_nan() || a <= b,
        FloatCmpOp::UnordGt => a.is_nan() || b.is_nan() || a > b,
        FloatCmpOp::UnordGe => a.is_nan() || b.is_nan() || a >= b,
        FloatCmpOp::IsNum => !a.is_nan() && !b.is_nan(),
        FloatCmpOp::IsNan => a.is_nan() || b.is_nan(),
    }
}

/// Evaluate an integer comparison.
#[must_use]
#[expect(clippy::cast_sign_loss, reason = "unsigned comparison reinterprets i32 bit pattern")]
pub const fn int_cmp(a: i32, b: i32, cmp: IntCmpOp, signed: bool) -> bool {
    if signed {
        match cmp {
            IntCmpOp::Eq | IntCmpOp::True => a == b,
            IntCmpOp::Ne | IntCmpOp::False => a != b,
            IntCmpOp::Lt => a < b,
            IntCmpOp::Le => a <= b,
            IntCmpOp::Gt => a > b,
            IntCmpOp::Ge => a >= b,
        }
    } else {
        let ua = a as u32;
        let ub = b as u32;
        match cmp {
            IntCmpOp::Eq | IntCmpOp::True => ua == ub,
            IntCmpOp::Ne | IntCmpOp::False => ua != ub,
            IntCmpOp::Lt => ua < ub,
            IntCmpOp::Le => ua <= ub,
            IntCmpOp::Gt => ua > ub,
            IntCmpOp::Ge => ua >= ub,
        }
    }
}

/// Apply a float rounding mode.
#[must_use]
pub fn apply_rnd_mode(val: f64, mode: FRndMode) -> f64 {
    match mode {
        FRndMode::NearestEven => val.round(),
        FRndMode::Zero => val.trunc(),
        FRndMode::NegInf => val.floor(),
        FRndMode::PosInf => val.ceil(),
    }
}

/// Evaluate a transcendental operation on an f32 value.
///
/// # Errors
///
/// Returns [`CpuError::Unsupported`] for unhandled transcendental operations.
pub fn eval_transcendental(src: f32, trans_op: TranscendentalOp) -> Result<f32, CpuError> {
    match trans_op {
        TranscendentalOp::Rcp => Ok(1.0 / src),
        TranscendentalOp::Rsq => Ok(1.0 / src.sqrt()),
        TranscendentalOp::Sqrt => Ok(src.sqrt()),
        TranscendentalOp::Log2 => Ok(src.log2()),
        TranscendentalOp::Exp2 => Ok(src.exp2()),
        TranscendentalOp::Sin => Ok(src.sin()),
        TranscendentalOp::Cos => Ok(src.cos()),
        TranscendentalOp::Tanh => Ok(src.tanh()),
        _ => Err(CpuError::Unsupported(format!(
            "transcendental {trans_op:?}"
        ))),
    }
}
