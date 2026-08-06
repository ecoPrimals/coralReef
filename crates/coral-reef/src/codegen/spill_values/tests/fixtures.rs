// SPDX-License-Identifier: AGPL-3.0-or-later

use super::super::*;
use crate::codegen::ir::{
    BasicBlock, Instr, IntCmpOp, IntCmpType, LabelAllocator, OpCopy, OpExit, OpISetP, PhiAllocator,
    PredSetOp, Src,
};
use crate::codegen::ssa_value::SSAValueAllocator;
use coral_reef_stubs::cfg::CFGBuilder;

pub fn make_function_with_many_gprs(num_defs: usize) -> Function {
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

pub fn make_function_with_many_ugprs(num_defs: usize) -> Function {
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

pub fn make_function_with_many_preds(num_defs: usize) -> Function {
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

pub fn default_shader_info() -> ShaderInfo {
    ShaderInfo::compute([1, 1, 1], 0)
}

/// Tight GPR limits for spiller stress tests (named bounds, not magic literals in assertions).
pub const LIMIT_ONE_GPR: u32 = 1;
pub const LIMIT_TWO_GPR: u32 = 2;
/// `ParCopy` stress: `rel_limit` is `PAR_COPY_GPR_LIMIT - 1` with one live source; need more dst pairs than that.
pub const PAR_COPY_GPR_LIMIT: u32 = 3;
pub const PAR_COPY_DST_PAIR_COUNT: usize = 5;
pub const PRESEEDED_MEM_SPILLS: u32 = 11;
pub const PRESEEDED_MEM_FILLS: u32 = 6;

/// Many chained `OpCopy` defs (50+) with a tight limit — exercises the main spill path without relying
/// on a specific spill count (copy chains may fold live ranges).
pub const SPILL_STRESS_MANY_DEFS: usize = 52;
