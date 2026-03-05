// Copyright © 2023 Collabora, Ltd.
// SPDX-License-Identifier: MIT
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

#![allow(clippy::wildcard_imports)]

use super::ir::*;

pub mod newton;
pub mod poly;

/// Lower f64 transcendental placeholder ops to DFMA sequences.
pub fn lower_f64_function(func: &mut Function, sm: &ShaderModelInfo) {
    if sm.sm() < 70 {
        return;
    }
    func.map_instrs(|instr, alloc| lower_instr(instr, alloc, sm));
}

pub(crate) fn lower_instr(
    instr: Instr,
    alloc: &mut SSAValueAllocator,
    sm: &ShaderModelInfo,
) -> MappedInstrs {
    let pred = instr.pred;
    match instr.op {
        Op::F64Exp2(op) => {
            let seq = poly::lower_f64_exp2(&op, pred, alloc, sm);
            MappedInstrs::Many(seq)
        }
        Op::F64Log2(op) => {
            let seq = poly::lower_f64_log2(&op, pred, alloc, sm);
            MappedInstrs::Many(seq)
        }
        Op::F64Sin(op) => {
            let seq = poly::lower_f64_sin(&op, pred, alloc, sm);
            MappedInstrs::Many(seq)
        }
        Op::F64Cos(op) => {
            let seq = poly::lower_f64_cos(&op, pred, alloc, sm);
            MappedInstrs::Many(seq)
        }
        Op::F64Sqrt(op) => {
            let seq = newton::lower_f64_sqrt(&op, pred, alloc, sm);
            MappedInstrs::Many(seq)
        }
        Op::F64Rcp(op) => {
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
