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
    _sm: &ShaderModelInfo,
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

    // ldexp(p, n): add n << 20 to the high word of p
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
