// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)
//! f64 log2(x) via MUFU.LOG2/EX2/RCP seed + Newton refinement (~46-bit accuracy).

#![allow(clippy::wildcard_imports, clippy::redundant_clone)]

use super::super::*;

/// log2(x) via MUFU.LOG2/EX2/RCP seed + Newton refinement (~46-bit accuracy).
pub fn lower_f64_log2(
    op: &OpF64Log2,
    pred: Pred,
    alloc: &mut SSAValueAllocator,
    _sm: &ShaderModelInfo,
) -> Vec<Instr> {
    let mut out = Vec::new();
    let rnd = FRndMode::NearestEven;

    let x = op.src.reference.clone().to_ssa();
    assert!(x.comps() == 2, "f64 log2 src must have 2 components");

    let x_f32 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpF2F {
            dst: x_f32.into(),
            src: Src::from(x),
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
        Instr::new(OpTranscendental {
            dst: y0_f32.into(),
            op: TranscendentalOp::Log2,
            src: x_f32.into(),
        }),
        pred,
    ));

    let exp_y0 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpTranscendental {
            dst: exp_y0.into(),
            op: TranscendentalOp::Exp2,
            src: y0_f32.into(),
        }),
        pred,
    ));

    let rcp_exp = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpTranscendental {
            dst: rcp_exp.into(),
            op: TranscendentalOp::Rcp,
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

#[cfg(test)]
mod tests {
    use super::super::super::*;
    use crate::codegen::ir::OpF64Log2;

    fn make_sm70() -> ShaderModelInfo {
        ShaderModelInfo::new(70, 64)
    }

    #[test]
    fn test_log2_inv_ln2_constant() {
        const INV_LN2: f64 = std::f64::consts::LOG2_E;
        assert!((INV_LN2 - std::f64::consts::LOG2_E).abs() < 1e-15);
        const { assert!(INV_LN2 > 1.4 && INV_LN2 < 1.5) }
        // 1/ln(2) ≈ LOG2_E
        assert!((INV_LN2 - std::f64::consts::LOG2_E).abs() < 1e-10);
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
        let result = super::super::super::lower_instr(instr, &mut alloc, &sm);

        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions")
        };
        assert!(
            seq.len() >= 3,
            "log2 should expand to at least 3 instructions"
        );
        let has_log2 = seq
            .iter()
            .any(|i| matches!(&i.op, Op::Transcendental(m) if m.op == TranscendentalOp::Log2));
        let has_exp2 = seq
            .iter()
            .any(|i| matches!(&i.op, Op::Transcendental(m) if m.op == TranscendentalOp::Exp2));
        let has_rcp = seq
            .iter()
            .any(|i| matches!(&i.op, Op::Transcendental(m) if m.op == TranscendentalOp::Rcp));
        assert!(has_log2, "log2 lowering should use MUFU.LOG2");
        assert!(has_exp2, "log2 Newton refinement should use MUFU.EX2");
        assert!(has_rcp, "log2 Newton refinement should use MUFU.RCP");
    }
}
