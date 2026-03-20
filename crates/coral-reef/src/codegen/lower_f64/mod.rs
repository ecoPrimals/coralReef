// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)
#![allow(
    clippy::redundant_clone,
    reason = "f64 lowering follows uniform Src/SSA builder patterns; clippy flags clones that match upstream style and keep ref usage consistent."
)]
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
//! - Ecosystem reference: `math_f64.wgsl`, `df64_transcendentals.wgsl` coefficients
//! - **groundSpring**: 13-tier tolerance architecture (`tol::ANALYTICAL` ≈ 1e-10 for
//!   single-transcendental paths), validation pipeline (34 binaries, 395 checks)
//! - **F64_LOWERING_THEORY.md**: MUFU seed + Newton-Raphson + Horner + Cody-Waite theory

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
    let is_amd = sm.is_amd();
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
    let is_amd = sm.is_amd();
    let pred = instr.pred;
    match instr.op {
        Op::F64Exp2(op) => {
            if is_amd {
                // AMD: polynomial via v_fma_f64 (same algorithm, different seed)
                // For now, pass through — encoder handles natively
                return MappedInstrs::One(Instr {
                    op: Op::F64Exp2(op),
                    pred,
                    ..instr
                });
            }
            let seq = poly::lower_f64_exp2(&op, pred, alloc, sm);
            MappedInstrs::Many(seq)
        }
        Op::F64Log2(op) => {
            if is_amd {
                return MappedInstrs::One(Instr {
                    op: Op::F64Log2(op),
                    pred,
                    ..instr
                });
            }
            let seq = poly::lower_f64_log2(&op, pred, alloc, sm);
            MappedInstrs::Many(seq)
        }
        Op::F64Sin(op) => {
            if is_amd {
                return MappedInstrs::One(Instr {
                    op: Op::F64Sin(op),
                    pred,
                    ..instr
                });
            }
            let seq = poly::lower_f64_sin(&op, pred, alloc, sm);
            MappedInstrs::Many(seq)
        }
        Op::F64Cos(op) => {
            if is_amd {
                return MappedInstrs::One(Instr {
                    op: Op::F64Cos(op),
                    pred,
                    ..instr
                });
            }
            let seq = poly::lower_f64_cos(&op, pred, alloc, sm);
            MappedInstrs::Many(seq)
        }
        Op::F64Sqrt(op) => {
            if is_amd {
                // AMD: native v_sqrt_f64 — no lowering needed
                return MappedInstrs::One(Instr {
                    op: Op::F64Sqrt(op),
                    pred,
                    ..instr
                });
            }
            let seq = newton::lower_f64_sqrt(&op, pred, alloc, sm);
            MappedInstrs::Many(seq)
        }
        Op::F64Rcp(op) => {
            if is_amd {
                // AMD: native v_rcp_f64 — no lowering needed
                return MappedInstrs::One(Instr {
                    op: Op::F64Rcp(op),
                    pred,
                    ..instr
                });
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

/// Emit a single f64 fused multiply-add, allocating a fresh SSA destination.
pub(super) fn emit_f64_dfma(
    out: &mut Vec<Instr>,
    alloc: &mut SSAValueAllocator,
    pred: Pred,
    a: Src,
    b: Src,
    c: Src,
) -> SSARef {
    let dst = alloc.alloc_vec(RegFile::GPR, 2);
    out.push(with_pred(
        Instr::new(OpDFma {
            dst: dst.clone().into(),
            srcs: [a, b, c],
            rnd_mode: FRndMode::NearestEven,
        }),
        pred,
    ));
    dst
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

#[cfg(test)]
mod tests {
    use super::super::ir::{
        BasicBlock, ComputeShaderInfo, Function, Instr, LabelAllocator, Op, OpCopy, OpExit,
        OpF64Exp2, OpF64Sqrt, PhiAllocator, RegFile, SSAValueAllocator, Shader, ShaderInfo,
        ShaderIoInfo, ShaderModelInfo, ShaderStageInfo, Src,
    };
    use coral_reef_stubs::cfg::CFGBuilder;

    fn make_shader_with_f64_op(
        instrs: Vec<Instr>,
        ssa_alloc: SSAValueAllocator,
    ) -> Shader<'static> {
        let sm = Box::leak(Box::new(ShaderModelInfo::new(70, 64)));
        let mut label_alloc = LabelAllocator::new();
        let mut cfg_builder = CFGBuilder::new();
        let block = BasicBlock {
            label: label_alloc.alloc(),
            uniform: false,
            instrs,
        };
        cfg_builder.add_block(block);
        let function = Function {
            ssa_alloc,
            phi_alloc: PhiAllocator::new(),
            blocks: cfg_builder.build(),
        };
        Shader {
            sm,
            info: ShaderInfo {
                max_warps_per_sm: 0,
                gpr_count: 0,
                control_barrier_count: 0,
                instr_count: 0,
                static_cycle_count: 0,
                spills_to_mem: 0,
                fills_from_mem: 0,
                spills_to_reg: 0,
                fills_from_reg: 0,
                shared_local_mem_size: 0,
                max_crs_depth: 0,
                uses_global_mem: false,
                writes_global_mem: false,
                uses_fp64: true,
                stage: ShaderStageInfo::Compute(ComputeShaderInfo {
                    local_size: [1, 1, 1],
                    shared_mem_size: 0,
                }),
                io: ShaderIoInfo::None,
            },
            functions: vec![function],
            fma_policy: crate::FmaPolicy::default(),
        }
    }

    #[test]
    fn test_lower_f64_transcendentals_exp2_produces_dfma() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let src = ssa_alloc.alloc_vec(RegFile::GPR, 2);
        let dst = ssa_alloc.alloc_vec(RegFile::GPR, 2);
        let mut shader = make_shader_with_f64_op(
            vec![
                Instr::new(OpCopy {
                    dst: src[0].into(),
                    src: Src::ZERO,
                }),
                Instr::new(OpCopy {
                    dst: src[1].into(),
                    src: Src::new_imm_u32(0x3FF0_0000),
                }),
                Instr::new(OpF64Exp2 {
                    dst: dst.into(),
                    src: Src::from(src),
                }),
                Instr::new(OpExit {}),
            ],
            ssa_alloc,
        );
        shader.lower_f64_transcendentals();
        let blocks = &shader.functions[0].blocks;
        let mut total_instrs = 0;
        let mut has_dfma = false;
        for b in blocks {
            for instr in &b.instrs {
                total_instrs += 1;
                if matches!(instr.op, Op::DFma(_)) {
                    has_dfma = true;
                }
            }
        }
        assert!(
            total_instrs > 5,
            "exp2 lowering should expand to many instructions"
        );
        assert!(has_dfma, "exp2 polynomial should use DFMA");
    }

    #[test]
    fn test_lower_f64_transcendentals_sqrt_produces_newton() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let src = ssa_alloc.alloc_vec(RegFile::GPR, 2);
        let dst = ssa_alloc.alloc_vec(RegFile::GPR, 2);
        let mut shader = make_shader_with_f64_op(
            vec![
                Instr::new(OpCopy {
                    dst: src[0].into(),
                    src: Src::ZERO,
                }),
                Instr::new(OpCopy {
                    dst: src[1].into(),
                    src: Src::new_imm_u32(0x3FF0_0000),
                }),
                Instr::new(OpF64Sqrt {
                    dst: dst.into(),
                    src: Src::from(src),
                }),
                Instr::new(OpExit {}),
            ],
            ssa_alloc,
        );
        shader.lower_f64_transcendentals();
        let mut has_rsq64h = false;
        for b in &shader.functions[0].blocks {
            for instr in &b.instrs {
                if let Op::Transcendental(t) = &instr.op {
                    if matches!(t.op, super::super::ir::TranscendentalOp::Rsq64H) {
                        has_rsq64h = true;
                    }
                }
            }
        }
        assert!(has_rsq64h, "sqrt Newton-Raphson should use Rsq64H seed");
    }
}
