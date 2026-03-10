// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! FMA contraction lowering — NoContraction / `FmaPolicy::Separate` enforcement.
//!
//! When `FmaPolicy::Separate` is active, fused multiply-add instructions
//! (`FFma`, `DFma`) are split into separate multiply + add pairs. This
//! ensures IEEE-754 compliant intermediate rounding — each operation rounds
//! independently, matching CPU behavior for bit-exact reproducibility.
//!
//! ## Motivation (ISSUE-011)
//!
//! GPU FMA fusion changes rounding behavior: `a*b + c` as FMA rounds once,
//! while `(a*b) + c` rounds twice. For algorithms with catastrophic
//! cancellation (e.g. plasma dispersion, Kahan summation), the single-ULP
//! difference gets amplified 50-1000×. `FmaPolicy::Separate` prevents this.
//!
//! ## SPIR-V `NoContraction`
//!
//! SPIR-V's `NoContraction` decoration maps directly to this pass. When a
//! shader requests NoContraction on an operation, `FmaPolicy::Separate`
//! prevents the compiler from fusing it into FMA.

#![allow(clippy::wildcard_imports)]

use super::ir::*;
use crate::FmaPolicy;

/// Split `FFma(dst, [a, b, c])` into `FMul(tmp, [a, b]) + FAdd(dst, [tmp, c])`.
fn split_ffma(ffma: OpFFma, pred: Pred, alloc: &mut SSAValueAllocator) -> MappedInstrs {
    let tmp = alloc.alloc(RegFile::GPR);
    let mul = Instr {
        op: Op::FMul(Box::new(OpFMul {
            dst: tmp.into(),
            srcs: [ffma.srcs[0].clone(), ffma.srcs[1].clone()],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: ffma.ftz,
            dnz: ffma.dnz,
        })),
        pred,
        deps: InstrDeps::new(),
    };
    let add = Instr {
        op: Op::FAdd(Box::new(OpFAdd {
            dst: ffma.dst,
            srcs: [tmp.into(), ffma.srcs[2].clone()],
            saturate: ffma.saturate,
            rnd_mode: FRndMode::NearestEven,
            ftz: ffma.ftz,
        })),
        pred,
        deps: InstrDeps::new(),
    };
    MappedInstrs::Many(vec![mul, add])
}

/// Split `DFma(dst, [a, b, c])` into `DMul(tmp, [a, b]) + DAdd(dst, [tmp, c])`.
fn split_dfma(dfma: OpDFma, pred: Pred, alloc: &mut SSAValueAllocator) -> MappedInstrs {
    let tmp = alloc.alloc_vec(RegFile::GPR, 2);
    let mul = Instr {
        op: Op::DMul(Box::new(OpDMul {
            dst: Dst::SSA(tmp.clone()),
            srcs: [dfma.srcs[0].clone(), dfma.srcs[1].clone()],
            rnd_mode: FRndMode::NearestEven,
        })),
        pred,
        deps: InstrDeps::new(),
    };
    let add = Instr {
        op: Op::DAdd(Box::new(OpDAdd {
            dst: dfma.dst,
            srcs: [Src::from(tmp), dfma.srcs[2].clone()],
            rnd_mode: FRndMode::NearestEven,
        })),
        pred,
        deps: InstrDeps::new(),
    };
    MappedInstrs::Many(vec![mul, add])
}

/// Map a single instruction, splitting FMA operations when policy demands.
fn lower_fma_instr(instr: Instr, alloc: &mut SSAValueAllocator) -> MappedInstrs {
    let pred = instr.pred;
    match instr.op {
        Op::FFma(ffma) => split_ffma(*ffma, pred, alloc),
        Op::DFma(dfma) => split_dfma(*dfma, pred, alloc),
        _ => MappedInstrs::One(instr),
    }
}

impl Shader<'_> {
    /// Lower FMA instructions to separate mul + add when `FmaPolicy::Separate`.
    ///
    /// `FmaPolicy::Fused` and `FmaPolicy::Auto` leave FMA instructions intact.
    pub fn lower_fma_contractions(&mut self) {
        if self.fma_policy != FmaPolicy::Separate {
            return;
        }
        for func in &mut self.functions {
            func.map_instrs(|instr, alloc| lower_fma_instr(instr, alloc));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{
        BasicBlock, ComputeShaderInfo, Function, LabelAllocator, PhiAllocator, ShaderInfo,
        ShaderIoInfo, ShaderModelInfo, ShaderStageInfo,
    };
    use coral_reef_stubs::cfg::CFGBuilder;

    fn make_test_shader(
        instrs: Vec<Instr>,
        alloc: SSAValueAllocator,
        fma_policy: FmaPolicy,
    ) -> Shader<'static> {
        let sm: &dyn ShaderModel = Box::leak(Box::new(ShaderModelInfo::new(70, 64)));
        let mut label_alloc = LabelAllocator::new();
        let mut cfg_builder = CFGBuilder::new();
        let block = BasicBlock {
            label: label_alloc.alloc(),
            uniform: false,
            instrs,
        };
        cfg_builder.add_block(block);
        let function = Function {
            ssa_alloc: alloc,
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
                uses_fp64: false,
                stage: ShaderStageInfo::Compute(ComputeShaderInfo {
                    local_size: [1, 1, 1],
                    shared_mem_size: 0,
                }),
                io: ShaderIoInfo::None,
            },
            functions: vec![function],
            fma_policy,
        }
    }

    fn make_ffma_shader(fma_policy: FmaPolicy) -> Shader<'static> {
        let mut alloc = SSAValueAllocator::new();
        let a = alloc.alloc(RegFile::GPR);
        let b = alloc.alloc(RegFile::GPR);
        let c = alloc.alloc(RegFile::GPR);
        let dst = alloc.alloc(RegFile::GPR);

        let ffma_instr = Instr::new(OpFFma {
            dst: dst.into(),
            srcs: [a.into(), b.into(), c.into()],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        });

        make_test_shader(vec![ffma_instr], alloc, fma_policy)
    }

    #[test]
    fn separate_splits_ffma_into_fmul_fadd() {
        let mut shader = make_ffma_shader(FmaPolicy::Separate);
        assert_eq!(
            shader.functions[0]
                .blocks
                .iter()
                .next()
                .unwrap()
                .instrs
                .len(),
            1
        );
        shader.lower_fma_contractions();
        let instrs = &shader.functions[0].blocks.iter().next().unwrap().instrs;
        assert_eq!(instrs.len(), 2, "FFma should be split into FMul + FAdd");
        assert!(matches!(instrs[0].op, Op::FMul(_)));
        assert!(matches!(instrs[1].op, Op::FAdd(_)));
    }

    #[test]
    fn auto_preserves_ffma() {
        let mut shader = make_ffma_shader(FmaPolicy::Auto);
        shader.lower_fma_contractions();
        let instrs = &shader.functions[0].blocks.iter().next().unwrap().instrs;
        assert_eq!(instrs.len(), 1);
        assert!(matches!(instrs[0].op, Op::FFma(_)));
    }

    #[test]
    fn fused_preserves_ffma() {
        let mut shader = make_ffma_shader(FmaPolicy::Fused);
        shader.lower_fma_contractions();
        let instrs = &shader.functions[0].blocks.iter().next().unwrap().instrs;
        assert_eq!(instrs.len(), 1);
        assert!(matches!(instrs[0].op, Op::FFma(_)));
    }
}
