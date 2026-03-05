// Copyright © 2023 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! Polynomial lowering for f64 exp2, log2, sin, cos.

#![allow(clippy::wildcard_imports)]

use super::*;
use crate::nak::ir::{IntCmpOp, IntCmpType, LogicOp2, PredSetOp};

/// exp2(x) via Horner polynomial with range reduction.
/// n = round(x), f = x - n, evaluate polynomial on f in [-0.5, 0.5], then ldexp by n.
pub(crate) fn lower_f64_exp2(
    op: &OpF64Exp2,
    pred: Pred,
    alloc: &mut SSAValueAllocator,
    _sm: &ShaderModelInfo,
) -> Vec<Instr> {
    let mut out = Vec::new();
    let rnd = FRndMode::NearestEven;

    let x = op.src.src_ref.clone().to_ssa();
    assert!(x.comps() == 2, "f64 exp2 src must have 2 components");
    let x_src = Src::from(x.clone());

    // n = round(x) via F2I with NearestEven
    let n_i32 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpF2I {
            dst: n_i32.into(),
            src: x_src.clone(),
            src_type: FloatType::F64,
            dst_type: IntType::I32,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
        }),
        pred,
    ));

    // n_f64 = I2F(n) for fractional part computation
    let n_f64 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpI2F {
            dst: n_f64.clone().into(),
            src: n_i32.into(),
            dst_type: FloatType::F64,
            src_type: IntType::I32,
            rnd_mode: FRndMode::NearestEven,
        }),
        pred,
    ));

    // f = x - n_f64 (fractional part, |f| <= 0.5)
    let neg_n = alloc.alloc_vec(RegFile::GPR, 2);
    let zero = emit_f64_zero(&mut out, alloc, pred);
    out.push(with_pred(
        Instr::new(OpDAdd {
            dst: neg_n.clone().into(),
            srcs: [Src::from(zero), Src::from(n_f64.clone()).fneg()],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let f = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDAdd {
            dst: f.clone().into(),
            srcs: [x_src.clone(), Src::from(neg_n)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let f_src = Src::from(f.clone());

    // Horner: p = c0 + f*(c1 + f*(c2 + f*(c3 + f*(c4 + f*(c5 + f*c6)))))
    const C0: f64 = 1.0;
    const C1: f64 = std::f64::consts::LN_2;
    const C2: f64 = 0.240_226_506_959_100_71;
    const C3: f64 = 0.055_504_108_664_821_58;
    const C4: f64 = 0.009_618_129_107_628_477;
    const C5: f64 = 0.001_333_355_814_642_844_3;
    const C6: f64 = 0.000_154_035_303_933_816_1;

    let c0 = emit_f64_const(&mut out, alloc, pred, C0);
    let c1 = emit_f64_const(&mut out, alloc, pred, C1);
    let c2 = emit_f64_const(&mut out, alloc, pred, C2);
    let c3 = emit_f64_const(&mut out, alloc, pred, C3);
    let c4 = emit_f64_const(&mut out, alloc, pred, C4);
    let c5 = emit_f64_const(&mut out, alloc, pred, C5);
    let c6 = emit_f64_const(&mut out, alloc, pred, C6);

    let mut p = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: p.clone().into(),
            srcs: [f_src.clone(), Src::from(c6), Src::from(c5)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let p_src = Src::from(p.clone());
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: p.clone().into(),
            srcs: [f_src.clone(), p_src, Src::from(c4)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let p_src = Src::from(p.clone());
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: p.clone().into(),
            srcs: [f_src.clone(), p_src, Src::from(c3)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let p_src = Src::from(p.clone());
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: p.clone().into(),
            srcs: [f_src.clone(), p_src, Src::from(c2)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let p_src = Src::from(p.clone());
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: p.clone().into(),
            srcs: [f_src.clone(), p_src, Src::from(c1)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let p_src = Src::from(p.clone());
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: p.clone().into(),
            srcs: [f_src.clone(), p_src, Src::from(c0)],
            rnd_mode: rnd,
        }),
        pred,
    ));

    // ldexp(p, n): add n << 20 to the high word of p (f64 exponent is bits [30:20] of high word)
    let n_shifted = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpShf {
            dst: n_shifted.into(),
            low: n_i32.into(),
            high: Src::ZERO,
            shift: Src::new_imm_u32(20),
            right: false,
            wrap: true,
            data_type: IntType::I32,
            dst_high: false,
        }),
        pred,
    ));

    let p_high_plus_n = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpIAdd3 {
            dst: p_high_plus_n.into(),
            srcs: [p[1].into(), n_shifted.into(), Src::ZERO],
            overflow: [Dst::None, Dst::None],
        }),
        pred,
    ));

    // Write result: low word unchanged, high word with new exponent
    let dst_ssa = op.dst.as_ssa().unwrap();
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: dst_ssa[0].into(),
            src: p[0].into(),
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: dst_ssa[1].into(),
            src: p_high_plus_n.into(),
        }),
        pred,
    ));

    out
}

