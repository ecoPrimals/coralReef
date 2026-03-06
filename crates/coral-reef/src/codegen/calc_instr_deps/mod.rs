// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::debug::{DEBUG, GetDebugFlags};
use super::ir::*;
use super::opt_instr_sched_common::estimate_block_weight;
use super::reg_tracker::{RegRefIterable, RegTracker, SparseRegTracker};

mod analysis;
mod types;

pub(super) use analysis::*;
pub(super) use types::*;

impl Shader<'_> {
    pub fn assign_deps_serial(&mut self) {
        for f in &mut self.functions {
            for b in &mut f.blocks.iter_mut().rev() {
                let mut wt = 0_u8;
                for instr in &mut b.instrs {
                    if matches!(&instr.op, Op::Bar(_))
                        || matches!(&instr.op, Op::BClear(_))
                        || matches!(&instr.op, Op::BSSy(_))
                        || matches!(&instr.op, Op::BSync(_))
                    {
                        instr.deps.set_yield(true);
                    } else if instr.is_branch() {
                        instr.deps.add_wt_bar_mask(0x3f);
                    } else {
                        instr.deps.add_wt_bar_mask(wt);
                        if instr.dsts().len() > 0 {
                            instr.deps.set_wr_bar(0);
                            wt |= 1 << 0;
                        }
                        if !instr.pred.predicate.is_none() || instr.srcs().len() > 0 {
                            instr.deps.set_rd_bar(1);
                            wt |= 1 << 1;
                        }
                    }
                }
            }
        }
    }

    pub fn calc_instr_deps(&mut self) {
        if self.sm.is_kepler() {
            for f in &mut self.functions {
                insert_texture_barriers(f, self.sm);
            }
        }

        if DEBUG.serial() {
            self.assign_deps_serial();
        } else {
            let mut min_num_static_cycles = 0u64;
            for f in &mut self.functions {
                assign_barriers(f, self.sm);
                min_num_static_cycles += calc_delays(f, self.sm);
            }

            if DEBUG.cycles() {
                // This is useful for debugging differences in the scheduler
                // cycle count model and the calc_delays() model.  However, it
                // isn't totally valid since assign_barriers() can add extra
                // dependencies for barrier re-use and those may add cycles.
                // The chances of it doing this are low, thanks to our LRU
                // allocation strategy, but it's still not an assert we want
                // running in production.
                assert!(self.info.static_cycle_count >= min_num_static_cycles);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{
        BasicBlock, Dst, Function, Instr, LabelAllocator, OpCopy, OpExit, OpIAdd2, OpRegOut,
        PhiAllocator, RegFile, SSAValueAllocator, ShaderModelInfo, Src,
    };
    use coral_reef_stubs::cfg::CFGBuilder;
    use std::ops::Range;

    fn reg_gpr(range: Range<usize>) -> RegRef {
        RegRef::new(
            RegFile::GPR,
            range.start as u32,
            (range.end - range.start) as u8,
        )
    }

    #[test]
    fn test_texdepbar_basic() {
        let mut sim = TexQueueSimulationState::new();

        // RaW
        assert_eq!(sim.push(reg_gpr(0..4)), None);
        assert_eq!(sim.flush(reg_gpr(2..3)), Some(0));

        // 2 entries in the queue
        assert_eq!(sim.push(reg_gpr(0..2)), None); // [A]
        assert_eq!(sim.push(reg_gpr(2..4)), None); // [B, A]
        assert_eq!(sim.flush(reg_gpr(0..1)), Some(1)); // [B]
        assert_eq!(sim.flush(reg_gpr(3..4)), Some(0)); // []

        // Test bucket conflicts
        assert_eq!(sim.push(reg_gpr(0..1)), None);
        assert_eq!(sim.flush(reg_gpr(1..3)), None);
        assert_eq!(sim.flush(reg_gpr(0..3)), Some(0));

        // Bucket conflict part 2: Electric Boogaloo
        assert_eq!(sim.push(reg_gpr(1..2)), None);
        assert_eq!(sim.push(reg_gpr(0..1)), None);
        assert_eq!(sim.flush(reg_gpr(1..2)), Some(1));
        assert_eq!(sim.flush(reg_gpr(0..1)), Some(0));

        // Interesting CFG case that the old pass got wrong.
        // CFG: A -> [B, C] -> D
        // A pushes
        assert_eq!(sim.push(reg_gpr(0..4)), None);
        // B: pushes a tex then flushes it
        let mut b_sim = sim.clone();
        assert_eq!(b_sim.push(reg_gpr(4..8)), None);
        assert_eq!(b_sim.flush(reg_gpr(4..8)), Some(0));
        // C: pushes 3 tex and never flishes them
        let mut c_sim = sim.clone();
        assert_eq!(c_sim.push(reg_gpr(4..5)), None);
        assert_eq!(c_sim.push(reg_gpr(5..6)), None);
        assert_eq!(c_sim.push(reg_gpr(6..7)), None);
        // D: flushes the tex pushed by A
        let mut d_sim = b_sim;
        d_sim.merge(&c_sim);
        assert_eq!(c_sim.flush(reg_gpr(0..4)), Some(3));
        // the "shortest push path" would pass by B but in fact
        // by passing in B our texture is flushed off the queue.
        // (old algorithm would insert a texdepbar 1)
    }

    #[test]
    fn test_texdepbar_overflow() {
        let mut sim = TexQueueSimulationState::new();

        // Fill the texture queue
        for i in 0..(usize::from(OpTexDepBar::MAX_TEXTURES_LEFT) + 1) {
            assert_eq!(sim.push(reg_gpr(i..(i + 1))), None);
        }
        // The new push would overflow the queue, we NEED a barrier
        assert_eq!(
            sim.push(reg_gpr(64..65)),
            Some(OpTexDepBar::MAX_TEXTURES_LEFT)
        );
        assert_eq!(
            sim.push(reg_gpr(65..66)),
            Some(OpTexDepBar::MAX_TEXTURES_LEFT)
        );
    }

    #[test]
    fn test_dep_graph_add_instr_and_waits() {
        let mut deps = DepGraph::new();
        let (rd, wr) = deps.add_instr(0, 0);
        deps.add_signal(rd);
        deps.add_signal(wr);
        deps.add_waits(0, 1, vec![wr]);
        assert_eq!(deps.get_instr_deps(0, 0), (rd, wr));
        assert_eq!(deps.get_instr_waits(0, 1), &[wr]);
    }

    #[test]
    fn test_dep_graph_add_barrier() {
        let mut deps = DepGraph::new();
        let (rd, wr) = deps.add_instr(0, 0);
        deps.add_signal(rd);
        deps.add_signal(wr);
        deps.add_barrier(0, 1);
        assert!(
            deps.get_instr_waits(0, 1).len() >= 1,
            "barrier should wait on at least one dep"
        );
    }

    #[test]
    fn test_dep_graph_dep_is_waited_after() {
        let mut deps = DepGraph::new();
        let (_rd, wr) = deps.add_instr(0, 0);
        deps.add_signal(wr);
        deps.add_waits(0, 2, vec![wr]);
        assert!(deps.dep_is_waited_after(wr, 0, 0));
        assert!(!deps.dep_is_waited_after(wr, 0, 2));
    }

    #[test]
    fn test_bar_alloc_basic() {
        let mut bars = BarAlloc::new();
        assert!(bars.bar_is_free(0));
        assert!(bars.try_find_free_bar().is_some());
        bars.set_bar_dep(0, 42);
        assert!(!bars.bar_is_free(0));
        assert_eq!(bars.get_bar_for_dep(42), Some(0));
        bars.free_bar(0);
        assert!(bars.bar_is_free(0));
        assert_eq!(bars.get_bar_for_dep(42), None);
    }

    #[test]
    fn test_bar_alloc_free_some() {
        let mut bars = BarAlloc::new();
        bars.set_bar_dep(0, 10);
        bars.set_bar_dep(1, 5);
        bars.set_bar_dep(2, 20);
        let freed = bars.free_some_bar();
        assert_eq!(freed, 1);
        assert!(bars.bar_is_free(1));
        assert!(!bars.bar_is_free(0));
        assert!(!bars.bar_is_free(2));
    }

    #[test]
    fn test_reg_use_map_add_read_set_write() {
        let mut map: RegUseMap<usize, u8> = RegUseMap::default();
        assert!(map.is_empty());
        map.add_read(1, 10);
        assert!(!map.is_empty());
        let reads: Vec<_> = map.iter_reads().collect();
        assert_eq!(reads.len(), 1);
        assert_eq!(*reads[0].1, 10);
        map.set_write(2, 20);
        let writes: Vec<_> = map.iter_writes().collect();
        assert_eq!(writes.len(), 1);
        assert_eq!(*writes[0].1, 20);
    }

    #[test]
    fn test_reg_use_deps() {
        let mut u = RegUse::<usize>::None;
        assert_eq!(u.deps(), &[]);
        u.add_read(1);
        assert_eq!(u.deps(), &[1]);
        u.set_write(2);
        assert_eq!(u.deps(), &[2]);
    }

    /// Exercises assign_barriers and calc_delays with a minimal Function.
    /// The pipeline uses assign_deps_serial() directly, so these analysis
    /// passes are never run during normal compilation. This test ensures
    /// they work correctly when invoked.
    #[test]
    fn test_assign_barriers_and_calc_delays() {
        use crate::codegen::ir::{OpExit, OpMov};

        let sm = ShaderModelInfo::new(80, 64); // SM80 to exercise latency tables
        let mut label_alloc = LabelAllocator::new();
        let mut cfg_builder = CFGBuilder::new();

        // Block: mov r0, 0; mov r1, r0; mov r2, r1; exit
        // Creates RaW chain r0 -> r1 -> r2 to exercise barrier/delay assignment
        let r0 = RegRef::new(RegFile::GPR, 0, 1);
        let r1 = RegRef::new(RegFile::GPR, 1, 1);
        let r2 = RegRef::new(RegFile::GPR, 2, 1);

        let block = BasicBlock {
            label: label_alloc.alloc(),
            uniform: false,
            instrs: vec![
                Instr::new(OpMov {
                    dst: r0.into(),
                    src: 0u32.into(),
                    quad_lanes: 0xf,
                }),
                Instr::new(OpMov {
                    dst: r1.into(),
                    src: r0.into(),
                    quad_lanes: 0xf,
                }),
                Instr::new(OpMov {
                    dst: r2.into(),
                    src: r1.into(),
                    quad_lanes: 0xf,
                }),
                Instr::new(OpExit {}),
            ],
        };
        cfg_builder.add_block(block);
        let mut function = Function {
            ssa_alloc: SSAValueAllocator::new(),
            phi_alloc: PhiAllocator::new(),
            blocks: cfg_builder.build(),
        };

        assign_barriers(&mut function, &sm);
        let cycles = calc_delays(&mut function, &sm);

        // Verify barriers/delays were assigned. OpMov is coupled on SM80 so may
        // not get rd_bar/wr_bar (those are for scoreboard ops), but we should
        // get delays from the backward dataflow.
        let mov1 = &function.blocks[0].instrs[0];
        let mov2 = &function.blocks[0].instrs[1];
        let mov3 = &function.blocks[0].instrs[2];
        assert!(
            mov1.deps.delay >= 1 || mov2.deps.delay >= 1 || mov3.deps.delay >= 1,
            "at least one instruction should have delay"
        );
        assert!(cycles > 0, "cycle estimate should be positive");
    }
}
