// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)
//! Newton-Raphson lowering for f64 sqrt and rcp.
//!
//! Provenance: MUFU.RSQ64H/RCP64H seeds from NVIDIA ISA; 2-iteration refinement
//! targets ≤1 ULP (per hotSpring DF64 requirements and groundSpring `tol::ANALYTICAL`).

#![allow(clippy::wildcard_imports, clippy::redundant_clone)]

use super::*;

const F64_NEG_HALF: u32 = 0xBFE0_0000; // -0.5 as f32 bits (high word of f64)
const F64_ONE_HALF: u32 = 0x3FF8_0000; // 1.5

/// sqrt(x) = x * (1/sqrt(x)) via Newton-Raphson on 1/sqrt(x):
/// y₀ = MUFU.RSQ64H(x_hi), y₁ = y₀·(3 - x·y₀²)/2, y₂ = y₁·(3 - x·y₁²)/2, result = x·y₂
pub fn lower_f64_sqrt(
    op: &OpF64Sqrt,
    pred: Pred,
    alloc: &mut SSAValueAllocator,
    _sm: &ShaderModelInfo,
) -> Vec<Instr> {
    let mut out = Vec::new();
    let rnd = FRndMode::NearestEven;

    let x = op.src.reference.clone().to_ssa();
    assert!(x.comps() == 2, "f64 sqrt src must have 2 components");
    let x_src = Src::from(x.clone());
    let x_hi = Src::from(x[1]);

    // y₀ = MUFU.RSQ64H(x_hi) — seed is f32
    let y0_f32 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpTranscendental {
            dst: y0_f32.into(),
            op: TranscendentalOp::Rsq64H,
            src: x_hi,
        }),
        pred,
    ));

    // y₀ as f64: low=0, high=y0_f32
    let y0 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: y0[0].into(),
            src: Src::ZERO,
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: y0[1].into(),
            src: y0_f32.into(),
        }),
        pred,
    ));
    let y0_src = Src::from(y0);

    // y₁ = y₀ · (3 - x·y₀²) / 2 = y₀ · (1.5 - 0.5·x·y₀²)
    // t = x * y₀²
    let y0_sq = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: y0_sq.clone().into(),
            srcs: [y0_src.clone(), y0_src.clone()],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let t = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: t.clone().into(),
            srcs: [x_src.clone(), Src::from(y0_sq)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let t_src = Src::from(t);

    // -0.5 as f64
    let neg_half = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: neg_half[0].into(),
            src: Src::ZERO,
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: neg_half[1].into(),
            src: Src::new_imm_u32(F64_NEG_HALF),
        }),
        pred,
    ));
    let neg_half_src = Src::from(neg_half);

    // 1.5 as f64
    let one_half = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: one_half[0].into(),
            src: Src::ZERO,
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: one_half[1].into(),
            src: Src::new_imm_u32(F64_ONE_HALF),
        }),
        pred,
    ));
    let one_half_src = Src::from(one_half.clone());

    // factor₁ = 1.5 - 0.5·t = DFMA(-0.5, t, 1.5)
    let factor1 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: factor1.clone().into(),
            srcs: [neg_half_src, t_src, one_half_src],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let factor1_src = Src::from(factor1);

    // y₁ = y₀ · factor₁
    let y1 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: y1.clone().into(),
            srcs: [y0_src, factor1_src],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let y1_src = Src::from(y1.clone());

    // y₂: t₂ = x·y₁², factor₂ = DFMA(-0.5, t₂, 1.5), y₂ = y₁·factor₂
    let y1_sq = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: y1_sq.clone().into(),
            srcs: [y1_src.clone(), y1_src.clone()],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let t2 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: t2.clone().into(),
            srcs: [x_src.clone(), Src::from(y1_sq.clone())],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let t2_src = Src::from(t2);

    let neg_half2 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: neg_half2[0].into(),
            src: Src::ZERO,
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: neg_half2[1].into(),
            src: Src::new_imm_u32(F64_NEG_HALF),
        }),
        pred,
    ));
    let one_half2 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: one_half2[0].into(),
            src: Src::ZERO,
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: one_half2[1].into(),
            src: Src::new_imm_u32(F64_ONE_HALF),
        }),
        pred,
    ));

    let factor2 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: factor2.clone().into(),
            srcs: [Src::from(neg_half2), t2_src, Src::from(one_half2)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let factor2_src = Src::from(factor2);

    let y2 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: y2.clone().into(),
            srcs: [y1_src, factor2_src],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let y2_src = Src::from(y2);

    // result = x · y₂
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: op.dst.clone(),
            srcs: [x_src, y2_src],
            rnd_mode: rnd,
        }),
        pred,
    ));

    out
}

