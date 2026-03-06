// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)
//! f64 transcendental software lowering.
//!
//! NVIDIA MUFU only supports f32 transcendentals. For f64 sqrt and rcp,
//! we expand to Newton-Raphson sequences using MUFU.RSQ64H/RCP64H seeds + DFMA.
//! For exp2, log2, sin, cos we use polynomial approximation.
//!
//! ## Cross-spring provenance
//!
//! Algorithms and ULP targets derive from:
//! - **hotSpring**: DF64 precision requirements for Yukawa force / molecular dynamics
//! - **barraCuda**: `math_f64.wgsl`, `df64_transcendentals.wgsl` reference coefficients
//! - **groundSpring**: 13-tier tolerance architecture (`tol::ANALYTICAL` ≈ 1e-10 for
//!   single-transcendental paths), validation pipeline (34 binaries, 395 checks)
//! - **F64_LOWERING_THEORY.md**: MUFU seed + Newton-Raphson + Horner + Cody-Waite theory

#![allow(clippy::wildcard_imports, clippy::redundant_clone)]

use super::ir::*;

pub mod newton;
pub mod poly;

/// Lower f64 transcendental placeholder ops to hardware sequences.
///
/// NVIDIA: MUFU seed + Newton-Raphson (MUFU only does f32).
/// AMD: native `v_sqrt_f64`/`v_rcp_f64` — no lowering needed for
/// sqrt/rcp. Transcendentals (sin/cos/exp2/log2) still need polynomial
/// lowering on both vendors.
pub fn lower_f64_function(func: &mut Function, sm: &dyn ShaderModel) {
    let is_amd = sm.sm() >= 100;
    if !is_amd && sm.sm() < 70 {
        return;
    }
    func.map_instrs(|instr, alloc| lower_instr(instr, alloc, sm));
}

pub fn lower_instr(
    instr: Instr,
    alloc: &mut SSAValueAllocator,
    sm: &dyn ShaderModel,
) -> MappedInstrs {
    let is_amd = sm.sm() >= 100;
    let pred = instr.pred;
    match instr.op {
        Op::F64Exp2(op) => {
            if is_amd {
                // AMD: polynomial via v_fma_f64 (same algorithm, different seed)
                // For now, pass through — encoder handles natively
                return MappedInstrs::One(Instr { op: Op::F64Exp2(op), pred, ..instr });
            }
            let seq = poly::lower_f64_exp2(&op, pred, alloc, sm);
            MappedInstrs::Many(seq)
        }
        Op::F64Log2(op) => {
            if is_amd {
                return MappedInstrs::One(Instr { op: Op::F64Log2(op), pred, ..instr });
            }
            let seq = poly::lower_f64_log2(&op, pred, alloc, sm);
            MappedInstrs::Many(seq)
        }
        Op::F64Sin(op) => {
            if is_amd {
                return MappedInstrs::One(Instr { op: Op::F64Sin(op), pred, ..instr });
            }
            let seq = poly::lower_f64_sin(&op, pred, alloc, sm);
            MappedInstrs::Many(seq)
        }
        Op::F64Cos(op) => {
            if is_amd {
                return MappedInstrs::One(Instr { op: Op::F64Cos(op), pred, ..instr });
            }
            let seq = poly::lower_f64_cos(&op, pred, alloc, sm);
            MappedInstrs::Many(seq)
        }
        Op::F64Sqrt(op) => {
            if is_amd {
                // AMD: native v_sqrt_f64 — no lowering needed
                return MappedInstrs::One(Instr { op: Op::F64Sqrt(op), pred, ..instr });
            }
            let seq = newton::lower_f64_sqrt(&op, pred, alloc, sm);
            MappedInstrs::Many(seq)
        }
        Op::F64Rcp(op) => {
            if is_amd {
                // AMD: native v_rcp_f64 — no lowering needed
                return MappedInstrs::One(Instr { op: Op::F64Rcp(op), pred, ..instr });
            }
            let seq = newton::lower_f64_rcp(&op, pred, alloc, sm);
            MappedInstrs::Many(seq)
        }
        _ => MappedInstrs::One(instr),
    }
}

pub(super) fn with_pred(mut instr: Instr, pred: Pred) -> Instr {
    instr.pred = pred;
    instr
}

pub(super) fn emit_f64_zero(
    out: &mut Vec<Instr>,
    alloc: &mut SSAValueAllocator,
    pred: Pred,
) -> SSARef {
    let zero = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: zero[0].into(),
            src: Src::ZERO,
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: zero[1].into(),
            src: Src::ZERO,
        }),
        pred,
    ));
    zero
}

pub(super) fn emit_f64_const(
    out: &mut Vec<Instr>,
    alloc: &mut SSAValueAllocator,
    pred: Pred,
    val: f64,
) -> SSARef {
    let bits = val.to_bits();
    let low = (bits & 0xFFFF_FFFF) as u32;
    let high = (bits >> 32) as u32;
    let dst = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: dst[0].into(),
            src: Src::new_imm_u32(low),
        }),
        pred,
    ));
    out.push(with_pred(
        Instr::new(OpCopy {
            dst: dst[1].into(),
            src: Src::new_imm_u32(high),
        }),
        pred,
    ));
    dst
}

impl Shader<'_> {
    /// Lower f64 transcendental placeholders (OpF64Sqrt, OpF64Rcp, OpF64Exp2, OpF64Log2, OpF64Sin, OpF64Cos) to DFMA/MUFU sequences.
    pub fn lower_f64_transcendentals(&mut self) {
        for func in &mut self.functions {
            lower_f64_function(func, self.sm);
        }
    }
}
