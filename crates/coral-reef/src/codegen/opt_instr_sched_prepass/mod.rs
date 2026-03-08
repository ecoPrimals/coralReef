// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Valve Corporation (2025)

#![allow(clippy::wildcard_imports)]

mod generate_order;
mod net_live;
mod schedule;

use generate_order::{GenerateOrder, ScheduleThresholds, calc_used_gprs, generate_dep_graph};
use schedule::*;

use super::debug::{DEBUG, GetDebugFlags};
use super::ir::*;
use super::liveness::{BlockLiveness, LiveSet, Liveness, SimpleLiveness};
use super::opt_instr_sched_common::{DepGraph, SideEffect, calc_statistics, side_effect_type};
use std::cmp::{max, min};

// This is the maximum number of reserved gprs - (DEBT(opt): Model more cases
// where we actually need 2)
const SW_RESERVED_GPRS: i32 = 1;
const SW_RESERVED_GPRS_SPILL: i32 = 2;

/// Target number of free GPRs. This is used for the threshold to switch to
/// scheduling for register pressure
const TARGET_FREE: i32 = 4;

/// Typically using an extra register is free... until you hit a threshold where
/// one more register causes occupancy to plummet. This function figures out how
/// many GPRs you can use without costing occupancy, assuming you always need at
/// least `x` GPRs.
fn next_occupancy_cliff(sm: &dyn ShaderModel, x: u32) -> u32 {
    let total_regs = sm.total_reg_file();
    let threads = max_warps_per_sm(sm, x) * sm.wave_size();

    // This function doesn't actually model the maximum number of registers
    // correctly - callers need to worry about that separately. We do,
    // however, want to avoid a divide by zero.
    let threads = max(threads, 1);

    prev_multiple_of(total_regs / threads, 8)
}

#[cfg(test)]
#[test]
fn test_next_occupancy_cliff() {
    for max_hw_warps in [32, 48, 64] {
        let sm = ShaderModelInfo::new(75, max_hw_warps);
        for x in 0..255 {
            let y = next_occupancy_cliff(&sm, x);
            assert!(y >= x);
            assert_eq!(max_warps_per_sm(&sm, x), max_warps_per_sm(&sm, y));
            assert!(max_warps_per_sm(&sm, y) > max_warps_per_sm(&sm, y + 1));
        }
    }
}

fn next_occupancy_cliff_with_reserved(sm: &dyn ShaderModel, gprs: i32, reserved: i32) -> i32 {
    i32::try_from(next_occupancy_cliff(
        sm,
        (gprs + reserved)
            .try_into()
            .expect("gprs+reserved must be non-negative for u32"),
    ))
    .expect("occupancy cliff must fit in i32")
        - reserved
}