/// rcp(x) via Newton-Raphson: y₀ = MUFU.RCP64H(x_hi), y₁ = y₀·(2 - x·y₀), y₂ = y₁·(2 - x·y₁)
pub fn lower_f64_rcp(
    op: &OpF64Rcp,
    pred: Pred,
    alloc: &mut SSAValueAllocator,
    _sm: &ShaderModelInfo,
) -> Vec<Instr> {
    let mut out = Vec::new();
    let rnd = FRndMode::NearestEven;

    let x = op.src.reference.clone().to_ssa();
    assert!(x.comps() == 2, "f64 rcp src must have 2 components");
    let x_src = Src::from(x.clone());
    let x_hi = Src::from(x[1]);

    // y₀ = MUFU.RCP64H(x_hi)
    let y0_f32 = alloc.alloc(RegFile::GPR);
    out.push(with_pred(
        Instr::new(OpTranscendental {
            dst: y0_f32.into(),
            op: TranscendentalOp::Rcp64H,
            src: x_hi,
        }),
        pred,
    ));

    // y₀ as f64
    let y0 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: y0[0].into(),
            src: Src::ZERO,
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: y0[1].into(),
            src: y0_f32.into(),
        }),
        pred,
    ));
    let y0_src = Src::from(y0);

    // 2.0 as f64
    const F64_TWO: u32 = 0x4000_0000;
    let two = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: two[0].into(),
            src: Src::ZERO,
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: two[1].into(),
            src: Src::new_imm_u32(F64_TWO),
        }),
        pred,
    ));
    let two_src = Src::from(two);

    // factor = 2 - x·y₀, y₁ = y₀ · factor
    let xy0 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: xy0.clone().into(),
            srcs: [x_src.clone(), y0_src.clone()],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let xy0_src = Src::from(xy0);

    // factor = 2 - xy0 = 2 + (-xy0)
    let zero = emit_f64_zero(&mut out, alloc, pred);
    let neg_xy0 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDAdd {
            dst: neg_xy0.clone().into(),
            srcs: [Src::from(zero), xy0_src.fneg()],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let factor1 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDAdd {
            dst: factor1.clone().into(),
            srcs: [two_src.clone(), Src::from(neg_xy0)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let factor1_src = Src::from(factor1);

    let y1 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: y1.clone().into(),
            srcs: [y0_src, factor1_src],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let y1_src = Src::from(y1);

    // y₂: xy1 = x·y₁, factor₂ = 2 - xy1, y₂ = y₁·factor₂
    let xy1 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: xy1.clone().into(),
            srcs: [x_src.clone(), y1_src.clone()],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let xy1_src = Src::from(xy1);

    let zero2 = emit_f64_zero(&mut out, alloc, pred);
    let neg_xy1 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDAdd {
            dst: neg_xy1.clone().into(),
            srcs: [Src::from(zero2), xy1_src.fneg()],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let factor2 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDAdd {
            dst: factor2.clone().into(),
            srcs: [two_src, Src::from(neg_xy1)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let factor2_src = Src::from(factor2);

    out.push(with_pred(
        Instr::new(OpDMul {
            dst: op.dst.clone(),
            srcs: [y1_src, factor2_src],
            rnd_mode: rnd,
        }),
        pred,
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{OpF64Rcp, OpF64Sqrt, Pred};
    use coral_reef_stubs::cfg::CFG;

    fn make_sm70() -> ShaderModelInfo {
        ShaderModelInfo::new(70, 64)
    }

    #[test]
    fn test_newton_f64_constants() {
        // These are high words of f64; low word is 0. Verify as full f64 values.
        // F64_NEG_HALF = -0.5 (high word of f64)
        const F64_NEG_HALF: u32 = 0xBFE0_0000;
        let f64_neg_half = f64::from_bits(u64::from(F64_NEG_HALF) << 32);
        assert!((f64_neg_half - (-0.5)).abs() < 1e-10);

        // F64_ONE_HALF = 1.5
        const F64_ONE_HALF: u32 = 0x3FF8_0000;
        let f64_one_half = f64::from_bits(u64::from(F64_ONE_HALF) << 32);
        assert!((f64_one_half - 1.5).abs() < 1e-10);

        // F64_TWO = 2.0
        const F64_TWO: u32 = 0x4000_0000;
        let f64_two = f64::from_bits(u64::from(F64_TWO) << 32);
        assert!((f64_two - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_f64_sqrt_lowering_uses_rsq64h() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);

        let op = OpF64Sqrt {
            dst: dst.into(),
            src: Src::from(x.clone()),
        };
        let instr = Instr::new(op);
        let result = super::super::lower_instr(instr, &mut alloc, &sm);

        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions");
        };
        let has_rsq64h = seq
            .iter()
            .any(|i| matches!(&i.op, Op::Transcendental(m) if m.op == TranscendentalOp::Rsq64H));
        assert!(has_rsq64h, "sqrt lowering must use MUFU.Rsq64H seed");
    }

    #[test]
    fn test_f64_rcp_lowering_uses_rcp64h() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);

        let op = OpF64Rcp {
            dst: dst.into(),
            src: Src::from(x.clone()),
        };
        let instr = Instr::new(op);
        let result = super::super::lower_instr(instr, &mut alloc, &sm);

        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions");
        };
        let has_rcp64h = seq
            .iter()
            .any(|i| matches!(&i.op, Op::Transcendental(m) if m.op == TranscendentalOp::Rcp64H));
        assert!(has_rcp64h, "rcp lowering must use MUFU.Rcp64H seed");
    }

    #[test]
    fn test_f64_sqrt_lowering_two_newton_iterations() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);

        let op = OpF64Sqrt {
            dst: dst.into(),
            src: Src::from(x.clone()),
        };
        let instr = Instr::new(op);
        let result = super::super::lower_instr(instr, &mut alloc, &sm);

        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions");
        };
        let dfma_count = seq.iter().filter(|i| matches!(i.op, Op::DFma(_))).count();
        assert!(
            dfma_count >= 2,
            "sqrt uses 2 Newton iterations (2 DFMA each)"
        );
    }

    #[test]
    fn test_f64_sqrt_lowering_produces_dfma_sequence() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);

        let op = OpF64Sqrt {
            dst: dst.into(),
            src: Src::from(x.clone()),
        };
        let instr = Instr::new(op);
        let result = super::super::lower_instr(instr, &mut alloc, &sm);

        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions");
        };
        assert!(seq.len() > 10, "sqrt should expand to many instructions");
        let has_transcendental = seq.iter().any(|i| matches!(i.op, Op::Transcendental(_)));
        let has_dfma = seq.iter().any(|i| matches!(i.op, Op::DFma(_)));
        let has_dmul = seq.iter().any(|i| matches!(i.op, Op::DMul(_)));
        assert!(has_transcendental, "sqrt lowering should use MUFU.Rsq64H");
        assert!(has_dfma, "sqrt lowering should use DFMA");
        assert!(has_dmul, "sqrt lowering should use DMul");
    }

    #[test]
    fn test_f64_rcp_lowering_produces_dfma_sequence() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);

        let op = OpF64Rcp {
            dst: dst.into(),
            src: Src::from(x.clone()),
        };
        let instr = Instr::new(op);
        let result = super::super::lower_instr(instr, &mut alloc, &sm);

        let MappedInstrs::Many(seq) = result else {
            panic!("Expected Many instructions");
        };
        assert!(seq.len() > 5, "rcp should expand to multiple instructions");
        let has_transcendental = seq.iter().any(|i| matches!(i.op, Op::Transcendental(_)));
        let has_dmul = seq.iter().any(|i| matches!(i.op, Op::DMul(_)));
        assert!(has_transcendental, "rcp lowering should use MUFU.Rcp64H");
        assert!(has_dmul, "rcp lowering should use DMul");
    }

    #[test]
    fn test_f64_sqrt_lowering_direct() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);
        let op = OpF64Sqrt {
            dst: dst.into(),
            src: Src::from(x.clone()),
        };
        let seq = lower_f64_sqrt(&op, Pred::from(true), &mut alloc, &sm);
        assert!(
            seq.len() >= 15,
            "sqrt direct lowering should produce ~20+ instructions"
        );
        let has_transcendental = seq
            .iter()
            .any(|i| matches!(&i.op, Op::Transcendental(m) if m.op == TranscendentalOp::Rsq64H));
        assert!(has_transcendental);
    }

    #[test]
    fn test_f64_rcp_lowering_direct() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);
        let op = OpF64Rcp {
            dst: dst.into(),
            src: Src::from(x.clone()),
        };
        let seq = lower_f64_rcp(&op, Pred::from(true), &mut alloc, &sm);
        assert!(
            seq.len() >= 10,
            "rcp direct lowering should produce >= 10 instructions"
        );
        let has_transcendental = seq
            .iter()
            .any(|i| matches!(&i.op, Op::Transcendental(m) if m.op == TranscendentalOp::Rcp64H));
        assert!(has_transcendental);
    }

    #[test]
    fn test_f64_rcp_lowering_two_newton_iterations() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let x = alloc.alloc_vec(RegFile::GPR, 2);
        let dst = alloc.alloc_vec(RegFile::GPR, 2);
        let op = OpF64Rcp {
            dst: dst.into(),
            src: Src::from(x.clone()),
        };
        let seq = lower_f64_rcp(&op, Pred::from(true), &mut alloc, &sm);
        let dadd_count = seq.iter().filter(|i| matches!(i.op, Op::DAdd(_))).count();
        let dmul_count = seq.iter().filter(|i| matches!(i.op, Op::DMul(_))).count();
        assert!(
            dmul_count >= 3,
            "rcp has 2 iterations: x*y0, x*y1, y1*factor2"
        );
        assert!(dadd_count >= 2, "rcp computes 2 - x*y0 and 2 - x*y1");
    }
}
