// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)
//! Polynomial lowering for f64 exp2, log2, sin, cos.
//!
//! Provenance: Horner coefficients from Cephes/FDLIBM; Cody-Waite range reduction
//! for sin/cos from Payne-Hanek literature. ULP budgets: exp2/log2 ≤2, sin/cos ≤4
//! (per barraCuda `df64_transcendentals.wgsl` and groundSpring validation targets).

#![allow(clippy::wildcard_imports, clippy::redundant_clone)]

use super::*;

pub mod exp2;
pub mod log2;
pub mod trig;

pub use exp2::lower_f64_exp2;
pub use log2::lower_f64_log2;
pub use trig::{lower_f64_cos, lower_f64_sin};

// Cody-Waite constants: 2/π, (π/2)_hi, (π/2)_lo (high-precision for range reduction)
#[expect(
    clippy::approx_constant,
    reason = "exact Cody-Waite coefficient, not std::f64::consts"
)]
const TWO_OVER_PI: f64 = 0.636_619_772_367_581_4;
#[expect(
    clippy::approx_constant,
    reason = "exact Cody-Waite coefficient, not std::f64::consts"
)]
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

    let pi_half_hi = emit_f64_const(out, alloc, pred, PI_HALF_HI);
    let pi_half_lo = emit_f64_const(out, alloc, pred, PI_HALF_LO);
    let r = emit_f64_dfma(
        out,
        alloc,
        pred,
        Src::from(n_f64.clone()).fneg(),
        Src::from(pi_half_hi),
        x_src,
    );
    let r = emit_f64_dfma(
        out,
        alloc,
        pred,
        Src::from(n_f64).fneg(),
        Src::from(pi_half_lo),
        Src::from(r),
    );

    (r, n_i32)
}

/// Component-wise select for f64: cond ? a : b
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
            srcs: [cond.into(), a[0].into(), b[0].into()],
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpSel {
            dst: dst[1].into(),
            srcs: [cond.into(), a[1].into(), b[1].into()],
        }),
        pred,
    ));
    dst
}
