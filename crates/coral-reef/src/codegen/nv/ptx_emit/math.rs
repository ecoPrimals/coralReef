// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt::Write as _;

use crate::error::CompileError;

use super::PtxEmitter;
use super::types::PtxVal;

impl PtxEmitter<'_> {
    /// Emits PTX for a built-in mathematical function (`min`, `max`, `fma`, etc.).
    pub(super) fn eval_math(
        &mut self,
        fun: naga::MathFunction,
        arg: &PtxVal,
        arg1: Option<&PtxVal>,
        arg2: Option<&PtxVal>,
        arg_handle: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        use naga::MathFunction as MF;
        let scalar = self.scalar_of(arg_handle);
        let ts = Self::ptx_type_suffix(scalar);

        match fun {
            MF::Abs => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    abs.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Min => {
                let rhs =
                    arg1.ok_or_else(|| CompileError::NotImplemented("min without arg1".into()))?;
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    min.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    rhs.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Max => {
                let rhs =
                    arg1.ok_or_else(|| CompileError::NotImplemented("max without arg1".into()))?;
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    max.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    rhs.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Clamp => {
                let lo =
                    arg1.ok_or_else(|| CompileError::NotImplemented("clamp without arg1".into()))?;
                let hi =
                    arg2.ok_or_else(|| CompileError::NotImplemented("clamp without arg2".into()))?;
                let tmp = self.alloc_for_scalar(scalar);
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    max.{ts} {}, {}, {};",
                    tmp.fmt_operand(),
                    arg.fmt_operand(),
                    lo.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    min.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    tmp.fmt_operand(),
                    hi.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Floor | MF::Ceil | MF::Round | MF::Trunc => {
                let mode = match fun {
                    MF::Floor => "rmi",
                    MF::Ceil => "rpi",
                    MF::Round => "rni",
                    MF::Trunc => "rzi",
                    _ => unreachable!(),
                };
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    cvt.{mode}.{ts}.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Sqrt => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    sqrt.rn.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::InverseSqrt => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    rsqrt.approx.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Sin | MF::Cos => {
                let op_name = if matches!(fun, MF::Sin) { "sin" } else { "cos" };
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    {op_name}.approx.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Exp2 => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    ex2.approx.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Log2 => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    lg2.approx.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Fma => {
                let mul_term =
                    arg1.ok_or_else(|| CompileError::NotImplemented("fma without arg1".into()))?;
                let add_term =
                    arg2.ok_or_else(|| CompileError::NotImplemented("fma without arg2".into()))?;
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    fma.rn.{ts} {}, {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    mul_term.fmt_operand(),
                    add_term.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            _ => Err(CompileError::NotImplemented(
                format!("PTX math function: {fun:?}").into(),
            )),
        }
    }
}
