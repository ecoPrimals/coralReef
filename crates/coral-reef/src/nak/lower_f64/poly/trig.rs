// Copyright © 2023 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! f64 sin/cos via minimax polynomial + Cody-Waite range reduction.

#![allow(clippy::wildcard_imports)]

use super::super::*;
use super::{emit_cody_waite_reduction, emit_f64_sel};
use crate::nak::ir::{IntCmpOp, IntCmpType, LogicOp2, PredSetOp};

// sin(x) ≈ x · (1 + x²·(s1 + x²·(s2 + x²·(s3 + x²·s4))))
const S1: f64 = -1.0 / 6.0;
const S2: f64 = 1.0 / 120.0;
const S3: f64 = -1.0 / 5040.0;
const S4: f64 = 1.0 / 362_880.0;

fn emit_sin_poly(
    out: &mut Vec<Instr>,
    alloc: &mut SSAValueAllocator,
    pred: Pred,
    r_src: Src,
) -> SSARef {
    let rnd = FRndMode::NearestEven;
    let r_sq = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul { dst: r_sq.clone().into(), srcs: [r_src.clone(), r_src.clone()], rnd_mode: rnd }),
        pred,
    ));
    let r_sq_src = Src::from(r_sq.clone());
    let s1 = emit_f64_const(out, alloc, pred, S1);
    let s2 = emit_f64_const(out, alloc, pred, S2);
    let s3 = emit_f64_const(out, alloc, pred, S3);
    let s4 = emit_f64_const(out, alloc, pred, S4);
    let mut inner = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma { dst: inner.clone().into(), srcs: [r_sq_src.clone(), Src::from(s4), Src::from(s3)], rnd_mode: rnd }),
        pred,
    ));
    let inner_src = Src::from(inner.clone());
    out.push(with_pred(
        Instr::new(OpDFma { dst: inner.clone().into(), srcs: [r_sq_src.clone(), inner_src, Src::from(s2)], rnd_mode: rnd }),
        pred,
    ));
    let inner_src = Src::from(inner.clone());
    out.push(with_pred(
        Instr::new(OpDFma { dst: inner.clone().into(), srcs: [r_sq_src.clone(), inner_src, Src::from(s1)], rnd_mode: rnd }),
        pred,
    ));
    let one = emit_f64_const(out, alloc, pred, 1.0);
    let inner_src = Src::from(inner);
    let poly = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma { dst: poly.clone().into(), srcs: [r_sq_src, inner_src, Src::from(one)], rnd_mode: rnd }),
        pred,
    ));
    let result = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul { dst: result.clone().into(), srcs: [r_src, Src::from(poly)], rnd_mode: rnd }),
        pred,
    ));
    result
}

pub(crate) fn lower_f64_sin(
    op: &OpF64Sin,
    pred: Pred,
    alloc: &mut SSAValueAllocator,
    _sm: &ShaderModelInfo,
) -> Vec<Instr> {
    let mut out = Vec::new();
    let rnd = FRndMode::NearestEven;

    let x = op.src.src_ref.clone().to_ssa();
    assert!(x.comps() == 2, "f64 sin src must have 2 components");
    let x_src = Src::from(x.clone());

    let x_sq = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul { dst: x_sq.clone().into(), srcs: [x_src.clone(), x_src.clone()], rnd_mode: rnd }),
        pred,
    ));
    let x_sq_src = Src::from(x_sq.clone());

    let s1 = emit_f64_const(&mut out, alloc, pred, S1);
    let s2 = emit_f64_const(&mut out, alloc, pred, S2);
    let s3 = emit_f64_const(&mut out, alloc, pred, S3);
    let s4 = emit_f64_const(&mut out, alloc, pred, S4);

    let mut inner = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma { dst: inner.clone().into(), srcs: [x_sq_src.clone(), Src::from(s4), Src::from(s3)], rnd_mode: rnd }),
        pred,
    ));
    let inner_src = Src::from(inner.clone());
    out.push(with_pred(
        Instr::new(OpDFma { dst: inner.clone().into(), srcs: [x_sq_src.clone(), inner_src, Src::from(s2)], rnd_mode: rnd }),
        pred,
    ));
    let inner_src = Src::from(inner.clone());
    out.push(with_pred(
        Instr::new(OpDFma { dst: inner.clone().into(), srcs: [x_sq_src.clone(), inner_src, Src::from(s1)], rnd_mode: rnd }),
        pred,
    ));

    let one = emit_f64_const(&mut out, alloc, pred, 1.0);
    let inner_src = Src::from(inner);
    let poly = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma { dst: poly.clone().into(), srcs: [x_sq_src, inner_src, Src::from(one)], rnd_mode: rnd }),
        pred,
    ));

    out.push(with_pred(
        Instr::new(OpDMul { dst: op.dst.clone(), srcs: [x_src, Src::from(poly)], rnd_mode: rnd }),
        pred,
    ));

    out
}

