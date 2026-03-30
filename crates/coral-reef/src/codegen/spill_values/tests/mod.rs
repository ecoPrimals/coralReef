// SPDX-License-Identifier: AGPL-3.0-only

use super::*;
use crate::codegen::ir::{
    BasicBlock, ComputeShaderInfo, Instr, LabelAllocator, Op, OpCopy, OpExit, OpISetP,
    PhiAllocator, PredSetOp, ShaderIoInfo, ShaderStageInfo, Src,
};
use crate::codegen::ir::{IntCmpOp, IntCmpType};
use crate::codegen::ssa_value::SSAValueAllocator;
use coral_reef_stubs::cfg::CFGBuilder;

mod cfg;
mod pressure;
mod regfile;
mod stats;

pub(super) fn make_function_with_many_gprs(num_defs: usize) -> Function {
    let mut ssa_alloc = SSAValueAllocator::new();
    let mut instrs = Vec::new();
    let base = ssa_alloc.alloc(RegFile::GPR);
    instrs.push(Instr::new(OpCopy {
        dst: base.into(),
        src: Src::ZERO,
    }));
    for _ in 1..num_defs {
        let next = ssa_alloc.alloc(RegFile::GPR);
        instrs.push(Instr::new(OpCopy {
            dst: next.into(),
            src: base.into(),
        }));
    }
    instrs.push(Instr::new(OpExit {}));

    let mut label_alloc = LabelAllocator::new();
    let mut cfg_builder = CFGBuilder::new();
    cfg_builder.add_block(BasicBlock {
        label: label_alloc.alloc(),
        uniform: false,
        instrs,
    });
    Function {
        ssa_alloc,
        phi_alloc: PhiAllocator::new(),
        blocks: cfg_builder.build(),
    }
}

pub(super) fn make_function_with_many_ugprs(num_defs: usize) -> Function {
    let mut ssa_alloc = SSAValueAllocator::new();
    let mut instrs = Vec::new();
    let base = ssa_alloc.alloc(RegFile::UGPR);
    instrs.push(Instr::new(OpCopy {
        dst: base.into(),
        src: Src::ZERO,
    }));
    for _ in 1..num_defs {
        let next = ssa_alloc.alloc(RegFile::UGPR);
        instrs.push(Instr::new(OpCopy {
            dst: next.into(),
            src: base.into(),
        }));
    }
    instrs.push(Instr::new(OpExit {}));

    let mut label_alloc = LabelAllocator::new();
    let mut cfg_builder = CFGBuilder::new();
    cfg_builder.add_block(BasicBlock {
        label: label_alloc.alloc(),
        uniform: true,
        instrs,
    });
    Function {
        ssa_alloc,
        phi_alloc: PhiAllocator::new(),
        blocks: cfg_builder.build(),
    }
}

pub(super) fn make_function_with_many_preds(num_defs: usize) -> Function {
    let mut ssa_alloc = SSAValueAllocator::new();
    let mut instrs = Vec::new();
    let base = ssa_alloc.alloc(RegFile::GPR);
    instrs.push(Instr::new(OpCopy {
        dst: base.into(),
        src: Src::ZERO,
    }));
    for _ in 0..num_defs {
        let p = ssa_alloc.alloc(RegFile::Pred);
        instrs.push(Instr::new(OpISetP {
            dst: p.into(),
            set_op: PredSetOp::And,
            cmp_op: IntCmpOp::Ne,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [base.into(), base.into(), true.into(), true.into()],
        }));
        let _ = p;
    }
    instrs.push(Instr::new(OpExit {}));

    let mut label_alloc = LabelAllocator::new();
    let mut cfg_builder = CFGBuilder::new();
    cfg_builder.add_block(BasicBlock {
        label: label_alloc.alloc(),
        uniform: false,
        instrs,
    });
    Function {
        ssa_alloc,
        phi_alloc: PhiAllocator::new(),
        blocks: cfg_builder.build(),
    }
}

pub(super) fn default_shader_info() -> ShaderInfo {
    ShaderInfo {
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
    }
}

/// Tight GPR limits for spiller stress tests (named bounds, not magic literals in assertions).
pub(super) const LIMIT_ONE_GPR: u32 = 1;
pub(super) const LIMIT_TWO_GPR: u32 = 2;
/// `ParCopy` stress: `rel_limit` is `PAR_COPY_GPR_LIMIT - 1` with one live source; need more dst pairs than that.
pub(super) const PAR_COPY_GPR_LIMIT: u32 = 3;
pub(super) const PAR_COPY_DST_PAIR_COUNT: usize = 5;
pub(super) const PRESEEDED_MEM_SPILLS: u32 = 11;
pub(super) const PRESEEDED_MEM_FILLS: u32 = 6;

/// Many chained `OpCopy` defs (50+) with a tight limit — exercises the main spill path without relying
/// on a specific spill count (copy chains may fold live ranges).
pub(super) const SPILL_STRESS_MANY_DEFS: usize = 52;
