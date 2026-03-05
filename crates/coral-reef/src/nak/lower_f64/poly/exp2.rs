// Copyright © 2023 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! f64 exp2(x) via Horner polynomial with range reduction + ldexp.

#![allow(clippy::wildcard_imports)]

use super::super::*;

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
        Instr::new(OpDFma { dst: p.clone().into(), srcs: [f_src.clone(), Src::from(c6), Src::from(c5)], rnd_mode: rnd }),
        pred,
    ));
    let p_src = Src::from(p.clone());
    out.push(with_pred(
        Instr::new(OpDFma { dst: p.clone().into(), srcs: [f_src.clone(), p_src, Src::from(c4)], rnd_mode: rnd }),
        pred,
    ));
    let p_src = Src::from(p.clone());
    out.push(with_pred(
        Instr::new(OpDFma { dst: p.clone().into(), srcs: [f_src.clone(), p_src, Src::from(c3)], rnd_mode: rnd }),
        pred,
    ));
    let p_src = Src::from(p.clone());
    out.push(with_pred(
        Instr::new(OpDFma { dst: p.clone().into(), srcs: [f_src.clone(), p_src, Src::from(c2)], rnd_mode: rnd }),
        pred,
    ));
    let p_src = Src::from(p.clone());
    out.push(with_pred(
        Instr::new(OpDFma { dst: p.clone().into(), srcs: [f_src.clone(), p_src, Src::from(c1)], rnd_mode: rnd }),
        pred,
    ));
    let p_src = Src::from(p.clone());
    out.push(with_pred(
        Instr::new(OpDFma { dst: p.clone().into(), srcs: [f_src.clone(), p_src, Src::from(c0)], rnd_mode: rnd }),
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
        Instr::new(OpCopy { dst: dst_ssa[0].into(), src: p[0].into() }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpCopy { dst: dst_ssa[1].into(), src: p_high_plus_n.into() }),
        pred,
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::super::super::*;
    use crate::nak::ir::OpF64Exp2;

    fn make_sm70() -> ShaderModelInfo {
        ShaderModelInfo::new(70, 64)
    }

    #[test]
    fn test_f64_exp2_lowering_produces_dfma_sequence() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);

        let op = OpF64Exp2 { dst: dst.into(), src: Src::from(x.clone()) };
        let instr = Instr::new(op);
        let result = super::super::super::lower_instr(instr, &mut alloc, &sm);

        let MappedInstrs::Many(seq) = result else { panic!("Expected Many instructions") };
        assert!(seq.len() > 5, "exp2 should expand to multiple instructions");
        let has_dfma = seq.iter().any(|i| matches!(i.op, Op::DFma(_)));
        assert!(has_dfma, "exp2 lowering should use DFMA");
    }
}