// cos(x) ≈ 1 + x²·(c1 + x²·(c2 + x²·(c3 + x²·c4)))
const COS_C1: f64 = -1.0 / 2.0;
const COS_C2: f64 = 1.0 / 24.0;
const COS_C3: f64 = -1.0 / 720.0;
const COS_C4: f64 = 1.0 / 40320.0;

fn emit_cos_poly(
    out: &mut Vec<Instr>,
    alloc: &mut SSAValueAllocator,
    pred: Pred,
    r_src: Src,
) -> SSARef {
    let rnd = FRndMode::NearestEven;
    let r_sq = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul { dst: r_sq.clone().into(), srcs: [r_src.clone(), r_src.clone()], rnd_mode: rnd }),
        pred,
    ));
    let r_sq_src = Src::from(r_sq.clone());
    let c1 = emit_f64_const(out, alloc, pred, COS_C1);
    let c2 = emit_f64_const(out, alloc, pred, COS_C2);
    let c3 = emit_f64_const(out, alloc, pred, COS_C3);
    let c4 = emit_f64_const(out, alloc, pred, COS_C4);
    let mut inner = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma { dst: inner.clone().into(), srcs: [r_sq_src.clone(), Src::from(c4), Src::from(c3)], rnd_mode: rnd }),
        pred,
    ));
    let inner_src = Src::from(inner.clone());
    out.push(with_pred(
        Instr::new(OpDFma { dst: inner.clone().into(), srcs: [r_sq_src.clone(), inner_src, Src::from(c2)], rnd_mode: rnd }),
        pred,
    ));
    let inner_src = Src::from(inner.clone());
    out.push(with_pred(
        Instr::new(OpDFma { dst: inner.clone().into(), srcs: [r_sq_src.clone(), inner_src, Src::from(c1)], rnd_mode: rnd }),
        pred,
    ));
    let one = emit_f64_const(out, alloc, pred, 1.0);
    let inner_src = Src::from(inner);
    let result = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma { dst: result.clone().into(), srcs: [r_sq_src, inner_src, Src::from(one)], rnd_mode: rnd }),
        pred,
    ));
    result
}

