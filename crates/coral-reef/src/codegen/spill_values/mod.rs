// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

#![allow(clippy::wildcard_imports)]

use super::const_tracker::ConstTracker;
use super::debug::{DEBUG, GetDebugFlags};
use super::ir::*;
use super::liveness::{BlockLiveness, LiveSet, Liveness, NextUseBlockLiveness, NextUseLiveness};

mod spiller;
mod types;

pub(super) use spiller::*;
pub(super) use types::*;

impl Function {
    /// Spill values from @file to fit within @limit registers
    ///
    /// This pass assumes that the function is already in CSSA form.  See
    /// @to_cssa for more details.
    ///
    /// The algorithm implemented here is roughly based on "Register Spilling
    /// and Live-Range Splitting for SSA-Form Programs" by Braun and Hack.  The
    /// primary contributions of the Braun and Hack paper are the global
    /// next-use distances which are implemented by @NextUseLiveness and a
    /// heuristic for computing spill sets at block boundaries.  The paper
    /// describes two sets:
    ///
    ///  - W, the set of variables currently resident
    ///
    ///  - S, the set of variables which have been spilled
    ///
    /// These sets are tracked as we walk instructions and \[un\]spill values to
    /// satisfy the given limit.  When spills are required we spill the value
    /// with the nighest next-use IP.  At block boundaries, Braun and Hack
    /// describe a heuristic for determining the starting W and S sets based on
    /// the W and S from the end of each of the forward edge predecessor blocks.
    ///
    /// What Braun and Hack do not describe is how to handle phis and parallel
    /// copies.  Because we assume the function is already in CSSA form, we can
    /// use a fairly simple algorithm.  On the first pass, we ignore phi sources
    /// and assign phi destinations based on W at the start of the block.  If
    /// the phi destination is in W, we leave it alone.  If it is not in W, then
    /// we allocate a new spill value and assign it to the phi destination.  In
    /// a second pass, we handle phi sources based on the destination.  If the
    /// destination is in W, we leave it alone.  If the destination is spilled,
    /// we read from the spill value corresponding to the source, spilling first
    /// if needed.  In the second pass, we also handle spilling across blocks as
    /// needed for values that do not pass through a phi.
    ///
    /// A special case is also required for parallel copies because they can
    /// have an unbounded number of destinations.  For any source values not in
    /// W, we allocate a spill value for the destination and copy in the spill
    /// register file.  For any sources which are in W, we try to leave as much
    /// in W as possible.  However, since source values may not be killed by the
    /// copy and because one source value may be copied to arbitrarily many
    /// destinations, that is not always possible.  Whenever we need to spill
    /// values, we spill according to the highest next-use of the destination
    /// and we spill the source first and then parallel copy the source into a
    /// spilled destination value.
    ///
    /// This all assumes that it's better to copy in spill space than to unspill
    /// just for the sake of a parallel copy.  While this may not be true in
    /// general, especially not when spilling to memory, the register allocator
    /// is good at eliding unnecessary copies.
    pub fn spill_values(&mut self, file: RegFile, limit: u32, info: &mut ShaderInfo) {
        match file {
            RegFile::GPR => {
                let spill = SpillGPR::new(info);
                spill_values(self, file, limit, spill);
            }
            RegFile::UGPR => {
                let spill = SpillUniform::new(info);
                spill_values(self, file, limit, spill);
            }
            RegFile::Pred => {
                let spill = SpillPred::new(info);
                spill_values(self, file, limit, spill);
            }
            RegFile::UPred => {
                let spill = SpillPred::new(info);
                spill_values(self, file, limit, spill);
            }
            RegFile::Bar => {
                let spill = SpillBar::new(info);
                spill_values(self, file, limit, spill);
            }
            _ => panic!("Don't know how to spill {} registers", file),
        }

        self.repair_ssa();
        self.opt_dce();

        if DEBUG.print() {
            eprintln!("IR after spilling {}:\n{}", file, self);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{
        BasicBlock, ComputeShaderInfo, Instr, LabelAllocator, Op, OpCopy, OpExit, PhiAllocator,
        ShaderIoInfo, ShaderStageInfo, Src,
    };
    use crate::codegen::ssa_value::SSAValueAllocator;
    use coral_reef_stubs::cfg::CFGBuilder;

    fn make_function_with_many_gprs(num_defs: usize) -> Function {
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

    #[test]
    fn test_spill_values_ugpr_with_high_pressure() {
        let mut func = make_function_with_many_ugprs(15);
        let mut info = ShaderInfo {
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
        };
        func.to_cssa();
        func.spill_values(RegFile::UGPR, 4, &mut info);
        assert!(!func.blocks[0].instrs.is_empty());
    }

    fn make_function_with_many_ugprs(num_defs: usize) -> Function {
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

    #[test]
    fn test_spill_values_preserves_semantics() {
        let mut func = make_function_with_many_gprs(8);
        let mut info = ShaderInfo {
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
        };
        func.to_cssa();
        func.spill_values(RegFile::GPR, 4, &mut info);
        assert!(!func.blocks[0].instrs.is_empty());
        let last = func.blocks[0].instrs.last().unwrap();
        assert!(matches!(last.op, Op::Exit(_)));
    }
}
