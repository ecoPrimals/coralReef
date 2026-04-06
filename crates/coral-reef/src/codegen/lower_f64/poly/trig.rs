// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)
//! f64 sin/cos via minimax polynomial + Cody-Waite range reduction.

use super::super::*;
use super::{emit_cody_waite_reduction, emit_f64_sel};
use crate::codegen::ir::{IntCmpOp, IntCmpType, LogicOp2, PredSetOp};

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
        Instr::new(OpDMul {
            dst: r_sq.clone().into(),
            srcs: [r_src.clone(), r_src.clone()],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let r_sq_src = Src::from(r_sq);
    let s1 = emit_f64_const(out, alloc, pred, S1);
    let s2 = emit_f64_const(out, alloc, pred, S2);
    let s3 = emit_f64_const(out, alloc, pred, S3);
    let s4 = emit_f64_const(out, alloc, pred, S4);
    let inner = emit_f64_dfma(
        out,
        alloc,
        pred,
        r_sq_src.clone(),
        Src::from(s4),
        Src::from(s3),
    );
    let inner = emit_f64_dfma(
        out,
        alloc,
        pred,
        r_sq_src.clone(),
        Src::from(inner),
        Src::from(s2),
    );
    let inner = emit_f64_dfma(
        out,
        alloc,
        pred,
        r_sq_src.clone(),
        Src::from(inner),
        Src::from(s1),
    );
    let one = emit_f64_const(out, alloc, pred, 1.0);
    let poly = emit_f64_dfma(out, alloc, pred, r_sq_src, Src::from(inner), Src::from(one));
    let result = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: result.clone().into(),
            srcs: [r_src, Src::from(poly)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    result
}

pub fn lower_f64_sin(
    op: &OpF64Sin,
    pred: Pred,
    alloc: &mut SSAValueAllocator,
    _sm: &dyn ShaderModel,
) -> Vec<Instr> {
    let mut out = Vec::new();
    let rnd = FRndMode::NearestEven;

    let x = op.src.reference.clone().to_ssa();
    assert!(x.comps() == 2, "f64 sin src must have 2 components");
    let x_src = Src::from(x);

    let x_sq = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: x_sq.clone().into(),
            srcs: [x_src.clone(), x_src.clone()],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let x_sq_src = Src::from(x_sq);

    let s1 = emit_f64_const(&mut out, alloc, pred, S1);
    let s2 = emit_f64_const(&mut out, alloc, pred, S2);
    let s3 = emit_f64_const(&mut out, alloc, pred, S3);
    let s4 = emit_f64_const(&mut out, alloc, pred, S4);

    let inner = emit_f64_dfma(
        &mut out,
        alloc,
        pred,
        x_sq_src.clone(),
        Src::from(s4),
        Src::from(s3),
    );
    let inner = emit_f64_dfma(
        &mut out,
        alloc,
        pred,
        x_sq_src.clone(),
        Src::from(inner),
        Src::from(s2),
    );
    let inner = emit_f64_dfma(
        &mut out,
        alloc,
        pred,
        x_sq_src.clone(),
        Src::from(inner),
        Src::from(s1),
    );

    let one = emit_f64_const(&mut out, alloc, pred, 1.0);
    let poly = emit_f64_dfma(
        &mut out,
        alloc,
        pred,
        x_sq_src,
        Src::from(inner),
        Src::from(one),
    );

    out.push(with_pred(
        Instr::new(OpDMul {
            dst: op.dst.clone(),
            srcs: [x_src, Src::from(poly)],
            rnd_mode: rnd,
        }),
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
        Instr::new(OpDMul {
            dst: r_sq.clone().into(),
            srcs: [r_src.clone(), r_src.clone()],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let r_sq_src = Src::from(r_sq);
    let c1 = emit_f64_const(out, alloc, pred, COS_C1);
    let c2 = emit_f64_const(out, alloc, pred, COS_C2);
    let c3 = emit_f64_const(out, alloc, pred, COS_C3);
    let c4 = emit_f64_const(out, alloc, pred, COS_C4);
    let inner = emit_f64_dfma(
        out,
        alloc,
        pred,
        r_sq_src.clone(),
        Src::from(c4),
        Src::from(c3),
    );
    let inner = emit_f64_dfma(
        out,
        alloc,
        pred,
        r_sq_src.clone(),
        Src::from(inner),
        Src::from(c2),
    );
    let inner = emit_f64_dfma(
        out,
        alloc,
        pred,
        r_sq_src.clone(),
        Src::from(inner),
        Src::from(c1),
    );
    let one = emit_f64_const(out, alloc, pred, 1.0);
    emit_f64_dfma(out, alloc, pred, r_sq_src, Src::from(inner), Src::from(one))
}

pub fn lower_f64_cos(
    op: &OpF64Cos,
    pred: Pred,
    alloc: &mut SSAValueAllocator,
    _sm: &dyn ShaderModel,
) -> Vec<Instr> {
    let mut out = Vec::new();
    let rnd = FRndMode::NearestEven;

    let x = op.src.reference.clone().to_ssa();
    assert!(x.comps() == 2, "f64 cos src must have 2 components");
    let x_src = Src::from(x);

    let (r, n_i32) = emit_cody_waite_reduction(&mut out, alloc, pred, x_src.clone());
    let r_src = Src::from(r);

    let sin_r = emit_sin_poly(&mut out, alloc, pred, r_src.clone());
    let cos_r = emit_cos_poly(&mut out, alloc, pred, r_src);

    // Quadrant correction: need_sin = (n & 1) != 0, need_negate = ((n+1) & 2) != 0
    let n_and_1 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpLop2 {
            dst: n_and_1.into(),
            srcs: [n_i32.into(), Src::new_imm_u32(1)],
            op: LogicOp2::And,
        }),
        pred,
    ));
    let p_sin = alloc.alloc(RegFile::Pred);
    out.push(with_pred(
        Instr::new(OpISetP {
            dst: p_sin.into(),
            set_op: PredSetOp::And,
            cmp_op: IntCmpOp::Ne,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [
                n_and_1.into(),
                Src::ZERO,
                Src::new_imm_bool(true),
                Src::new_imm_bool(true),
            ],
        }),
        pred,
    ));
    let n_plus_1 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpIAdd3 {
            dsts: [n_plus_1.into(), Dst::None, Dst::None],
            srcs: [n_i32.into(), Src::new_imm_u32(1), Src::ZERO],
        }),
        pred,
    ));
    let n_plus_1_and_2 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpLop2 {
            dst: n_plus_1_and_2.into(),
            srcs: [n_plus_1.into(), Src::new_imm_u32(2)],
            op: LogicOp2::And,
        }),
        pred,
    ));
    let p_neg = alloc.alloc(RegFile::Pred);
    out.push(with_pred(
        Instr::new(OpISetP {
            dst: p_neg.into(),
            set_op: PredSetOp::And,
            cmp_op: IntCmpOp::Ne,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [
                n_plus_1_and_2.into(),
                Src::ZERO,
                Src::new_imm_bool(true),
                Src::new_imm_bool(true),
            ],
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

    let dst_ssa = op.dst.as_ssa().expect("trig destination must be SSA value");
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: dst_ssa[0].into(),
            src: result[0].into(),
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: dst_ssa[1].into(),
            src: result[1].into(),
        }),
        pred,
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::super::super::*;
    use crate::codegen::ir::{OpF64Cos, OpF64Sin};

    fn make_sm70() -> ShaderModelInfo {
        ShaderModelInfo::new(70, 64)
    }

    #[test]
    fn test_sin_polynomial_coefficients() {
        const S1: f64 = -1.0 / 6.0;
        const S2: f64 = 1.0 / 120.0;
        const S3: f64 = -1.0 / 5040.0;
        const S4: f64 = 1.0 / 362_880.0;
        let coeffs = [S1, S2, S3, S4];
        assert_eq!(coeffs.len(), 4);
        assert!((S1 - (-1.0 / 6.0)).abs() < 1e-15);
        assert!((S2 - (1.0 / 120.0)).abs() < 1e-15);
        assert!((S3 - (-1.0 / 5040.0)).abs() < 1e-15);
        assert!((S4 - (1.0 / 362_880.0)).abs() < 1e-15);
        const { assert!(S1 < 0.0 && S2 > 0.0 && S3 < 0.0 && S4 > 0.0) }
    }

    #[test]
    fn test_cos_polynomial_coefficients() {
        const COS_C1: f64 = -1.0 / 2.0;
        const COS_C2: f64 = 1.0 / 24.0;
        const COS_C3: f64 = -1.0 / 720.0;
        const COS_C4: f64 = 1.0 / 40320.0;
        let coeffs = [COS_C1, COS_C2, COS_C3, COS_C4];
        assert_eq!(coeffs.len(), 4);
        assert!((COS_C1 - (-0.5)).abs() < 1e-15);
        assert!((COS_C2 - (1.0 / 24.0)).abs() < 1e-15);
        assert!((COS_C3 - (-1.0 / 720.0)).abs() < 1e-15);
        assert!((COS_C4 - (1.0 / 40320.0)).abs() < 1e-15);
    }

    #[test]
    fn test_sin_polynomial_at_zero() {
        // sin(x) ≈ x * (1 + x²*(s1 + x²*(s2 + x²*(s3 + x²*s4))))
        // At x=0: sin(0) = 0
        const S1: f64 = -1.0 / 6.0;
        const S2: f64 = 1.0 / 120.0;
        const S3: f64 = -1.0 / 5040.0;
        const S4: f64 = 1.0 / 362_880.0;
        let x = 0.0_f64;
        let x_sq = x * x;
        let inner = x_sq.mul_add(S4, S3);
        let inner = x_sq.mul_add(inner, S2);
        let inner = x_sq.mul_add(inner, S1);
        let poly = x_sq.mul_add(inner, 1.0);
        let result = x * poly;
        assert!((result - 0.0).abs() < 1e-15);
    }

    #[test]
    fn test_sin_polynomial_small_angle() {
        const S1: f64 = -1.0 / 6.0;
        const S2: f64 = 1.0 / 120.0;
        const S3: f64 = -1.0 / 5040.0;
        const S4: f64 = 1.0 / 362_880.0;
        let x = 0.1_f64;
        let x_sq = x * x;
        let inner = x_sq.mul_add(S4, S3);
        let inner = x_sq.mul_add(inner, S2);
        let inner = x_sq.mul_add(inner, S1);
        let poly = x_sq.mul_add(inner, 1.0);
        let result = x * poly;
        let expected = x.sin();
        assert!(
            (result - expected).abs() < 1e-10,
            "sin poly at {x}: got {result}, expected {expected}"
        );
    }

    #[test]
    fn test_cos_polynomial_at_zero() {
        // cos(x) ≈ 1 + x²*(c1 + x²*(c2 + x²*(c3 + x²*c4)))
        const COS_C1: f64 = -1.0 / 2.0;
        const COS_C2: f64 = 1.0 / 24.0;
        const COS_C3: f64 = -1.0 / 720.0;
        const COS_C4: f64 = 1.0 / 40320.0;
        let x = 0.0_f64;
        let x_sq = x * x;
        let inner = x_sq.mul_add(COS_C4, COS_C3);
        let inner = x_sq.mul_add(inner, COS_C2);
        let inner = x_sq.mul_add(inner, COS_C1);
        let result = x_sq.mul_add(inner, 1.0);
        assert!((result - 1.0).abs() < 1e-15);
    }

    #[test]
    fn test_f64_sin_lowering_produces_dfma_sequence() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);
        let op = OpF64Sin {
            dst: dst.into(),
            src: Src::from(x),
        };
        let instr = Instr::new(op);
        let result = super::super::super::lower_instr(instr, &mut alloc, &sm);
        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions")
        };
        assert!(seq.len() > 5, "sin should expand to multiple instructions");
        assert!(
            seq.iter().any(|i| matches!(i.op, Op::DFma(_))),
            "sin lowering should use DFMA"
        );
        assert!(
            seq.iter().any(|i| matches!(i.op, Op::DMul(_))),
            "sin lowering should use DMul"
        );
    }

    #[test]
    fn test_f64_sin_lowering_includes_cody_waite_range_reduction() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);
        let op = OpF64Sin {
            dst: dst.into(),
            src: Src::from(x),
        };
        let instr = Instr::new(op);
        let result = super::super::super::lower_instr(instr, &mut alloc, &sm);
        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions")
        };
        assert!(
            seq.iter().any(|i| matches!(i.op, Op::DMul(_))),
            "sin lowering should use DMul for x * 2/π"
        );
        assert!(
            seq.iter().any(|i| matches!(i.op, Op::DFma(_))),
            "sin lowering should use DFMA for Cody-Waite"
        );
    }

    #[test]
    fn test_f64_sin_lowering_includes_quadrant_correction() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);
        let op = OpF64Sin {
            dst: dst.into(),
            src: Src::from(x),
        };
        let instr = Instr::new(op);
        let result = super::super::super::lower_instr(instr, &mut alloc, &sm);
        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions")
        };
        assert!(
            seq.iter().any(|i| matches!(i.op, Op::DFma(_))),
            "sin lowering should use DFMA"
        );
        assert!(
            seq.iter().any(|i| matches!(i.op, Op::DMul(_))),
            "sin lowering should use DMul for range reduction"
        );
    }

    #[test]
    fn test_f64_cos_lowering_produces_dfma_sequence() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);
        let op = OpF64Cos {
            dst: dst.into(),
            src: Src::from(x),
        };
        let instr = Instr::new(op);
        let result = super::super::super::lower_instr(instr, &mut alloc, &sm);
        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions")
        };
        assert!(seq.len() > 5, "cos should expand to multiple instructions");
        assert!(
            seq.iter().any(|i| matches!(i.op, Op::DFma(_))),
            "cos lowering should use DFMA"
        );
    }

    #[test]
    fn test_f64_cos_lowering_includes_cody_waite_range_reduction() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);
        let op = OpF64Cos {
            dst: dst.into(),
            src: Src::from(x),
        };
        let instr = Instr::new(op);
        let result = super::super::super::lower_instr(instr, &mut alloc, &sm);
        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions")
        };
        assert!(
            seq.iter().any(|i| matches!(i.op, Op::DMul(_))),
            "cos lowering should use DMul for x * 2/π"
        );
        assert!(
            seq.iter().any(|i| matches!(i.op, Op::DFma(_))),
            "cos lowering should use DFMA for Cody-Waite"
        );
    }

    #[test]
    fn test_f64_cos_lowering_includes_quadrant_correction() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);
        let op = OpF64Cos {
            dst: dst.into(),
            src: Src::from(x),
        };
        let instr = Instr::new(op);
        let result = super::super::super::lower_instr(instr, &mut alloc, &sm);
        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions")
        };
        assert!(
            seq.iter().any(|i| matches!(i.op, Op::DFma(_))),
            "cos lowering should use DFMA"
        );
        assert!(
            seq.iter().any(|i| matches!(i.op, Op::DMul(_))),
            "cos lowering should use DMul for range reduction"
        );
        assert!(
            seq.iter().any(|i| matches!(i.op, Op::F2I(_))),
            "cos lowering should use F2I for quadrant index"
        );
        assert!(
            seq.iter().any(|i| matches!(i.op, Op::I2F(_))),
            "cos lowering should use I2F for Cody-Waite"
        );
    }
}