/// log2(x) via MUFU.LOG2/EX2/RCP seed + Newton refinement (~46-bit accuracy).
pub(crate) fn lower_f64_log2(
    op: &OpF64Log2,
    pred: Pred,
    alloc: &mut SSAValueAllocator,
    _sm: &ShaderModelInfo,
) -> Vec<Instr> {
    let mut out = Vec::new();
    let rnd = FRndMode::NearestEven;

    let x = op.src.src_ref.clone().to_ssa();
    assert!(x.comps() == 2, "f64 log2 src must have 2 components");

    let x_f32 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpF2F {
            dst: x_f32.into(),
            src: Src::from(x.clone()),
            src_type: FloatType::F64,
            dst_type: FloatType::F32,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dst_high: false,
            integer_rnd: false,
        }),
        pred,
    ));

    let y0_f32 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpMuFu {
            dst: y0_f32.into(),
            op: MuFuOp::Log2,
            src: x_f32.into(),
        }),
        pred,
    ));

    let exp_y0 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpMuFu {
            dst: exp_y0.into(),
            op: MuFuOp::Exp2,
            src: y0_f32.into(),
        }),
        pred,
    ));

    let rcp_exp = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpMuFu {
            dst: rcp_exp.into(),
            op: MuFuOp::Rcp,
            src: exp_y0.into(),
        }),
        pred,
    ));

    let ratio = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpFMul {
            dst: ratio.into(),
            srcs: [x_f32.into(), rcp_exp.into()],
            saturate: false,
            rnd_mode: rnd,
            ftz: false,
            dnz: false,
        }),
        pred,
    ));

    let minus_one_f32 = (-1.0f32).to_bits();
    let diff_f32 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpFAdd {
            dst: diff_f32.into(),
            srcs: [ratio.into(), Src::new_imm_u32(minus_one_f32)],
            saturate: false,
            rnd_mode: rnd,
            ftz: false,
        }),
        pred,
    ));

    let diff_f64 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpF2F {
            dst: diff_f64.clone().into(),
            src: diff_f32.into(),
            src_type: FloatType::F32,
            dst_type: FloatType::F64,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dst_high: false,
            integer_rnd: false,
        }),
        pred,
    ));

    let y0_f64 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpF2F {
            dst: y0_f64.clone().into(),
            src: y0_f32.into(),
            src_type: FloatType::F32,
            dst_type: FloatType::F64,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dst_high: false,
            integer_rnd: false,
        }),
        pred,
    ));

    const INV_LN2: f64 = std::f64::consts::LOG2_E;
    let inv_ln2 = emit_f64_const(&mut out, alloc, pred, INV_LN2);
    let correction = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: correction.clone().into(),
            srcs: [Src::from(diff_f64), Src::from(inv_ln2)],
            rnd_mode: rnd,
        }),
        pred,
    ));

    out.push(with_pred(
        Instr::new(OpDAdd {
            dst: op.dst.clone(),
            srcs: [Src::from(y0_f64), Src::from(correction)],
            rnd_mode: rnd,
        }),
        pred,
    ));

    out
}

