// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT

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
                        if !instr.pred.pred_ref.is_none() || instr.srcs().len() > 0 {
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
                assert!(self.info.num_static_cycles >= min_num_static_cycles);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
