// SPDX-License-Identifier: AGPL-3.0-or-later
//! PTX trigonometric and inverse-hyperbolic math operations.
//!
//! Software approximations using hardware MUFU seeds (sin.approx, cos.approx,
//! rsqrt.approx, lg2.approx) and polynomial/identity expansions.

use std::fmt::Write as _;

use crate::error::CompileError;

use super::PtxEmitter;
use super::types::PtxVal;

impl PtxEmitter<'_> {
    /// Trigonometric and inverse-hyperbolic extended math operations.
    ///
    /// Handles: Tan, Atan, Atan2, Asin, Acos, Asinh, Acosh, Atanh.
    pub(super) fn eval_math_trig(
        &mut self,
        fun: naga::MathFunction,
        arg: &PtxVal,
        arg1: Option<&PtxVal>,
        scalar: naga::Scalar,
        ts: &str,
    ) -> Result<PtxVal, CompileError> {
        use naga::MathFunction as MF;
        match fun {
            MF::Tan => {
                let sin_dst = self.alloc_for_scalar(scalar);
                let cos_dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    sin.approx.{ts} {}, {};",
                    sin_dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    cos.approx.{ts} {}, {};",
                    cos_dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    div.rn.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    sin_dst.fmt_operand(),
                    cos_dst.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Atan => {
                let dst = self.alloc_for_scalar(scalar);
                let x2 = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    abs.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand()
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    mul.rn.{ts} {}, {}, {};",
                    x2.fmt_operand(),
                    arg.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                // atan(x) ≈ x / (1 + 0.28125*x²) for |x|<1
                let denom = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    fma.rn.{ts} {denom}, 0f3E900000, {x2}, 0f3F800000;",
                    denom = denom.fmt_operand(),
                    x2 = x2.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    div.rn.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    denom.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Atan2 => {
                let rhs =
                    arg1.ok_or_else(|| CompileError::NotImplemented("atan2 without arg1".into()))?;
                let dst = self.alloc_for_scalar(scalar);
                let ratio = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    div.rn.{ts} {}, {}, {};",
                    ratio.fmt_operand(),
                    arg.fmt_operand(),
                    rhs.fmt_operand(),
                )
                .expect("write to String");
                let r2 = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.rn.{ts} {}, {}, {};",
                    r2.fmt_operand(),
                    ratio.fmt_operand(),
                    ratio.fmt_operand(),
                )
                .expect("write to String");
                let denom = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    fma.rn.{ts} {denom}, 0f3E900000, {r2}, 0f3F800000;",
                    denom = denom.fmt_operand(),
                    r2 = r2.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    div.rn.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    ratio.fmt_operand(),
                    denom.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Asin => {
                let dst = self.alloc_for_scalar(scalar);
                let x2 = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.rn.{ts} {}, {}, {};",
                    x2.fmt_operand(),
                    arg.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                let one_minus = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    sub.{ts} {}, 0f3F800000, {};",
                    one_minus.fmt_operand(),
                    x2.fmt_operand(),
                )
                .expect("write to String");
                let inv_sqrt = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    rsqrt.approx.{ts} {}, {};",
                    inv_sqrt.fmt_operand(),
                    one_minus.fmt_operand(),
                )
                .expect("write to String");
                let scaled = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.rn.{ts} {}, {}, {};",
                    scaled.fmt_operand(),
                    arg.fmt_operand(),
                    inv_sqrt.fmt_operand(),
                )
                .expect("write to String");
                let s2 = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.rn.{ts} {}, {}, {};",
                    s2.fmt_operand(),
                    scaled.fmt_operand(),
                    scaled.fmt_operand(),
                )
                .expect("write to String");
                let denom = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    fma.rn.{ts} {denom}, 0f3E900000, {s2}, 0f3F800000;",
                    denom = denom.fmt_operand(),
                    s2 = s2.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    div.rn.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    scaled.fmt_operand(),
                    denom.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Acos => {
                // acos(x) = π/2 - asin(x)
                let dst = self.alloc_for_scalar(scalar);
                let x2 = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.rn.{ts} {}, {}, {};",
                    x2.fmt_operand(),
                    arg.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                let one_minus = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    sub.{ts} {}, 0f3F800000, {};",
                    one_minus.fmt_operand(),
                    x2.fmt_operand(),
                )
                .expect("write to String");
                let inv_sqrt = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    rsqrt.approx.{ts} {}, {};",
                    inv_sqrt.fmt_operand(),
                    one_minus.fmt_operand(),
                )
                .expect("write to String");
                let scaled = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.rn.{ts} {}, {}, {};",
                    scaled.fmt_operand(),
                    arg.fmt_operand(),
                    inv_sqrt.fmt_operand(),
                )
                .expect("write to String");
                let s2 = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.rn.{ts} {}, {}, {};",
                    s2.fmt_operand(),
                    scaled.fmt_operand(),
                    scaled.fmt_operand(),
                )
                .expect("write to String");
                let denom = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    fma.rn.{ts} {denom}, 0f3E900000, {s2}, 0f3F800000;",
                    denom = denom.fmt_operand(),
                    s2 = s2.fmt_operand(),
                )
                .expect("write to String");
                let asin_val = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    div.rn.{ts} {}, {}, {};",
                    asin_val.fmt_operand(),
                    scaled.fmt_operand(),
                    denom.fmt_operand(),
                )
                .expect("write to String");
                // pi/2 = 0x3FC90FDB
                writeln!(
                    self.body,
                    "    sub.{ts} {}, 0f3FC90FDB, {};",
                    dst.fmt_operand(),
                    asin_val.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Asinh => {
                // asinh(x) = log(x + sqrt(x*x + 1))
                let x_sq = self.alloc_for_scalar(scalar);
                let sum = self.alloc_for_scalar(scalar);
                let sq = self.alloc_for_scalar(scalar);
                let inner = self.alloc_for_scalar(scalar);
                let lg2 = self.alloc_for_scalar(scalar);
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.{ts} {}, {}, {};",
                    x_sq.fmt_operand(),
                    arg.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    add.{ts} {}, {}, 0f3F800000;",
                    sum.fmt_operand(),
                    x_sq.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    sqrt.rn.{ts} {}, {};",
                    sq.fmt_operand(),
                    sum.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    add.{ts} {}, {}, {};",
                    inner.fmt_operand(),
                    arg.fmt_operand(),
                    sq.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    lg2.approx.{ts} {}, {};",
                    lg2.fmt_operand(),
                    inner.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    mul.{ts} {}, {}, 0f3F317218;",
                    dst.fmt_operand(),
                    lg2.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Acosh => {
                // acosh(x) = log(x + sqrt(x*x - 1))
                let x_sq = self.alloc_for_scalar(scalar);
                let diff = self.alloc_for_scalar(scalar);
                let sq = self.alloc_for_scalar(scalar);
                let inner = self.alloc_for_scalar(scalar);
                let lg2 = self.alloc_for_scalar(scalar);
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.{ts} {}, {}, {};",
                    x_sq.fmt_operand(),
                    arg.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    sub.{ts} {}, {}, 0f3F800000;",
                    diff.fmt_operand(),
                    x_sq.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    sqrt.rn.{ts} {}, {};",
                    sq.fmt_operand(),
                    diff.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    add.{ts} {}, {}, {};",
                    inner.fmt_operand(),
                    arg.fmt_operand(),
                    sq.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    lg2.approx.{ts} {}, {};",
                    lg2.fmt_operand(),
                    inner.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    mul.{ts} {}, {}, 0f3F317218;",
                    dst.fmt_operand(),
                    lg2.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Atanh => {
                // atanh(x) = 0.5 * log((1+x)/(1-x))
                let one_plus = self.alloc_for_scalar(scalar);
                let one_minus = self.alloc_for_scalar(scalar);
                let ratio = self.alloc_for_scalar(scalar);
                let lg2 = self.alloc_for_scalar(scalar);
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    add.{ts} {}, 0f3F800000, {};",
                    one_plus.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    sub.{ts} {}, 0f3F800000, {};",
                    one_minus.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    div.rn.{ts} {}, {}, {};",
                    ratio.fmt_operand(),
                    one_plus.fmt_operand(),
                    one_minus.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    lg2.approx.{ts} {}, {};",
                    lg2.fmt_operand(),
                    ratio.fmt_operand(),
                )
                .expect("write to String");
                // 0.5 * ln(2) * lg2(x) = 0.5 * 0.6931472 * lg2 ≈ 0.34657359
                writeln!(
                    self.body,
                    "    mul.{ts} {}, {}, 0f3EB17218;",
                    dst.fmt_operand(),
                    lg2.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            _ => Err(CompileError::NotImplemented(
                format!("PTX trig function: {fun:?}").into(),
            )),
        }
    }
}