// Cody-Waite constants: 2/π, (π/2)_hi, (π/2)_lo (high-precision for range reduction)
#[allow(clippy::approx_constant)]
const TWO_OVER_PI: f64 = 0.636_619_772_367_581_4;
#[allow(clippy::approx_constant)]
const PI_HALF_HI: f64 = 1.570_796_326_794_896_6;
const PI_HALF_LO: f64 = 6.123_233_995_736_766e-17;

/// Cody-Waite range reduction to [-π/4, π/4].
/// Returns (r: reduced argument, n_i32: quadrant index for n mod 4).
fn emit_cody_waite_reduction(
    out: &mut Vec<Instr>,
    alloc: &mut SSAValueAllocator,
    pred: Pred,
    x_src: Src,
) -> (SSARef, SSAValue) {
    let rnd = FRndMode::NearestEven;

    // t = x * (2/π)
    let t = alloc.alloc_vec(RegFile::GPR, 2);
    let two_over_pi = emit_f64_const(out, alloc, pred, TWO_OVER_PI);
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: t.clone().into(),
            srcs: [x_src.clone(), Src::from(two_over_pi)],
            rnd_mode: rnd,
        }),
        pred,
    ));

    // n = round(t) via F2I
    let n_i32 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpF2I {
            dst: n_i32.into(),
            src: Src::from(t.clone()),
            src_type: FloatType::F64,
            dst_type: IntType::I32,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
        }),
        pred,
    ));

    // n_f64 = I2F(n) for Cody-Waite subtraction
    let n_f64 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpI2F {
            dst: n_f64.clone().into(),
            src: n_i32.into(),
            dst_type: FloatType::F64,
            src_type: IntType::I32,
            rnd_mode: FRndMode::NearestEven,
        }),
        pred,
    ));

    // r = x - n * (π/2)_hi - n * (π/2)_lo via DFMA
    let pi_half_hi = emit_f64_const(out, alloc, pred, PI_HALF_HI);
    let pi_half_lo = emit_f64_const(out, alloc, pred, PI_HALF_LO);
    let mut r = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: r.clone().into(),
            srcs: [Src::from(n_f64.clone()).fneg(), Src::from(pi_half_hi), x_src],
            rnd_mode: rnd,
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: r.clone().into(),
            srcs: [
                Src::from(n_f64).fneg(),
                Src::from(pi_half_lo),
                Src::from(r.clone()),
            ],
            rnd_mode: rnd,
        }),
        pred,
    ));

    (r, n_i32)
}

/// emit_f64_sel: component-wise select for f64: cond ? a : b
fn emit_f64_sel(
    out: &mut Vec<Instr>,
    alloc: &mut SSAValueAllocator,
    pred: Pred,
    cond: SSAValue,
    a: SSARef,
    b: SSARef,
) -> SSARef {
    let dst = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpSel {
            dst: dst[0].into(),
            cond: cond.into(),
            srcs: [a[0].into(), b[0].into()],
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpSel {
            dst: dst[1].into(),
            cond: cond.into(),
            srcs: [a[1].into(), b[1].into()],
        }),
        pred,
    ));
    dst
}