impl Function {
    pub fn opt_instr_sched_prepass(
        &mut self,
        sm: &dyn ShaderModel,
        max_reg_count: PerRegFile<i32>,
    ) {
        let liveness = SimpleLiveness::for_function(self);
        let mut live_out_sets: Vec<LiveSet> = Vec::new();

        #[cfg(debug_assertions)]
        let orig_instr_counts: Vec<usize> = self.blocks.iter().map(|b| b.instrs.len()).collect();

        let reserved_gprs = SW_RESERVED_GPRS + (sm.hw_reserved_gpr_count() as i32);

        // First pass: Set up data structures and gather some statistics about
        // register pressure

        // lower and upper bounds for how many gprs we will use
        let mut min_gpr_target = 1;
        let mut max_gpr_target = 1;

        let mut schedule_units = Vec::new();

        for block_idx in 0..self.blocks.len() {
            let block_live = liveness.block_live(block_idx);
            let mut live_set = {
                let mut set = LiveSet::new();
                for &pred in self.blocks.pred_indices(block_idx) {
                    if pred < live_out_sets.len() {
                        for ssa in live_out_sets[pred].iter() {
                            if block_live.is_live_in(ssa) {
                                set.insert(*ssa);
                            }
                        }
                    }
                }
                set
            };

            let has_back_edge_pred = self
                .blocks
                .pred_indices(block_idx)
                .iter()
                .any(|&p| p >= block_idx);
            let block = &mut self.blocks[block_idx];
            let mut unit: ScheduleUnit = ScheduleUnit::default();
            unit.skip_schedule = has_back_edge_pred;

            for (ip, instr) in std::mem::take(&mut block.instrs).into_iter().enumerate() {
                let starts_block = match instr.op {
                    Op::PhiDsts(_) => true,
                    _ => false,
                };
                let ends_block = match instr.op {
                    Op::PhiSrcs(_) => true,
                    _ => instr.op.is_branch(),
                };

                // First use the live set before the instr
                if !starts_block && unit.live_in_count == None {
                    unit.live_in_count = Some(PerRegFile::new_with(|f| live_set.count(f)));
                }
                if ends_block {
                    unit.finish_block(&live_set);
                }

                // Update the live set
                let live_count = live_set.insert_instr_top_down(ip, &instr, block_live);

                // Now use the live set after the instruction
                {
                    let live_count = PerRegFile::new_with(|f| {
                        live_count[f]
                            .try_into()
                            .expect("live count must fit in i32")
                    });
                    let mut used_gprs = calc_used_gprs(live_count, max_reg_count);

                    if let Op::RegOut(reg_out) = &instr.op {
                        // This should be the last instruction.  Everything should
                        // be dead once we've processed it.
                        assert_eq!(live_set.count(RegFile::GPR), 0);
                        let gpr_output_count = reg_out
                            .srcs
                            .len()
                            .try_into()
                            .expect("RegOut count must fit in i32");
                        used_gprs = max(used_gprs, gpr_output_count);
                    }

                    // We never want our target to be worse than the original schedule
                    max_gpr_target = max(max_gpr_target, used_gprs);

                    if side_effect_type(&instr.op) == SideEffect::Barrier {
                        // If we can't reorder an instruction, then it forms a lower
                        // bound on how well we can do after rescheduling
                        min_gpr_target = max(min_gpr_target, used_gprs);
                    }

                    if !starts_block && !ends_block {
                        unit.peak_gpr_count = max(unit.peak_gpr_count, used_gprs);
                    }
                }

                match instr.op {
                    Op::PhiDsts(_) => {
                        unit.phi_dsts = Some(instr);
                    }
                    Op::PhiSrcs(_) => {
                        unit.phi_srcs = Some(instr);
                    }
                    _ => {
                        if instr.op.is_branch() {
                            unit.branch = Some(instr);
                        } else {
                            assert!(unit.live_out.is_none());
                            unit.instrs.push(instr);
                        }
                    }
                }
            }
            unit.finish_block(&live_set);
            schedule_units.push(unit);

            live_out_sets.push(live_set);
        }

        // Second pass: Generate a schedule for each schedule_unit
        let mut schedule_types = get_schedule_types(
            sm,
            max_reg_count,
            min_gpr_target,
            max_gpr_target,
            reserved_gprs,
        );
        schedule_types.reverse();

        for u in &mut schedule_units {
            if u.instrs.is_empty() || u.skip_schedule {
                continue;
            }
            loop {
                let schedule_type = *schedule_types
                    .last()
                    .expect("schedule_types must not be empty");
                let thresholds = schedule_type.thresholds(max_reg_count, u);

                u.schedule(sm, max_reg_count, schedule_type, thresholds);

                if u.has_new_order() {
                    break;
                }

                if schedule_types.len() > 1 {
                    schedule_types.pop();
                } else {
                    break;
                }
            }
        }

        // Third pass: Apply the generated schedules
        let schedule_type = schedule_types
            .into_iter()
            .last()
            .expect("schedule_types must not be empty");

        for (mut u, block) in schedule_units.into_iter().zip(self.blocks.iter_mut()) {
            if !u.instrs.is_empty() && u.last_tried_schedule_type != Some(schedule_type) {
                let thresholds = schedule_type.thresholds(max_reg_count, &u);
                u.schedule(sm, max_reg_count, schedule_type, thresholds);
            }

            block.instrs = u.to_instrs();
        }

        #[cfg(debug_assertions)]
        debug_assert_eq!(
            orig_instr_counts,
            self.blocks
                .iter()
                .map(|b| b.instrs.len())
                .collect::<Vec<usize>>()
        );

        if let ScheduleType::RegLimit(limit) = schedule_type {
            debug_assert!(
                {
                    let live = SimpleLiveness::for_function(self);
                    let max_live = live.calc_max_live(self);
                    max_live[RegFile::GPR]
                } <= limit.into()
            );
        }
    }
}

impl Shader<'_> {
    /// Pre-RA instruction scheduling
    ///
    /// We prioritize:
    /// 1. Occupancy
    /// 2. Preventing spills to memory
    /// 3. Instruction level parallelism
    ///
    /// We accomplish this by having an outer loop that tries different register
    /// limits in order of most to least occupancy. The inner loop computes
    /// actual schedules using a heuristic inspired by Goodman & Hsu 1988
    /// section 3, although the heuristic from that paper cannot be used
    /// directly here because they assume a single register file and we have
    /// multiple. Care is also taken to model quirks of register pressure on
    /// NVIDIA GPUs correctly.
    ///
    /// J. R. Goodman and W.-C. Hsu. 1988. Code scheduling and register
    ///     allocation in large basic blocks. In Proceedings of the 2nd
    ///     international conference on Supercomputing (ICS '88). Association
    ///     for Computing Machinery, New York, NY, USA, 442–452.
    ///     <https://doi.org/10.1145/55364.55407>
    pub fn opt_instr_sched_prepass(&mut self) {
        if DEBUG.annotate() {
            self.remove_annotations();
        }

        let mut max_reg_count = PerRegFile::<i32>::new_with(|f| {
            self.sm
                .reg_count(f)
                .try_into()
                .expect("reg_count must fit in i32")
        });
        if let ShaderStageInfo::Compute(cs_info) = &self.info.stage {
            max_reg_count[RegFile::GPR] = min(
                max_reg_count[RegFile::GPR],
                (gpr_limit_from_local_size(&cs_info.local_size) - self.sm.hw_reserved_gpr_count())
                    .try_into()
                    .expect("GPR limit must fit in i32"),
            );
        }
        max_reg_count[RegFile::GPR] -= SW_RESERVED_GPRS;

        for f in &mut self.functions {
            f.opt_instr_sched_prepass(self.sm, max_reg_count);
        }
    }
}
