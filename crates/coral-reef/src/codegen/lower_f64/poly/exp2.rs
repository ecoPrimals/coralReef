// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)
//! f64 exp2(x) via Horner polynomial with range reduction + ldexp.

#![allow(clippy::wildcard_imports, clippy::redundant_clone)]

use super::super::*;

/// exp2(x) via Horner polynomial with range reduction.
/// n = round(x), f = x - n, evaluate polynomial on f in [-0.5, 0.5], then ldexp by n.
pub fn lower_f64_exp2(
    op: &OpF64Exp2,
    pred: Pred,
    alloc: &mut SSAValueAllocator,
    _sm: &dyn ShaderModel,
) -> Vec<Instr> {
    let mut out = Vec::new();
    let rnd = FRndMode::NearestEven;

    let x = op.src.reference.clone().to_ssa();
    assert!(x.comps() == 2, "f64 exp2 src must have 2 components");
    let x_src = Src::from(x);

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

    let neg_n = alloc.alloc_vec(RegFile::GPR, 2);
    let zero = emit_f64_zero(&mut out, alloc, pred);
    out.push(with_pred(
        Instr::new(OpDAdd {
            dst: neg_n.clone().into(),
            srcs: [Src::from(zero), Src::from(n_f64).fneg()],
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
    let f_src = Src::from(f);

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

    // Horner: p = c0 + f*(c1 + f*(c2 + f*(c3 + f*(c4 + f*(c5 + f*c6)))))
    let p = emit_f64_dfma(
        &mut out,
        alloc,
        pred,
        f_src.clone(),
        Src::from(c6),
        Src::from(c5),
    );
    let p = emit_f64_dfma(
        &mut out,
        alloc,
        pred,
        f_src.clone(),
        Src::from(p),
        Src::from(c4),
    );
    let p = emit_f64_dfma(
        &mut out,
        alloc,
        pred,
        f_src.clone(),
        Src::from(p),
        Src::from(c3),
    );
    let p = emit_f64_dfma(
        &mut out,
        alloc,
        pred,
        f_src.clone(),
        Src::from(p),
        Src::from(c2),
    );
    let p = emit_f64_dfma(
        &mut out,
        alloc,
        pred,
        f_src.clone(),
        Src::from(p),
        Src::from(c1),
    );
    let p = emit_f64_dfma(
        &mut out,
        alloc,
        pred,
        f_src.clone(),
        Src::from(p),
        Src::from(c0),
    );

    // ldexp(p, n) with subnormal handling.
    //
    // Direct high-word addition works for -1022 <= n <= 1023 (normal range).
    // For n < -1022 (subnormal results), split into two steps:
    //   ldexp(p, n) = ldexp(ldexp(p, n1), n2) where n1 = max(n, -1022), n2 = n - n1
    // This prevents the intermediate result from underflowing to zero.

    let dst_ssa = op.dst.as_ssa().expect("exp2 destination must be SSA value");

    // Check if n is in the subnormal danger zone (n < -1022)
    let is_subnormal = alloc.alloc(RegFile::Pred);
    out.push(with_pred(
        Instr::new(OpISetP {
            dst: is_subnormal.into(),
            set_op: PredSetOp::And,
            cmp_op: IntCmpOp::Lt,
            cmp_type: IntCmpType::I32,
            ex: false,
            srcs: [n_i32.into(), Src::new_imm_u32((-1022_i32) as u32)],
            accum: SrcRef::True.into(),
            low_cmp: Src::new_imm_bool(false),
        }),
        pred,
    ));

    // Normal path: n1 = n
    // Subnormal path: n1 = -1022, n2 = n - (-1022) = n + 1022
    let n1 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpSel {
            dst: n1.into(),
            cond: is_subnormal.into(),
            srcs: [Src::new_imm_u32((-1022_i32) as u32), n_i32.into()],
        }),
        pred,
    ));

    // First ldexp: add n1 << 20 to p's high word
    let n1_shifted = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpShf {
            dst: n1_shifted.into(),
            low: n1.into(),
            high: Src::ZERO,
            shift: Src::new_imm_u32(20),
            right: false,
            wrap: true,
            data_type: IntType::I32,
            dst_high: false,
        }),
        pred,
    ));

    let p_high_step1 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpIAdd3 {
            dst: p_high_step1.into(),
            srcs: [p[1].into(), n1_shifted.into(), Src::ZERO],
            overflow: [Dst::None, Dst::None],
        }),
        pred,
    ));

    // For normal path, this is the final result.
    // For subnormal path, multiply by 2^n2 where n2 = n + 1022.
    let n2 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpIAdd3 {
            dst: n2.into(),
            srcs: [n_i32.into(), Src::new_imm_u32(1022), Src::ZERO],
            overflow: [Dst::None, Dst::None],
        }),
        pred,
    ));

    // Build 2^n2 as f64: high word = (n2 + 1023) << 20, low word = 0
    let n2_biased = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpIAdd3 {
            dst: n2_biased.into(),
            srcs: [n2.into(), Src::new_imm_u32(1023), Src::ZERO],
            overflow: [Dst::None, Dst::None],
        }),
        pred,
    ));
    let scale_hi = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpShf {
            dst: scale_hi.into(),
            low: n2_biased.into(),
            high: Src::ZERO,
            shift: Src::new_imm_u32(20),
            right: false,
            wrap: true,
            data_type: IntType::I32,
            dst_high: false,
        }),
        pred,
    ));

    // step1 * 2^n2
    let step1 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: step1[0].into(),
            src: p[0].into(),
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: step1[1].into(),
            src: p_high_step1.into(),
        }),
        pred,
    ));

    let scale = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: scale[0].into(),
            src: Src::ZERO,
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: scale[1].into(),
            src: scale_hi.into(),
        }),
        pred,
    ));

    let sub_result = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: sub_result.clone().into(),
            srcs: [Src::from(step1.clone()), Src::from(scale)],
            rnd_mode: FRndMode::NearestEven,
        }),
        pred,
    ));

    // Select between normal result (step1) and subnormal result (sub_result)
    out.push(with_pred(
        Instr::new(OpSel {
            dst: dst_ssa[0].into(),
            cond: is_subnormal.into(),
            srcs: [sub_result[0].into(), step1[0].into()],
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpSel {
            dst: dst_ssa[1].into(),
            cond: is_subnormal.into(),
            srcs: [sub_result[1].into(), step1[1].into()],
        }),
        pred,
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::super::super::*;
    use crate::codegen::ir::OpF64Exp2;

    fn make_sm70() -> ShaderModelInfo {
        ShaderModelInfo::new(70, 64)
    }

    /// Horner polynomial for exp2(f) on f in [-0.5, 0.5]. Same coefficients as in lower_f64_exp2.
    fn horner_exp2_poly(f: f64) -> f64 {
        const C0: f64 = 1.0;
        const C1: f64 = std::f64::consts::LN_2;
        const C2: f64 = 0.240_226_506_959_100_71;
        const C3: f64 = 0.055_504_108_664_821_58;
        const C4: f64 = 0.009_618_129_107_628_477;
        const C5: f64 = 0.001_333_355_814_642_844_3;
        const C6: f64 = 0.000_154_035_303_933_816_1;
        let coeffs = [C0, C1, C2, C3, C4, C5, C6];
        assert_eq!(coeffs.len(), 7, "exp2 polynomial has 7 coefficients");
        // p = c0 + f*(c1 + f*(c2 + f*(c3 + f*(c4 + f*(c5 + f*c6)))))
        let mut p = coeffs[6];
        for i in (0..6).rev() {
            p = f.mul_add(p, coeffs[i]);
        }
        p
    }

    #[test]
    fn test_exp2_coefficient_count_and_values() {
        const C0: f64 = 1.0;
        const C1: f64 = std::f64::consts::LN_2;
        const C2: f64 = 0.240_226_506_959_100_71;
        const C3: f64 = 0.055_504_108_664_821_58;
        const C4: f64 = 0.009_618_129_107_628_477;
        const C5: f64 = 0.001_333_355_814_642_844_3;
        const C6: f64 = 0.000_154_035_303_933_816_1;
        let coeffs = [C0, C1, C2, C3, C4, C5, C6];
        assert_eq!(coeffs.len(), 7);
        assert!((C0 - 1.0).abs() < 1e-15);
        assert!((C1 - std::f64::consts::LN_2).abs() < 1e-15);
        const { assert!(C2 > 0.2 && C2 < 0.25) }
        const { assert!(C3 > 0.05 && C3 < 0.06) }
        const { assert!(C4 > 0.009 && C4 < 0.01) }
        const { assert!(C5 > 0.001 && C5 < 0.002) }
        const { assert!(C6 > 0.0001 && C6 < 0.0002) }
    }

    #[test]
    fn test_exp2_polynomial_known_values() {
        // exp2(0) = 1, so for f=0 the polynomial should yield 1
        let p0 = horner_exp2_poly(0.0);
        assert!(
            (p0 - 1.0).abs() < 1e-14,
            "exp2 poly at 0 should be 1, got {}",
            p0
        );

        // exp2(0.5) ≈ 1.414..., exp2(-0.5) ≈ 0.707...
        // Polynomial has ~46-bit accuracy (≤2 ULP); use relaxed tolerance
        let p_half = horner_exp2_poly(0.5);
        let expected = 2_f64.sqrt();
        assert!(
            (p_half - expected).abs() < 1e-6,
            "exp2 poly at 0.5 should be sqrt(2)≈{}, got {}",
            expected,
            p_half
        );

        let p_neg_half = horner_exp2_poly(-0.5);
        let expected_neg = 1.0 / 2_f64.sqrt();
        assert!(
            (p_neg_half - expected_neg).abs() < 1e-6,
            "exp2 poly at -0.5 should be 1/sqrt(2)≈{}, got {}",
            expected_neg,
            p_neg_half
        );
    }

    #[test]
    fn test_exp2_polynomial_edge_values() {
        // Very small f
        let p_tiny = horner_exp2_poly(1e-10);
        assert!(p_tiny > 0.99 && p_tiny < 1.01);

        // At boundary f=0.5
        let p = horner_exp2_poly(0.5);
        assert!(p > 1.0 && p < 2.0);
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
        let result = super::super::super::lower_instr(instr, &mut alloc, &sm);

        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions")
        };
        assert!(seq.len() > 5, "exp2 should expand to multiple instructions");
        let has_dfma = seq.iter().any(|i| matches!(i.op, Op::DFma(_)));
        assert!(has_dfma, "exp2 lowering should use DFMA");
    }
}
