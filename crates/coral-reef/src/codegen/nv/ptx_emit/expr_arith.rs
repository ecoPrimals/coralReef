// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt::Write as _;

use crate::error::CompileError;

use super::PtxEmitter;
use super::types::PtxVal;

impl PtxEmitter<'_> {
    pub(super) fn eval_binary(
        &mut self,
        op: naga::BinaryOperator,
        left: &PtxVal,
        right: &PtxVal,
        left_handle: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        let scalar = self.scalar_of(left_handle);
        let ts = Self::ptx_type_suffix(scalar);

        if let (PtxVal::Vec(lv), PtxVal::Vec(rv)) = (left, right) {
            let mut results = Vec::with_capacity(lv.len());
            for (left_comp, right_comp) in lv.iter().zip(rv.iter()) {
                results.push(self.eval_binary_scalar(op, left_comp, right_comp, scalar, ts)?);
            }
            return Ok(PtxVal::Vec(results));
        }

        self.eval_binary_scalar(op, left, right, scalar, ts)
    }

    fn eval_binary_scalar(
        &mut self,
        op: naga::BinaryOperator,
        left: &PtxVal,
        right: &PtxVal,
        scalar: naga::Scalar,
        ts: &str,
    ) -> Result<PtxVal, CompileError> {
        use naga::BinaryOperator as BO;
        let is_float = scalar.kind == naga::ScalarKind::Float;

        match op {
            BO::Add | BO::Subtract | BO::Multiply | BO::Divide | BO::Modulo => {
                let dst = self.alloc_for_scalar(scalar);
                let instr = match op {
                    BO::Add => "add",
                    BO::Subtract => "sub",
                    BO::Multiply if is_float => "mul",
                    BO::Multiply => "mul.lo",
                    BO::Divide => "div.rn",
                    BO::Modulo => "rem",
                    _ => unreachable!(),
                };
                writeln!(
                    self.body,
                    "    {instr}.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    left.fmt_operand(),
                    right.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            BO::Equal
            | BO::NotEqual
            | BO::Less
            | BO::LessEqual
            | BO::Greater
            | BO::GreaterEqual => {
                let pred = self.alloc_pred();
                let cmp = match op {
                    BO::Equal => "eq",
                    BO::NotEqual => "ne",
                    BO::Less => "lt",
                    BO::LessEqual => "le",
                    BO::Greater => "gt",
                    BO::GreaterEqual => "ge",
                    _ => unreachable!(),
                };
                writeln!(
                    self.body,
                    "    setp.{cmp}.{ts} {}, {}, {};",
                    pred.fmt_operand(),
                    left.fmt_operand(),
                    right.fmt_operand(),
                )
                .expect("write to String");
                Ok(pred)
            }
            BO::And => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    and.b{} {}, {}, {};",
                    scalar.width * 8,
                    dst.fmt_operand(),
                    left.fmt_operand(),
                    right.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            BO::InclusiveOr => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    or.b{} {}, {}, {};",
                    scalar.width * 8,
                    dst.fmt_operand(),
                    left.fmt_operand(),
                    right.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            BO::ExclusiveOr => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    xor.b{} {}, {}, {};",
                    scalar.width * 8,
                    dst.fmt_operand(),
                    left.fmt_operand(),
                    right.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            BO::ShiftLeft => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    shl.b{} {}, {}, {};",
                    scalar.width * 8,
                    dst.fmt_operand(),
                    left.fmt_operand(),
                    right.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            BO::ShiftRight => {
                let dst = self.alloc_for_scalar(scalar);
                let instr = if scalar.kind == naga::ScalarKind::Sint {
                    "shr.s"
                } else {
                    "shr.u"
                };
                writeln!(
                    self.body,
                    "    {instr}{} {}, {}, {};",
                    scalar.width * 8,
                    dst.fmt_operand(),
                    left.fmt_operand(),
                    right.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            BO::LogicalAnd => {
                let lp = self.ensure_pred(left)?;
                let rp = self.ensure_pred(right)?;
                let dst = self.alloc_pred();
                writeln!(
                    self.body,
                    "    and.pred {}, {}, {};",
                    dst.fmt_operand(),
                    lp.fmt_operand(),
                    rp.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            BO::LogicalOr => {
                let lp = self.ensure_pred(left)?;
                let rp = self.ensure_pred(right)?;
                let dst = self.alloc_pred();
                writeln!(
                    self.body,
                    "    or.pred {}, {}, {};",
                    dst.fmt_operand(),
                    lp.fmt_operand(),
                    rp.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
        }
    }

    pub(super) fn eval_unary(
        &mut self,
        op: naga::UnaryOperator,
        val: &PtxVal,
        expr: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        let scalar = self.scalar_of(expr);
        match op {
            naga::UnaryOperator::Negate => {
                let dst = self.alloc_for_scalar(scalar);
                let ts = Self::ptx_type_suffix(scalar);
                writeln!(
                    self.body,
                    "    neg.{ts} {}, {};",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            naga::UnaryOperator::LogicalNot => {
                let p = self.ensure_pred(val)?;
                let dst = self.alloc_pred();
                writeln!(
                    self.body,
                    "    not.pred {}, {};",
                    dst.fmt_operand(),
                    p.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            naga::UnaryOperator::BitwiseNot => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    not.b{} {}, {};",
                    scalar.width * 8,
                    dst.fmt_operand(),
                    val.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
        }
    }
}