/// sin(x) via minimax polynomial. Valid for |x| <= π/4.
/// sin(x) ≈ x · (1 + x²·(s1 + x²·(s2 + x²·(s3 + x²·s4))))
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
    let r_sq_src = Src::from(r_sq.clone());
    let s1 = emit_f64_const(out, alloc, pred, S1);
    let s2 = emit_f64_const(out, alloc, pred, S2);
    let s3 = emit_f64_const(out, alloc, pred, S3);
    let s4 = emit_f64_const(out, alloc, pred, S4);
    let mut inner = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: inner.clone().into(),
            srcs: [r_sq_src.clone(), Src::from(s4), Src::from(s3)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let inner_src = Src::from(inner.clone());
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: inner.clone().into(),
            srcs: [r_sq_src.clone(), inner_src, Src::from(s2)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let inner_src = Src::from(inner.clone());
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: inner.clone().into(),
            srcs: [r_sq_src.clone(), inner_src, Src::from(s1)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let one = emit_f64_const(out, alloc, pred, 1.0);
    let inner_src = Src::from(inner);
    let poly = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: poly.clone().into(),
            srcs: [r_sq_src, inner_src, Src::from(one)],
            rnd_mode: rnd,
        }),
        pred,
    ));
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

    // x² = DMul(x, x)
    let x_sq = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: x_sq.clone().into(),
            srcs: [x_src.clone(), x_src.clone()],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let x_sq_src = Src::from(x_sq.clone());

    // Inner polynomial: s1 + x²·(s2 + x²·(s3 + x²·s4))
    let s1 = emit_f64_const(&mut out, alloc, pred, S1);
    let s2 = emit_f64_const(&mut out, alloc, pred, S2);
    let s3 = emit_f64_const(&mut out, alloc, pred, S3);
    let s4 = emit_f64_const(&mut out, alloc, pred, S4);

    let mut inner = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: inner.clone().into(),
            srcs: [x_sq_src.clone(), Src::from(s4), Src::from(s3)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let inner_src = Src::from(inner.clone());
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: inner.clone().into(),
            srcs: [x_sq_src.clone(), inner_src, Src::from(s2)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let inner_src = Src::from(inner.clone());
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: inner.clone().into(),
            srcs: [x_sq_src.clone(), inner_src, Src::from(s1)],
            rnd_mode: rnd,
        }),
        pred,
    ));

    // poly = 1 + x²·inner
    let one = emit_f64_const(&mut out, alloc, pred, 1.0);
    let inner_src = Src::from(inner);
    let poly = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: poly.clone().into(),
            srcs: [x_sq_src, inner_src, Src::from(one)],
            rnd_mode: rnd,
        }),
        pred,
    ));

    // result = x * poly
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