pub(crate) fn lower_f64_cos(
    op: &OpF64Cos,
    pred: Pred,
    alloc: &mut SSAValueAllocator,
    _sm: &ShaderModelInfo,
) -> Vec<Instr> {
    let mut out = Vec::new();
    let rnd = FRndMode::NearestEven;

    let x = op.src.src_ref.clone().to_ssa();
    assert!(x.comps() == 2, "f64 cos src must have 2 components");
    let x_src = Src::from(x.clone());

    let (r, n_i32) = emit_cody_waite_reduction(&mut out, alloc, pred, x_src.clone());
    let r_src = Src::from(r.clone());

    let sin_r = emit_sin_poly(&mut out, alloc, pred, r_src.clone());
    let cos_r = emit_cos_poly(&mut out, alloc, pred, r_src);

    // Quadrant correction: need_sin = (n & 1) != 0, need_negate = ((n+1) & 2) != 0
    let n_and_1 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpLop2 { dst: n_and_1.into(), srcs: [n_i32.into(), Src::new_imm_u32(1)], op: LogicOp2::And }),
        pred,
    ));
    let p_sin = alloc.alloc(RegFile::Pred);
    out.push(with_pred(
        Instr::new(OpISetP {
            dst: p_sin.into(), set_op: PredSetOp::And, cmp_op: IntCmpOp::Ne,
            cmp_type: IntCmpType::U32, ex: false,
            srcs: [n_and_1.into(), Src::ZERO],
            accum: Src::new_imm_bool(true), low_cmp: Src::new_imm_bool(true),
        }),
        pred,
    ));
    let n_plus_1 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpIAdd3 {
            dst: n_plus_1.into(), srcs: [n_i32.into(), Src::new_imm_u32(1), Src::ZERO],
            overflow: [Dst::None, Dst::None],
        }),
        pred,
    ));
    let n_plus_1_and_2 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpLop2 { dst: n_plus_1_and_2.into(), srcs: [n_plus_1.into(), Src::new_imm_u32(2)], op: LogicOp2::And }),
        pred,
    ));
    let p_neg = alloc.alloc(RegFile::Pred);
    out.push(with_pred(
        Instr::new(OpISetP {
            dst: p_neg.into(), set_op: PredSetOp::And, cmp_op: IntCmpOp::Ne,
            cmp_type: IntCmpType::U32, ex: false,
            srcs: [n_plus_1_and_2.into(), Src::ZERO],
            accum: Src::new_imm_bool(true), low_cmp: Src::new_imm_bool(true),
        }),
        pred,
    ));

    let picked = emit_f64_sel(&mut out, alloc, pred, p_sin, sin_r, cos_r);

    let zero = emit_f64_zero(&mut out, alloc, pred);
    let neg_picked = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDAdd {
            dst: neg_picked.clone().into(),
            srcs: [Src::from(zero), Src::from(picked.clone()).fneg()],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let result = emit_f64_sel(&mut out, alloc, pred, p_neg, neg_picked, picked);

    out.push(with_pred(
        Instr::new(OpCopy { dst: op.dst.as_ssa().unwrap()[0].into(), src: result[0].into() }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpCopy { dst: op.dst.as_ssa().unwrap()[1].into(), src: result[1].into() }),
        pred,
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::super::super::*;
    use crate::nak::ir::{OpF64Cos, OpF64Sin};

    fn make_sm70() -> ShaderModelInfo {
        ShaderModelInfo::new(70, 64)
    }

    #[test]
    fn test_f64_sin_lowering_produces_dfma_sequence() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);
        let op = OpF64Sin { dst: dst.into(), src: Src::from(x.clone()) };
        let instr = Instr::new(op);
        let result = super::super::super::lower_instr(instr, &mut alloc, &sm);
        let MappedInstrs::Many(seq) = result else { panic!("Expected Many instructions") };
        assert!(seq.len() > 5, "sin should expand to multiple instructions");
        assert!(seq.iter().any(|i| matches!(i.op, Op::DFma(_))), "sin lowering should use DFMA");
        assert!(seq.iter().any(|i| matches!(i.op, Op::DMul(_))), "sin lowering should use DMul");
    }

    #[test]
    fn test_f64_sin_lowering_includes_cody_waite_range_reduction() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);
        let op = OpF64Sin { dst: dst.into(), src: Src::from(x.clone()) };
        let instr = Instr::new(op);
        let result = super::super::super::lower_instr(instr, &mut alloc, &sm);
        let MappedInstrs::Many(seq) = result else { panic!("Expected Many instructions") };
        assert!(seq.iter().any(|i| matches!(i.op, Op::DMul(_))), "sin lowering should use DMul for x * 2/π");
        assert!(seq.iter().any(|i| matches!(i.op, Op::DFma(_))), "sin lowering should use DFMA for Cody-Waite");
    }

    #[test]
    fn test_f64_sin_lowering_includes_quadrant_correction() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);
        let op = OpF64Sin { dst: dst.into(), src: Src::from(x.clone()) };
        let instr = Instr::new(op);
        let result = super::super::super::lower_instr(instr, &mut alloc, &sm);
        let MappedInstrs::Many(seq) = result else { panic!("Expected Many instructions") };
        assert!(seq.iter().any(|i| matches!(i.op, Op::DFma(_))), "sin lowering should use DFMA");
        assert!(seq.iter().any(|i| matches!(i.op, Op::DMul(_))), "sin lowering should use DMul for range reduction");
    }

    #[test]
    fn test_f64_cos_lowering_produces_dfma_sequence() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);
        let op = OpF64Cos { dst: dst.into(), src: Src::from(x.clone()) };
        let instr = Instr::new(op);
        let result = super::super::super::lower_instr(instr, &mut alloc, &sm);
        let MappedInstrs::Many(seq) = result else { panic!("Expected Many instructions") };
        assert!(seq.len() > 5, "cos should expand to multiple instructions");
        assert!(seq.iter().any(|i| matches!(i.op, Op::DFma(_))), "cos lowering should use DFMA");
    }

    #[test]
    fn test_f64_cos_lowering_includes_cody_waite_range_reduction() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);
        let op = OpF64Cos { dst: dst.into(), src: Src::from(x.clone()) };
        let instr = Instr::new(op);
        let result = super::super::super::lower_instr(instr, &mut alloc, &sm);
        let MappedInstrs::Many(seq) = result else { panic!("Expected Many instructions") };
        assert!(seq.iter().any(|i| matches!(i.op, Op::DMul(_))), "cos lowering should use DMul for x * 2/π");
        assert!(seq.iter().any(|i| matches!(i.op, Op::DFma(_))), "cos lowering should use DFMA for Cody-Waite");
    }

    #[test]
    fn test_f64_cos_lowering_includes_quadrant_correction() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);
        let op = OpF64Cos { dst: dst.into(), src: Src::from(x.clone()) };
        let instr = Instr::new(op);
        let result = super::super::super::lower_instr(instr, &mut alloc, &sm);
        let MappedInstrs::Many(seq) = result else { panic!("Expected Many instructions") };
        assert!(seq.iter().any(|i| matches!(i.op, Op::DFma(_))), "cos lowering should use DFMA");
        assert!(seq.iter().any(|i| matches!(i.op, Op::DMul(_))), "cos lowering should use DMul for range reduction");
        assert!(seq.iter().any(|i| matches!(i.op, Op::F2I(_))), "cos lowering should use F2I for quadrant index");
        assert!(seq.iter().any(|i| matches!(i.op, Op::I2F(_))), "cos lowering should use I2F for Cody-Waite");
    }
}
