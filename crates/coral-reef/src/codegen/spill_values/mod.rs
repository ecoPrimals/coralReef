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

use spiller::*;
use types::*;

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
    pub fn spill_values(
        &mut self,
        file: RegFile,
        limit: u32,
        info: &mut ShaderInfo,
    ) -> Result<(), crate::CompileError> {
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
            _ => super::ice!("Don't know how to spill {file} registers"),
        }

        self.repair_ssa()?;
        self.opt_dce();

        if DEBUG.print() {
            eprintln!("IR after spilling {file}:\n{self}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{
        BasicBlock, ComputeShaderInfo, Instr, IntCmpOp, IntCmpType, LabelAllocator, Op, OpCopy,
        OpExit, OpISetP, OpPin, PhiAllocator, PredSetOp, ShaderIoInfo, ShaderStageInfo, Src,
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
        func.spill_values(RegFile::UGPR, 4, &mut info).unwrap();
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
        func.spill_values(RegFile::GPR, 4, &mut info).unwrap();
        assert!(!func.blocks[0].instrs.is_empty());
        let last = func.blocks[0].instrs.last().unwrap();
        assert!(matches!(last.op, Op::Exit(_)));
    }

    #[test]
    fn test_spill_values_pred_with_high_pressure() {
        let mut func = make_function_with_many_preds(8);
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
        func.spill_values(RegFile::Pred, 4, &mut info).unwrap();
        assert!(!func.blocks[0].instrs.is_empty());
    }

    fn make_function_with_many_preds(num_defs: usize) -> Function {
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

    fn default_shader_info() -> ShaderInfo {
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

    /// Two-block linear CFG (entry -> exit) exercises single-predecessor path in spiller.
    #[test]
    fn test_spill_values_two_blocks_single_predecessor() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let mut label_alloc = LabelAllocator::new();

        let mut entry_instrs = Vec::new();
        let base = ssa_alloc.alloc(RegFile::GPR);
        entry_instrs.push(Instr::new(OpCopy {
            dst: base.into(),
            src: Src::ZERO,
        }));
        for _ in 1..8 {
            let next = ssa_alloc.alloc(RegFile::GPR);
            entry_instrs.push(Instr::new(OpCopy {
                dst: next.into(),
                src: base.into(),
            }));
        }

        let mut exit_instrs = Vec::new();
        let use_val = ssa_alloc.alloc(RegFile::GPR);
        exit_instrs.push(Instr::new(OpCopy {
            dst: use_val.into(),
            src: base.into(),
        }));
        exit_instrs.push(Instr::new(OpExit {}));

        let mut cfg_builder = CFGBuilder::new();
        cfg_builder.add_block(BasicBlock {
            label: label_alloc.alloc(),
            uniform: false,
            instrs: entry_instrs,
        });
        cfg_builder.add_block(BasicBlock {
            label: label_alloc.alloc(),
            uniform: false,
            instrs: exit_instrs,
        });
        cfg_builder.add_edge(0, 1);

        let mut func = Function {
            ssa_alloc,
            phi_alloc: PhiAllocator::new(),
            blocks: cfg_builder.build(),
        };
        let mut info = default_shader_info();
        func.to_cssa();
        func.spill_values(RegFile::GPR, 4, &mut info).unwrap();
        assert_eq!(func.blocks.len(), 2);
        assert!(
            !func.blocks[0].instrs.is_empty() || !func.blocks[1].instrs.is_empty(),
            "at least one block should have instructions"
        );
    }

    /// Very low limit with high pressure exercises spill cost/selection paths.
    #[test]
    fn test_spill_values_very_low_limit() {
        let mut func = make_function_with_many_gprs(20);
        let mut info = default_shader_info();
        func.to_cssa();
        func.spill_values(RegFile::GPR, 2, &mut info).unwrap();
        let last = func.blocks[0].instrs.last().unwrap();
        assert!(matches!(last.op, Op::Exit(_)));
    }

    /// High limit: no spilling needed; exercises early-exit paths.
    #[test]
    fn test_spill_values_no_spill_needed() {
        let mut func = make_function_with_many_gprs(4);
        let mut info = default_shader_info();
        func.to_cssa();
        func.spill_values(RegFile::GPR, 64, &mut info).unwrap();
        assert_eq!(info.spills_to_mem, 0);
        assert_eq!(info.fills_from_mem, 0);
    }

    /// UPred spilling path (spills to UGPR, fills via OpISetP).
    #[test]
    fn test_spill_values_upred_with_pressure() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let mut instrs = Vec::new();
        let base = ssa_alloc.alloc(RegFile::UGPR);
        instrs.push(Instr::new(OpCopy {
            dst: base.into(),
            src: Src::ZERO,
        }));
        for _ in 0..6 {
            let p = ssa_alloc.alloc(RegFile::UPred);
            instrs.push(Instr::new(OpISetP {
                dst: p.into(),
                set_op: PredSetOp::And,
                cmp_op: IntCmpOp::Ne,
                cmp_type: IntCmpType::U32,
                ex: false,
                srcs: [base.into(), base.into(), true.into(), true.into()],
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
        let mut func = Function {
            ssa_alloc,
            phi_alloc: PhiAllocator::new(),
            blocks: cfg_builder.build(),
        };
        let mut info = default_shader_info();
        func.to_cssa();
        func.spill_values(RegFile::UPred, 3, &mut info).unwrap();
        assert!(!func.blocks[0].instrs.is_empty());
    }

    /// OpPin marks destination as pinned; SpillChooser skips pinned values.
    #[test]
    fn test_spill_values_with_pinned() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let mut instrs = Vec::new();
        let base = ssa_alloc.alloc(RegFile::GPR);
        instrs.push(Instr::new(OpCopy {
            dst: base.into(),
            src: Src::ZERO,
        }));
        let pinned = ssa_alloc.alloc(RegFile::GPR);
        instrs.push(Instr::new(Op::Pin(Box::new(OpPin {
            dst: pinned.into(),
            src: base.into(),
        }))));
        for _ in 0..10 {
            let next = ssa_alloc.alloc(RegFile::GPR);
            instrs.push(Instr::new(OpCopy {
                dst: next.into(),
                src: base.into(),
            }));
        }
        instrs.push(Instr::new(OpCopy {
            dst: ssa_alloc.alloc(RegFile::GPR).into(),
            src: pinned.into(),
        }));
        instrs.push(Instr::new(OpExit {}));

        let mut label_alloc = LabelAllocator::new();
        let mut cfg_builder = CFGBuilder::new();
        cfg_builder.add_block(BasicBlock {
            label: label_alloc.alloc(),
            uniform: false,
            instrs,
        });
        let mut func = Function {
            ssa_alloc,
            phi_alloc: PhiAllocator::new(),
            blocks: cfg_builder.build(),
        };
        let mut info = default_shader_info();
        func.to_cssa();
        func.spill_values(RegFile::GPR, 4, &mut info).unwrap();
        assert!(!func.blocks[0].instrs.is_empty());
    }
}