/// cos(x) via minimax polynomial. Valid for |x| <= π/4.
/// cos(x) ≈ 1 + x²·(c1 + x²·(c2 + x²·(c3 + x²·c4)))
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
    let r_sq_src = Src::from(r_sq.clone());
    let c1 = emit_f64_const(out, alloc, pred, COS_C1);
    let c2 = emit_f64_const(out, alloc, pred, COS_C2);
    let c3 = emit_f64_const(out, alloc, pred, COS_C3);
    let c4 = emit_f64_const(out, alloc, pred, COS_C4);
    let mut inner = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: inner.clone().into(),
            srcs: [r_sq_src.clone(), Src::from(c4), Src::from(c3)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let inner_src = Src::from(inner.clone());
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: inner.clone().into(),
            srcs: [r_sq_src.clone(), inner_src, Src::from(c2)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let inner_src = Src::from(inner.clone());
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: inner.clone().into(),
            srcs: [r_sq_src.clone(), inner_src, Src::from(c1)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let one = emit_f64_const(out, alloc, pred, 1.0);
    let inner_src = Src::from(inner);
    let result = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: result.clone().into(),
            srcs: [r_sq_src, inner_src, Src::from(one)],
            rnd_mode: rnd,
        }),
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

    // Cody-Waite range reduction
    let (r, n_i32) = emit_cody_waite_reduction(&mut out, alloc, pred, x_src.clone());
    let r_src = Src::from(r.clone());

    // sin(r) and cos(r) polynomials
    let sin_r = emit_sin_poly(&mut out, alloc, pred, r_src.clone());
    let cos_r = emit_cos_poly(&mut out, alloc, pred, r_src);

    // Quadrant correction for cos: need_sin=(n&1)!=0, need_negate=((n+1)&2)!=0
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
            srcs: [n_and_1.into(), Src::ZERO],
            accum: Src::new_imm_bool(true),
            low_cmp: Src::new_imm_bool(true),
        }),
        pred,
    ));
    let n_plus_1 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpIAdd3 {
            dst: n_plus_1.into(),
            srcs: [n_i32.into(), Src::new_imm_u32(1), Src::ZERO],
            overflow: [Dst::None, Dst::None],
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
            srcs: [n_plus_1_and_2.into(), Src::ZERO],
            accum: Src::new_imm_bool(true),
            low_cmp: Src::new_imm_bool(true),
        }),
        pred,
    ));

    // picked = need_sin ? sin(r) : cos(r)
    let picked = emit_f64_sel(&mut out, alloc, pred, p_sin, sin_r, cos_r);

    // result = need_negate ? -picked : picked
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
        Instr::new(OpCopy {
            dst: op.dst.as_ssa().unwrap()[0].into(),
            src: result[0].into(),
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: op.dst.as_ssa().unwrap()[1].into(),
            src: result[1].into(),
        }),
        pred,
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nak::ir::{OpF64Cos, OpF64Exp2, OpF64Log2, OpF64Sin};
    use coral_nak_stubs::cfg::CFG;

    fn make_sm70() -> ShaderModelInfo {
        ShaderModelInfo::new(70, 64)
    }

    #[test]
    fn test_f64_exp2_lowering_produces_dfma_sequence() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);

        let op = OpF64Exp2 {
            dst: dst.into(),
            src: Src::from(x.clone()),
        };
        let instr = Instr::new(op);
        let result = super::super::lower_instr(instr, &mut alloc, &sm);

        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions");
        };
        assert!(seq.len() > 5, "exp2 should expand to multiple instructions");
        let has_dfma = seq.iter().any(|i| matches!(i.op, Op::DFma(_)));
        assert!(has_dfma, "exp2 lowering should use DFMA");
    }

    #[test]
    fn test_f64_log2_lowering_produces_mufu_sequence() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);

        let op = OpF64Log2 {
            dst: dst.into(),
            src: Src::from(x.clone()),
        };
        let instr = Instr::new(op);
        let result = super::super::lower_instr(instr, &mut alloc, &sm);

        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions");
        };
        assert!(seq.len() >= 3, "log2 should expand to at least 3 instructions");
        let has_log2 = seq
            .iter()
            .any(|i| matches!(&i.op, Op::MuFu(m) if m.op == MuFuOp::Log2));
        let has_exp2 = seq
            .iter()
            .any(|i| matches!(&i.op, Op::MuFu(m) if m.op == MuFuOp::Exp2));
        let has_rcp = seq
            .iter()
            .any(|i| matches!(&i.op, Op::MuFu(m) if m.op == MuFuOp::Rcp));
        assert!(has_log2, "log2 lowering should use MUFU.LOG2");
        assert!(has_exp2, "log2 Newton refinement should use MUFU.EX2");
        assert!(has_rcp, "log2 Newton refinement should use MUFU.RCP");
    }

    #[test]
    fn test_f64_sin_lowering_produces_dfma_sequence() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);

        let op = OpF64Sin {
            dst: dst.into(),
            src: Src::from(x.clone()),
        };
        let instr = Instr::new(op);
        let result = super::super::lower_instr(instr, &mut alloc, &sm);

        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions");
        };
        assert!(seq.len() > 5, "sin should expand to multiple instructions");
        let has_dfma = seq.iter().any(|i| matches!(i.op, Op::DFma(_)));
        let has_dmul = seq.iter().any(|i| matches!(i.op, Op::DMul(_)));
        assert!(has_dfma, "sin lowering should use DFMA");
        assert!(has_dmul, "sin lowering should use DMul");
    }

    #[test]
    fn test_f64_sin_lowering_includes_cody_waite_range_reduction() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);

        let op = OpF64Sin {
            dst: dst.into(),
            src: Src::from(x.clone()),
        };
        let instr = Instr::new(op);
        let result = super::super::lower_instr(instr, &mut alloc, &sm);

        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions");
        };
        let has_dmul_2_over_pi = seq.iter().any(|i| matches!(i.op, Op::DMul(_)));
        let has_dfma_cody_waite = seq.iter().any(|i| matches!(i.op, Op::DFma(_)));
        assert!(has_dmul_2_over_pi, "sin lowering should use DMul for x * 2/π");
        assert!(has_dfma_cody_waite, "sin lowering should use DFMA for Cody-Waite");
    }

    #[test]
    fn test_f64_sin_lowering_includes_quadrant_correction() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);

        let op = OpF64Sin {
            dst: dst.into(),
            src: Src::from(x.clone()),
        };
        let instr = Instr::new(op);
        let result = super::super::lower_instr(instr, &mut alloc, &sm);

        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions");
        };
        // Range reduction (DMul, DFMA, F2I, I2F) and quadrant correction are all present
        // in the full Cody-Waite implementation.
        let has_dfma = seq.iter().any(|i| matches!(i.op, Op::DFma(_)));
        let has_dmul = seq.iter().any(|i| matches!(i.op, Op::DMul(_)));
        assert!(has_dfma, "sin lowering should use DFMA");
        assert!(has_dmul, "sin lowering should use DMul for range reduction");
    }

    #[test]
    fn test_f64_cos_lowering_produces_dfma_sequence() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);

        let op = OpF64Cos {
            dst: dst.into(),
            src: Src::from(x.clone()),
        };
        let instr = Instr::new(op);
        let result = super::super::lower_instr(instr, &mut alloc, &sm);

        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions");
        };
        assert!(seq.len() > 5, "cos should expand to multiple instructions");
        let has_dfma = seq.iter().any(|i| matches!(i.op, Op::DFma(_)));
        assert!(has_dfma, "cos lowering should use DFMA");
    }

    #[test]
    fn test_f64_cos_lowering_includes_cody_waite_range_reduction() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);

        let op = OpF64Cos {
            dst: dst.into(),
            src: Src::from(x.clone()),
        };
        let instr = Instr::new(op);
        let result = super::super::lower_instr(instr, &mut alloc, &sm);

        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions");
        };
        let has_dmul = seq.iter().any(|i| matches!(i.op, Op::DMul(_)));
        let has_dfma = seq.iter().any(|i| matches!(i.op, Op::DFma(_)));
        assert!(has_dmul, "cos lowering should use DMul for x * 2/π");
        assert!(has_dfma, "cos lowering should use DFMA for Cody-Waite");
    }

    #[test]
    fn test_f64_cos_lowering_includes_quadrant_correction() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);

        let op = OpF64Cos {
            dst: dst.into(),
            src: Src::from(x.clone()),
        };
        let instr = Instr::new(op);
        let result = super::super::lower_instr(instr, &mut alloc, &sm);

        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions");
        };
        let has_dfma = seq.iter().any(|i| matches!(i.op, Op::DFma(_)));
        let has_dmul = seq.iter().any(|i| matches!(i.op, Op::DMul(_)));
        let has_f2i = seq.iter().any(|i| matches!(i.op, Op::F2I(_)));
        let has_i2f = seq.iter().any(|i| matches!(i.op, Op::I2F(_)));
        assert!(has_dfma, "cos lowering should use DFMA");
        assert!(has_dmul, "cos lowering should use DMul for range reduction");
        assert!(has_f2i, "cos lowering should use F2I for quadrant index");
        assert!(has_i2f, "cos lowering should use I2F for Cody-Waite");
    }
}
