// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)
//! Newton-Raphson lowering for f64 sqrt and rcp.
//!
//! Provenance: MUFU.RSQ64H/RCP64H seeds from NVIDIA ISA; 2-iteration refinement
//! targets ≤1 ULP (per ecosystem DF64 requirements and numerical-validation `tol::ANALYTICAL`).

use super::*;

/// Ensure `src` is a 2-component SSA ref. If copy propagation folded it to
/// immediates or CBuf references, materialize via OpCopy.
fn ensure_f64_ssa(src: &Src, alloc: &mut SSAValueAllocator, out: &mut Vec<Instr>) -> SSARef {
    if let SrcRef::SSA(ssa) = &src.reference {
        if ssa.comps() == 2 {
            return ssa.clone();
        }
    }
    let dst = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(Instr::new(OpCopy {
        dst: dst[0].into(),
        src: src.clone(),
    }));
    let mut hi_src = src.clone();
    if let SrcRef::SSA(ssa) = &hi_src.reference {
        if ssa.comps() >= 2 {
            hi_src.reference = SrcRef::SSA(SSARef::from(ssa[1]));
        }
    }
    out.push(Instr::new(OpCopy {
        dst: dst[1].into(),
        src: hi_src,
    }));
    dst
}

const F64_NEG_HALF: u32 = 0xBFE0_0000; // -0.5 as f32 bits (high word of f64)
const F64_ONE_HALF: u32 = 0x3FF8_0000; // 1.5

/// sqrt(x) = x * (1/sqrt(x)) via Newton-Raphson on 1/sqrt(x):
/// y₀ = seed(x_hi), y₁ = y₀·(3 - x·y₀²)/2, y₂ = y₁·(3 - x·y₁²)/2, result = x·y₂
///
/// On SM < 100: y₀ = MUFU.RSQ64H(x_hi) — hardware takes f64 high bits directly.
/// On SM ≥ 100 (Blackwell): y₀ = F2F(MUFU.RSQ(F2F(x))) — bypass RSQ64H via f32.
pub fn lower_f64_sqrt(
    op: &OpF64Sqrt,
    pred: Pred,
    alloc: &mut SSAValueAllocator,
    sm: &dyn ShaderModel,
) -> Vec<Instr> {
    let mut out = Vec::new();
    let rnd = FRndMode::NearestEven;

    let x = ensure_f64_ssa(&op.src, alloc, &mut out);
    let x_hi = Src::from(x[1]);
    let x_src = Src::from(x);

    let y0_src = if !sm.has_hardware_f64_rcp() {
        // Blackwell/Kepler path: F2F(f64→f32) + MUFU.RSQ + F2F(f32→f64)
        let x_as_f32 = alloc.alloc(RegFile::GPR);
        out.push(with_pred(
            Instr::new(OpF2F {
                dst: x_as_f32.into(),
                src: x_src.clone(),
                src_type: FloatType::F64,
                dst_type: FloatType::F32,
                rnd_mode: FRndMode::NearestEven,
                ftz: false,
                dst_high: false,
                integer_rnd: false,
            }),
            pred,
        ));

        let rsq_f32 = alloc.alloc(RegFile::GPR);
        out.push(with_pred(
            Instr::new(OpTranscendental {
                dst: rsq_f32.into(),
                op: TranscendentalOp::Rsq,
                src: x_as_f32.into(),
            }),
            pred,
        ));

        let y0 = alloc.alloc_vec(RegFile::GPR, 2);
        out.push(with_pred(
            Instr::new(OpF2F {
                dst: y0.clone().into(),
                src: rsq_f32.into(),
                src_type: FloatType::F32,
                dst_type: FloatType::F64,
                rnd_mode: FRndMode::NearestEven,
                ftz: false,
                dst_high: false,
                integer_rnd: false,
            }),
            pred,
        ));
        Src::from(y0)
    } else {
        // Pre-Blackwell path: MUFU.RSQ64H takes f64 high bits directly
        let y0_f32 = alloc.alloc(RegFile::GPR);
        out.push(with_pred(
            Instr::new(OpTranscendental {
                dst: y0_f32.into(),
                op: TranscendentalOp::Rsq64H,
                src: x_hi,
            }),
            pred,
        ));

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
        Src::from(y0)
    };

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
    let one_half_src = Src::from(one_half);

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
    let y1_src = Src::from(y1);

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
            srcs: [x_src.clone(), Src::from(y1_sq)],
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

/// AMD rcp(x) via Newton-Raphson with native V_RCP_F64 seed.
///
/// V_RCP_F64 on GCN/RDNA provides ~24-29 bits of mantissa precision.
/// Two Newton-Raphson iterations refine to full 53-bit double precision:
///   y₀ = V_RCP_F64(x), t = FMA(-x, yₙ, 2), yₙ₊₁ = yₙ * t
pub fn lower_f64_rcp_amd(op: &OpF64Rcp, pred: Pred, alloc: &mut SSAValueAllocator) -> Vec<Instr> {
    let mut out = Vec::new();
    let rnd = FRndMode::NearestEven;

    let x = ensure_f64_ssa(&op.src, alloc, &mut out);
    let x_src = Src::from(x);

    // y₀ = V_RCP_F64(x) — hardware seed (~24-29 bits)
    let y0 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpF64Rcp {
            dst: y0.clone().into(),
            src: x_src.clone(),
        }),
        pred,
    ));
    let y0_src = Src::from(y0);

    // Iteration 1: y₁ = y₀ * (2 - x*y₀) = y₀ * FMA(-x, y₀, 2)
    let two1 = super::emit_f64_const(&mut out, alloc, pred, 2.0);
    let t1 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: t1.clone().into(),
            srcs: [x_src.clone().fneg(), y0_src.clone(), Src::from(two1)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let y1 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: y1.clone().into(),
            srcs: [y0_src, Src::from(t1)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    let y1_src = Src::from(y1);

    // Iteration 2: y₂ = y₁ * (2 - x*y₁) = y₁ * FMA(-x, y₁, 2)
    let two2 = super::emit_f64_const(&mut out, alloc, pred, 2.0);
    let t2 = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: t2.clone().into(),
            srcs: [x_src.fneg(), y1_src.clone(), Src::from(two2)],
            rnd_mode: rnd,
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpDMul {
            dst: op.dst.clone(),
            srcs: [y1_src, Src::from(t2)],
            rnd_mode: rnd,
        }),
        pred,
    ));

    out
}

