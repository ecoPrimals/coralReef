// SPDX-License-Identifier: AGPL-3.0-only
//! Binary operator translation for Naga expressions.
#![allow(clippy::wildcard_imports, unreachable_patterns)]
use super::super::ir::*;
use super::func::FuncTranslator;
use crate::error::CompileError;
use naga::Handle;

impl<'a, 'b> FuncTranslator<'a, 'b> {
    pub(super) fn translate_binary(
        &mut self,
        op: naga::BinaryOperator,
        l: SSARef,
        r: SSARef,
        left_handle: Handle<naga::Expression>,
        _right_handle: Handle<naga::Expression>,
    ) -> Result<SSARef, CompileError> {
        let is_float = self.is_float_expr(left_handle);
        let is_f64 = self.is_f64_expr(left_handle);
        let comps = l.comps().max(1);

        match op {
            naga::BinaryOperator::Add if is_f64 => {
                let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                self.push_instr(Instr::new(OpDAdd {
                    dst: dst.clone().into(),
                    srcs: [Src::from(l), Src::from(r)],
                    rnd_mode: FRndMode::NearestEven,
                }));
                Ok(dst)
            }
            naga::BinaryOperator::Add if is_float => {
                self.emit_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    s.push_instr(Instr::new(OpFAdd {
                        dst: dst.into(),
                        srcs: [a.into(), b.into()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Add => self.emit_int_componentwise(comps, l, r, |s, a, b| {
                let dst = s.alloc_ssa(RegFile::GPR);
                if s.sm.sm() >= 70 {
                    s.push_instr(Instr::new(OpIAdd3 {
                        dsts: [dst.into(), Dst::None, Dst::None],
                        srcs: [a.into(), b.into(), Src::ZERO],
                    }));
                } else {
                    s.push_instr(Instr::new(OpIAdd2 {
                        dsts: [dst.into(), Dst::None],
                        srcs: [a.into(), b.into()],
                    }));
                }
                dst
            }),
            naga::BinaryOperator::Subtract if is_f64 => {
                let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                self.push_instr(Instr::new(OpDAdd {
                    dst: dst.clone().into(),
                    srcs: [Src::from(l), Src::from(r).fneg()],
                    rnd_mode: FRndMode::NearestEven,
                }));
                Ok(dst)
            }
            naga::BinaryOperator::Subtract if is_float => {
                self.emit_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    s.push_instr(Instr::new(OpFAdd {
                        dst: dst.into(),
                        srcs: [a.into(), Src::from(b).fneg()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Subtract => {
                self.emit_int_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    if s.sm.sm() >= 70 {
                        s.push_instr(Instr::new(OpIAdd3 {
                            dsts: [dst.into(), Dst::None, Dst::None],
                            srcs: [a.into(), Src::from(b).ineg(), Src::ZERO],
                        }));
                    } else {
                        s.push_instr(Instr::new(OpIAdd2 {
                            dsts: [dst.into(), Dst::None],
                            srcs: [a.into(), Src::from(b).ineg()],
                        }));
                    }
                    dst
                })
            }
            naga::BinaryOperator::Multiply if is_f64 => {
                let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                self.push_instr(Instr::new(OpDMul {
                    dst: dst.clone().into(),
                    srcs: [Src::from(l), Src::from(r)],
                    rnd_mode: FRndMode::NearestEven,
                }));
                Ok(dst)
            }
            naga::BinaryOperator::Multiply if is_float => {
                self.emit_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    s.push_instr(Instr::new(OpFMul {
                        dst: dst.into(),
                        srcs: [a.into(), b.into()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                        dnz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Multiply => {
                self.emit_int_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    if s.sm.sm() >= 70 {
                        s.push_instr(Instr::new(OpIMad {
                            dst: dst.into(),
                            srcs: [a.into(), b.into(), Src::ZERO],
                            signed: false,
                        }));
                    } else {
                        s.push_instr(Instr::new(OpIMul {
                            dst: dst.into(),
                            srcs: [a.into(), b.into()],
                            signed: [false; 2],
                            high: false,
                        }));
                    }
                    dst
                })
            }
            naga::BinaryOperator::Divide if is_f64 => {
                let rcp = self.alloc_ssa_vec(RegFile::GPR, 2);
                self.push_instr(Instr::new(OpF64Rcp {
                    dst: rcp.clone().into(),
                    src: Src::from(r),
                }));
                let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                self.push_instr(Instr::new(OpDMul {
                    dst: dst.clone().into(),
                    srcs: [Src::from(l), Src::from(rcp)],
                    rnd_mode: FRndMode::NearestEven,
                }));
                Ok(dst)
            }
            naga::BinaryOperator::Divide if is_float => {
                self.emit_componentwise(comps, l, r, |s, a, b| {
                    let rcp = s.alloc_ssa(RegFile::GPR);
                    s.push_instr(Instr::new(OpTranscendental {
                        dst: rcp.into(),
                        op: TranscendentalOp::Rcp,
                        src: b.into(),
                    }));
                    let dst = s.alloc_ssa(RegFile::GPR);
                    s.push_instr(Instr::new(OpFMul {
                        dst: dst.into(),
                        srcs: [a.into(), rcp.into()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                        dnz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Divide => self.emit_int_componentwise(comps, l, r, |s, a, b| {
                let fa = s.alloc_ssa(RegFile::GPR);
                s.push_instr(Instr::new(OpI2F {
                    dst: fa.into(),
                    src: a.into(),
                    dst_type: FloatType::F32,
                    src_type: IntType::I32,
                    rnd_mode: FRndMode::NearestEven,
                }));
                let fb = s.alloc_ssa(RegFile::GPR);
                s.push_instr(Instr::new(OpI2F {
                    dst: fb.into(),
                    src: b.into(),
                    dst_type: FloatType::F32,
                    src_type: IntType::I32,
                    rnd_mode: FRndMode::NearestEven,
                }));
                let rcp = s.alloc_ssa(RegFile::GPR);
                s.push_instr(Instr::new(OpTranscendental {
                    dst: rcp.into(),
                    op: TranscendentalOp::Rcp,
                    src: fb.into(),
                }));
                let quot_f = s.alloc_ssa(RegFile::GPR);
                s.push_instr(Instr::new(OpFMul {
                    dst: quot_f.into(),
                    srcs: [fa.into(), rcp.into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                let trunc = s.alloc_ssa(RegFile::GPR);
                s.push_instr(Instr::new(OpFRnd {
                    dst: trunc.into(),
                    src: quot_f.into(),
                    dst_type: FloatType::F32,
                    src_type: FloatType::F32,
                    rnd_mode: FRndMode::Zero,
                    ftz: false,
                }));
                let dst = s.alloc_ssa(RegFile::GPR);
                s.push_instr(Instr::new(OpF2I {
                    dst: dst.into(),
                    src: trunc.into(),
                    dst_type: IntType::I32,
                    src_type: FloatType::F32,
                    rnd_mode: FRndMode::Zero,
                    ftz: false,
                }));
                dst
            }),
            naga::BinaryOperator::Modulo if is_f64 => {
                let rcp = self.alloc_ssa_vec(RegFile::GPR, 2);
                self.push_instr(Instr::new(OpF64Rcp {
                    dst: rcp.clone().into(),
                    src: Src::from(r.clone()),
                }));
                let quot = self.alloc_ssa_vec(RegFile::GPR, 2);
                self.push_instr(Instr::new(OpDMul {
                    dst: quot.clone().into(),
                    srcs: [Src::from(l.clone()), Src::from(rcp)],
                    rnd_mode: FRndMode::NearestEven,
                }));
                let floored = self.emit_f64_floor(quot)?;
                let prod = self.alloc_ssa_vec(RegFile::GPR, 2);
                self.push_instr(Instr::new(OpDMul {
                    dst: prod.clone().into(),
                    srcs: [Src::from(floored), Src::from(r)],
                    rnd_mode: FRndMode::NearestEven,
                }));
                let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                self.push_instr(Instr::new(OpDAdd {
                    dst: dst.clone().into(),
                    srcs: [Src::from(l), Src::from(prod).fneg()],
                    rnd_mode: FRndMode::NearestEven,
                }));
                Ok(dst)
            }
            naga::BinaryOperator::Modulo if is_float => {
                self.emit_componentwise(comps, l, r, |s, a, b| {
                    let rcp = s.alloc_ssa(RegFile::GPR);
                    s.push_instr(Instr::new(OpTranscendental {
                        dst: rcp.into(),
                        op: TranscendentalOp::Rcp,
                        src: b.into(),
                    }));
                    let quot = s.alloc_ssa(RegFile::GPR);
                    s.push_instr(Instr::new(OpFMul {
                        dst: quot.into(),
                        srcs: [a.into(), rcp.into()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                        dnz: false,
                    }));
                    let floored = s.alloc_ssa(RegFile::GPR);
                    s.push_instr(Instr::new(OpFRnd {
                        dst: floored.into(),
                        src: quot.into(),
                        dst_type: FloatType::F32,
                        src_type: FloatType::F32,
                        rnd_mode: FRndMode::NegInf,
                        ftz: false,
                    }));
                    let prod = s.alloc_ssa(RegFile::GPR);
                    s.push_instr(Instr::new(OpFMul {
                        dst: prod.into(),
                        srcs: [floored.into(), b.into()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                        dnz: false,
                    }));
                    let dst = s.alloc_ssa(RegFile::GPR);
                    s.push_instr(Instr::new(OpFAdd {
                        dst: dst.into(),
                        srcs: [a.into(), Src::from(prod).fneg()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Modulo => self.emit_int_componentwise(comps, l, r, |s, a, b| {
                let fa = s.alloc_ssa(RegFile::GPR);
                s.push_instr(Instr::new(OpI2F {
                    dst: fa.into(),
                    src: a.into(),
                    dst_type: FloatType::F32,
                    src_type: IntType::I32,
                    rnd_mode: FRndMode::NearestEven,
                }));
                let fb = s.alloc_ssa(RegFile::GPR);
                s.push_instr(Instr::new(OpI2F {
                    dst: fb.into(),
                    src: b.into(),
                    dst_type: FloatType::F32,
                    src_type: IntType::I32,
                    rnd_mode: FRndMode::NearestEven,
                }));
                let rcp = s.alloc_ssa(RegFile::GPR);
                s.push_instr(Instr::new(OpTranscendental {
                    dst: rcp.into(),
                    op: TranscendentalOp::Rcp,
                    src: fb.into(),
                }));
                let quot_f = s.alloc_ssa(RegFile::GPR);
                s.push_instr(Instr::new(OpFMul {
                    dst: quot_f.into(),
                    srcs: [fa.into(), rcp.into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                let trunc = s.alloc_ssa(RegFile::GPR);
                s.push_instr(Instr::new(OpFRnd {
                    dst: trunc.into(),
                    src: quot_f.into(),
                    dst_type: FloatType::F32,
                    src_type: FloatType::F32,
                    rnd_mode: FRndMode::Zero,
                    ftz: false,
                }));
                let prod = s.alloc_ssa(RegFile::GPR);
                s.push_instr(Instr::new(OpFMul {
                    dst: prod.into(),
                    srcs: [trunc.into(), fb.into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                let rem_f = s.alloc_ssa(RegFile::GPR);
                s.push_instr(Instr::new(OpFAdd {
                    dst: rem_f.into(),
                    srcs: [fa.into(), Src::from(prod).fneg()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }));
                let dst = s.alloc_ssa(RegFile::GPR);
                s.push_instr(Instr::new(OpF2I {
                    dst: dst.into(),
                    src: rem_f.into(),
                    dst_type: IntType::I32,
                    src_type: FloatType::F32,
                    rnd_mode: FRndMode::Zero,
                    ftz: false,
                }));
                dst
            }),
            naga::BinaryOperator::And => self.emit_int_componentwise(comps, l, r, |s, a, b| {
                let dst = s.alloc_ssa(RegFile::GPR);
                if s.sm.sm() >= 70 {
                    s.push_instr(Instr::new(OpLop3 {
                        dst: dst.into(),
                        srcs: [a.into(), b.into(), Src::ZERO],
                        op: LogicOp3::new_lut(&|x, y, _| x & y),
                    }));
                } else {
                    s.push_instr(Instr::new(OpLop2 {
                        dst: dst.into(),
                        srcs: [a.into(), b.into()],
                        op: LogicOp2::And,
                    }));
                }
                dst
            }),
            naga::BinaryOperator::InclusiveOr => {
                self.emit_int_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    if s.sm.sm() >= 70 {
                        s.push_instr(Instr::new(OpLop3 {
                            dst: dst.into(),
                            srcs: [a.into(), b.into(), Src::ZERO],
                            op: LogicOp3::new_lut(&|x, y, _| x | y),
                        }));
                    } else {
                        s.push_instr(Instr::new(OpLop2 {
                            dst: dst.into(),
                            srcs: [a.into(), b.into()],
                            op: LogicOp2::Or,
                        }));
                    }
                    dst
                })
            }
            naga::BinaryOperator::ExclusiveOr => {
                self.emit_int_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    if s.sm.sm() >= 70 {
                        s.push_instr(Instr::new(OpLop3 {
                            dst: dst.into(),
                            srcs: [a.into(), b.into(), Src::ZERO],
                            op: LogicOp3::new_lut(&|x, y, _| x ^ y),
                        }));
                    } else {
                        s.push_instr(Instr::new(OpLop2 {
                            dst: dst.into(),
                            srcs: [a.into(), b.into()],
                            op: LogicOp2::Xor,
                        }));
                    }
                    dst
                })
            }
            naga::BinaryOperator::ShiftLeft => {
                self.emit_int_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    if s.sm.sm() >= 70 {
                        s.push_instr(Instr::new(OpShf {
                            dst: dst.into(),
                            srcs: [a.into(), Src::ZERO, b.into()],
                            right: false,
                            wrap: true,
                            data_type: IntType::I32,
                            dst_high: false,
                        }));
                    } else {
                        s.push_instr(Instr::new(OpShl {
                            dst: dst.into(),
                            srcs: [a.into(), b.into()],
                            wrap: true,
                        }));
                    }
                    dst
                })
            }
            naga::BinaryOperator::ShiftRight => {
                self.emit_int_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    if s.sm.sm() >= 70 {
                        s.push_instr(Instr::new(OpShf {
                            dst: dst.into(),
                            srcs: [Src::ZERO, a.into(), b.into()],
                            right: true,
                            wrap: true,
                            data_type: IntType::U32,
                            dst_high: true,
                        }));
                    } else {
                        s.push_instr(Instr::new(OpShr {
                            dst: dst.into(),
                            srcs: [a.into(), b.into()],
                            wrap: true,
                            signed: false,
                        }));
                    }
                    dst
                })
            }
            naga::BinaryOperator::Equal if is_f64 => {
                let dst = self.alloc_ssa(RegFile::Pred);
                self.push_instr(Instr::new(OpDSetP {
                    dst: dst.into(),
                    set_op: PredSetOp::And,
                    cmp_op: FloatCmpOp::OrdEq,
                    srcs: [Src::from(l), Src::from(r), SrcRef::True.into()],
                }));
                Ok(dst.into())
            }
            naga::BinaryOperator::Equal if is_float => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpFSetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: FloatCmpOp::OrdEq,
                        srcs: [a.into(), b.into(), SrcRef::True.into()],
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Equal => self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                let dst = s.alloc_ssa(RegFile::Pred);
                s.push_instr(Instr::new(OpISetP {
                    dst: dst.into(),
                    set_op: PredSetOp::And,
                    cmp_op: IntCmpOp::Eq,
                    cmp_type: IntCmpType::U32,
                    ex: false,
                    srcs: [a.into(), b.into(), true.into(), true.into()],
                }));
                dst
            }),
            naga::BinaryOperator::NotEqual if is_f64 => {
                let dst = self.alloc_ssa(RegFile::Pred);
                self.push_instr(Instr::new(OpDSetP {
                    dst: dst.into(),
                    set_op: PredSetOp::And,
                    cmp_op: FloatCmpOp::OrdNe,
                    srcs: [Src::from(l), Src::from(r), SrcRef::True.into()], // accum
                }));
                Ok(dst.into())
            }
            naga::BinaryOperator::NotEqual if is_float => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpFSetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: FloatCmpOp::OrdNe,
                        srcs: [a.into(), b.into(), SrcRef::True.into()],
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::NotEqual => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpISetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: IntCmpOp::Ne,
                        cmp_type: IntCmpType::U32,
                        ex: false,
                        srcs: [a.into(), b.into(), true.into(), true.into()],
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Less if is_f64 => {
                let dst = self.alloc_ssa(RegFile::Pred);
                self.push_instr(Instr::new(OpDSetP {
                    dst: dst.into(),
                    set_op: PredSetOp::And,
                    cmp_op: FloatCmpOp::OrdLt,
                    srcs: [Src::from(l), Src::from(r), SrcRef::True.into()],
                }));
                Ok(dst.into())
            }
            naga::BinaryOperator::Less if is_float => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpFSetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: FloatCmpOp::OrdLt,
                        srcs: [a.into(), b.into(), SrcRef::True.into()],
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Less => self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                let dst = s.alloc_ssa(RegFile::Pred);
                s.push_instr(Instr::new(OpISetP {
                    dst: dst.into(),
                    set_op: PredSetOp::And,
                    cmp_op: IntCmpOp::Lt,
                    cmp_type: IntCmpType::U32,
                    ex: false,
                    srcs: [a.into(), b.into(), true.into(), true.into()],
                }));
                dst
            }),
            naga::BinaryOperator::LessEqual if is_f64 => {
                let dst = self.alloc_ssa(RegFile::Pred);
                self.push_instr(Instr::new(OpDSetP {
                    dst: dst.into(),
                    set_op: PredSetOp::And,
                    cmp_op: FloatCmpOp::OrdLe,
                    srcs: [Src::from(l), Src::from(r), SrcRef::True.into()],
                }));
                Ok(dst.into())
            }
            naga::BinaryOperator::LessEqual if is_float => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpFSetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: FloatCmpOp::OrdLe,
                        srcs: [a.into(), b.into(), SrcRef::True.into()],
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::LessEqual => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpISetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: IntCmpOp::Le,
                        cmp_type: IntCmpType::U32,
                        ex: false,
                        srcs: [a.into(), b.into(), true.into(), true.into()],
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Greater if is_f64 => {
                let dst = self.alloc_ssa(RegFile::Pred);
                self.push_instr(Instr::new(OpDSetP {
                    dst: dst.into(),
                    set_op: PredSetOp::And,
                    cmp_op: FloatCmpOp::OrdGt,
                    srcs: [Src::from(l), Src::from(r), SrcRef::True.into()],
                }));
                Ok(dst.into())
            }
            naga::BinaryOperator::Greater if is_float => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpFSetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: FloatCmpOp::OrdGt,
                        srcs: [a.into(), b.into(), SrcRef::True.into()],
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Greater => self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                let dst = s.alloc_ssa(RegFile::Pred);
                s.push_instr(Instr::new(OpISetP {
                    dst: dst.into(),
                    set_op: PredSetOp::And,
                    cmp_op: IntCmpOp::Gt,
                    cmp_type: IntCmpType::U32,
                    ex: false,
                    srcs: [a.into(), b.into(), true.into(), true.into()],
                }));
                dst
            }),
            naga::BinaryOperator::GreaterEqual if is_f64 => {
                let dst = self.alloc_ssa(RegFile::Pred);
                self.push_instr(Instr::new(OpDSetP {
                    dst: dst.into(),
                    set_op: PredSetOp::And,
                    cmp_op: FloatCmpOp::OrdGe,
                    srcs: [Src::from(l), Src::from(r), SrcRef::True.into()],
                }));
                Ok(dst.into())
            }
            naga::BinaryOperator::GreaterEqual if is_float => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpFSetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: FloatCmpOp::OrdGe,
                        srcs: [a.into(), b.into(), SrcRef::True.into()],
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::GreaterEqual => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpISetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: IntCmpOp::Ge,
                        cmp_type: IntCmpType::U32,
                        ex: false,
                        srcs: [a.into(), b.into(), true.into(), true.into()],
                    }));
                    dst
                })
            }
            naga::BinaryOperator::LogicalAnd => {
                let dst = self.alloc_ssa(RegFile::Pred);
                if self.sm.sm() >= 70 {
                    self.push_instr(Instr::new(OpPLop3 {
                        dsts: [dst.into(), Dst::None],
                        srcs: [l[0].into(), r[0].into(), true.into()],
                        ops: [
                            LogicOp3::new_lut(&|x, y, _| x & y),
                            LogicOp3::new_const(false),
                        ],
                    }));
                } else {
                    self.push_instr(Instr::new(OpPSetP {
                        dsts: [dst.into(), Dst::None],
                        ops: [PredSetOp::And, PredSetOp::And],
                        srcs: [l[0].into(), r[0].into(), true.into()],
                    }));
                }
                Ok(dst.into())
            }
            naga::BinaryOperator::LogicalOr => {
                let dst = self.alloc_ssa(RegFile::Pred);
                if self.sm.sm() >= 70 {
                    self.push_instr(Instr::new(OpPLop3 {
                        dsts: [dst.into(), Dst::None],
                        srcs: [l[0].into(), r[0].into(), true.into()],
                        ops: [
                            LogicOp3::new_lut(&|x, y, _| x | y),
                            LogicOp3::new_const(false),
                        ],
                    }));
                } else {
                    self.push_instr(Instr::new(OpPSetP {
                        dsts: [dst.into(), Dst::None],
                        ops: [PredSetOp::Or, PredSetOp::And],
                        srcs: [l[0].into(), r[0].into(), true.into()],
                    }));
                }
                Ok(dst.into())
            }
            _ => Err(CompileError::NotImplemented(
                format!("binary op {op:?} not yet supported").into(),
            )),
        }
    }
}
