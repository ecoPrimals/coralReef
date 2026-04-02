// SPDX-License-Identifier: AGPL-3.0-only
//! Binary and unary operation lowering → CoralIR.

use super::FuncLowerer;
use crate::ast::{BinaryOp, UnaryOp};
use coral_reef::codegen::ir::*;
use coral_reef::error::CompileError;

const F32_ONE: u32 = 0x3F80_0000;

impl FuncLowerer<'_, '_> {
    pub(crate) fn lower_binary(&mut self, op: BinaryOp, l: SSARef, r: SSARef) -> Result<SSARef, CompileError> {
        let dst = self.alloc_ssa(RegFile::GPR);
        match op {
            BinaryOp::Add => {
                self.push_instr(Instr::new(OpFAdd {
                    dst: dst.into(),
                    srcs: [Src::from(l[0]), Src::from(r[0])],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }));
            }
            BinaryOp::Subtract => {
                self.push_instr(Instr::new(OpFAdd {
                    dst: dst.into(),
                    srcs: [Src::from(l[0]), Src::from(r[0]).fneg()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }));
            }
            BinaryOp::Multiply => {
                self.push_instr(Instr::new(OpFMul {
                    dst: dst.into(),
                    srcs: [Src::from(l[0]), Src::from(r[0])],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
            }
            BinaryOp::Divide => {
                // fdiv(a, b) = a * rcp(b)
                let rcp = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental {
                    dst: rcp.into(),
                    op: TranscendentalOp::Rcp,
                    src: Src::from(r[0]),
                }));
                self.push_instr(Instr::new(OpFMul {
                    dst: dst.into(),
                    srcs: [Src::from(l[0]), Src::from(rcp)],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
            }
            BinaryOp::Modulo => {
                // fmod(a, b) = a - floor(a/b) * b
                // a/b via rcp
                let rcp = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental {
                    dst: rcp.into(),
                    op: TranscendentalOp::Rcp,
                    src: Src::from(r[0]),
                }));
                let quot = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: quot.into(),
                    srcs: [Src::from(l[0]), Src::from(rcp)],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                // floor(quot)
                let floored_i = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpF2I {
                    dst: floored_i.into(),
                    src: Src::from(quot),
                    src_type: FloatType::F32,
                    dst_type: IntType::I32,
                    rnd_mode: FRndMode::NegInf,
                    ftz: false,
                }));
                let floored = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpI2F {
                    dst: floored.into(),
                    src: Src::from(floored_i),
                    dst_type: FloatType::F32,
                    src_type: IntType::I32,
                    rnd_mode: FRndMode::NearestEven,
                }));
                // a - floor(a/b) * b  via  fma(-floor, b, a)
                self.push_instr(Instr::new(OpFFma {
                    dst: dst.into(),
                    srcs: [Src::from(floored).fneg(), Src::from(r[0]), Src::from(l[0])],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
            }
            BinaryOp::Equal | BinaryOp::NotEqual | BinaryOp::Less
            | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
                let pred = self.alloc_ssa(RegFile::Pred);
                let cmp_op = match op {
                    BinaryOp::Equal => FloatCmpOp::OrdEq,
                    BinaryOp::NotEqual => FloatCmpOp::UnordNe,
                    BinaryOp::Less => FloatCmpOp::OrdLt,
                    BinaryOp::LessEqual => FloatCmpOp::OrdLe,
                    BinaryOp::Greater => FloatCmpOp::OrdGt,
                    BinaryOp::GreaterEqual => FloatCmpOp::OrdGe,
                    _ => unreachable!(),
                };
                self.push_instr(Instr::new(OpFSetP {
                    dst: pred.into(),
                    set_op: PredSetOp::And,
                    cmp_op,
                    srcs: [Src::from(l[0]), Src::from(r[0]), SrcRef::True.into()],
                    ftz: false,
                }));
                self.push_instr(Instr::new(OpSel {
                    dst: dst.into(),
                    srcs: [Src::from(pred), Src::new_imm_u32(F32_ONE), Src::ZERO],
                }));
            }
            BinaryOp::And => {
                let pred_l = self.alloc_ssa(RegFile::Pred);
                self.push_instr(Instr::new(OpISetP {
                    dst: pred_l.into(),
                    set_op: PredSetOp::And,
                    cmp_op: IntCmpOp::Ne,
                    cmp_type: IntCmpType::U32,
                    ex: false,
                    srcs: [Src::from(l[0]), Src::ZERO, SrcRef::True.into(), SrcRef::True.into()],
                }));
                let pred_r = self.alloc_ssa(RegFile::Pred);
                self.push_instr(Instr::new(OpISetP {
                    dst: pred_r.into(),
                    set_op: PredSetOp::And,
                    cmp_op: IntCmpOp::Ne,
                    cmp_type: IntCmpType::U32,
                    ex: false,
                    srcs: [Src::from(r[0]), Src::ZERO, Src::from(pred_l), SrcRef::True.into()],
                }));
                self.push_instr(Instr::new(OpSel {
                    dst: dst.into(),
                    srcs: [Src::from(pred_r), Src::new_imm_u32(1), Src::ZERO],
                }));
            }
            BinaryOp::Or => {
                let pred_l = self.alloc_ssa(RegFile::Pred);
                self.push_instr(Instr::new(OpISetP {
                    dst: pred_l.into(),
                    set_op: PredSetOp::And,
                    cmp_op: IntCmpOp::Ne,
                    cmp_type: IntCmpType::U32,
                    ex: false,
                    srcs: [Src::from(l[0]), Src::ZERO, SrcRef::True.into(), SrcRef::True.into()],
                }));
                let pred_r = self.alloc_ssa(RegFile::Pred);
                self.push_instr(Instr::new(OpISetP {
                    dst: pred_r.into(),
                    set_op: PredSetOp::Or,
                    cmp_op: IntCmpOp::Ne,
                    cmp_type: IntCmpType::U32,
                    ex: false,
                    srcs: [Src::from(r[0]), Src::ZERO, Src::from(pred_l), SrcRef::True.into()],
                }));
                self.push_instr(Instr::new(OpSel {
                    dst: dst.into(),
                    srcs: [Src::from(pred_r), Src::new_imm_u32(1), Src::ZERO],
                }));
            }
            BinaryOp::BitwiseAnd => {
                self.push_instr(Instr::new(OpLop2 {
                    dst: dst.into(),
                    srcs: [Src::from(l[0]), Src::from(r[0])],
                    op: LogicOp2::And,
                }));
            }
            BinaryOp::BitwiseOr => {
                self.push_instr(Instr::new(OpLop2 {
                    dst: dst.into(),
                    srcs: [Src::from(l[0]), Src::from(r[0])],
                    op: LogicOp2::Or,
                }));
            }
            BinaryOp::BitwiseXor => {
                self.push_instr(Instr::new(OpLop2 {
                    dst: dst.into(),
                    srcs: [Src::from(l[0]), Src::from(r[0])],
                    op: LogicOp2::Xor,
                }));
            }
            BinaryOp::ShiftLeft => {
                self.push_instr(Instr::new(OpShl {
                    dst: dst.into(),
                    srcs: [Src::from(l[0]), Src::from(r[0])],
                    wrap: false,
                }));
            }
            BinaryOp::ShiftRight => {
                self.push_instr(Instr::new(OpShr {
                    dst: dst.into(),
                    srcs: [Src::from(l[0]), Src::from(r[0])],
                    wrap: false,
                    signed: false,
                }));
            }
        }
        Ok(dst.into())
    }

    pub(crate) fn lower_unary(&mut self, op: UnaryOp, val: SSARef) -> Result<SSARef, CompileError> {
        let dst = self.alloc_ssa(RegFile::GPR);
        match op {
            UnaryOp::Negate => {
                self.push_instr(Instr::new(OpFAdd {
                    dst: dst.into(),
                    srcs: [Src::ZERO, Src::from(val[0]).fneg()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }));
            }
            UnaryOp::BitwiseNot => {
                // ~x = x XOR 0xFFFFFFFF
                let ones = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy {
                    dst: ones.into(),
                    src: Src::new_imm_u32(0xFFFF_FFFF),
                }));
                self.push_instr(Instr::new(OpLop2 {
                    dst: dst.into(),
                    srcs: [Src::from(val[0]), Src::from(ones)],
                    op: LogicOp2::Xor,
                }));
            }
            UnaryOp::LogicalNot => {
                let pred = self.alloc_ssa(RegFile::Pred);
                self.push_instr(Instr::new(OpISetP {
                    dst: pred.into(),
                    set_op: PredSetOp::And,
                    cmp_op: IntCmpOp::Eq,
                    cmp_type: IntCmpType::U32,
                    ex: false,
                    srcs: [Src::from(val[0]), Src::ZERO, SrcRef::True.into(), SrcRef::True.into()],
                }));
                self.push_instr(Instr::new(OpSel {
                    dst: dst.into(),
                    srcs: [Src::from(pred), Src::new_imm_u32(1), Src::ZERO],
                }));
            }
        }
        Ok(dst.into())
    }
}