/// rcp(x) via Newton-Raphson: y₀ = seed(x), y₁ = y₀·(2 - x·y₀), y₂ = y₁·(2 - x·y₁)
///
/// On SM < 100: y₀ = MUFU.RCP64H(x_hi) — hardware takes f64 high bits directly.
/// On SM ≥ 100 (Blackwell): y₀ = F2F(MUFU.RCP(F2F(x))) — bypass RCP64H via f32
/// round-trip, with an extra Newton-Raphson iteration to recover full precision.
pub fn lower_f64_rcp(
    op: &OpF64Rcp,
    pred: Pred,
    alloc: &mut SSAValueAllocator,
    sm: &dyn ShaderModel,
) -> Vec<Instr> {
    let mut out = Vec::new();
    let rnd = FRndMode::NearestEven;

    let x = ensure_f64_ssa(&op.src, alloc, &mut out);
    let x_hi = Src::from(x[1]);
    let x_src = Src::from(x);

    let y0_src = if !sm.has_hardware_f64_rcp() {
        // Blackwell/Kepler path: F2F(f64→f32) + MUFU.RCP + F2F(f32→f64)
        let x_as_f32 = alloc.alloc(RegFile::GPR);
        out.push(with_pred(
            Instr::new(OpF2F {
                dst: x_as_f32.into(),
                src: x_src.clone(),
                src_type: FloatType::F64,
                dst_type: FloatType::F32,
                rnd_mode: FRndMode::NearestEven,
                ftz: false,
                dst_high: false,
                integer_rnd: false,
            }),
            pred,
        ));

        let rcp_f32 = alloc.alloc(RegFile::GPR);
        out.push(with_pred(
            Instr::new(OpTranscendental {
                dst: rcp_f32.into(),
                op: TranscendentalOp::Rcp,
                src: x_as_f32.into(),
            }),
            pred,
        ));

        let y0 = alloc.alloc_vec(RegFile::GPR, 2);
        out.push(with_pred(
            Instr::new(OpF2F {
                dst: y0.clone().into(),
                src: rcp_f32.into(),
                src_type: FloatType::F32,
                dst_type: FloatType::F64,
                rnd_mode: FRndMode::NearestEven,
                ftz: false,
                dst_high: false,
                integer_rnd: false,
            }),
            pred,
        ));
        Src::from(y0)
    } else {
        // Pre-Blackwell path: MUFU.RCP64H takes f64 high bits directly
        let y0_f32 = alloc.alloc(RegFile::GPR);
        out.push(with_pred(
            Instr::new(OpTranscendental {
                dst: y0_f32.into(),
                op: TranscendentalOp::Rcp64H,
                src: x_hi,
            }),
            pred,
        ));

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
        Src::from(y0)
    };

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
#[path = "newton_tests.rs"]
mod tests;
