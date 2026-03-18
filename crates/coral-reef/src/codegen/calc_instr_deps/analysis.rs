// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

use super::*;
use coral_reef_stubs::dataflow::{BackwardDataflow, ForwardDataflow};

/// Hardware has a FIFO queue of texture that are still fetching,
/// when the oldest tex finishes executing, it's written to the reg,
/// removed from the queue and it begins executing the new one.
/// The problem arises when a texture is read while it is still being fetched
/// to avoid it, we have a `texdepbar {i}` instruction that stalls until
/// the texture fetch queue has at most {i} elements.
/// e.g. the most simple solution is to have texdepbar 0 after each texture
/// instruction, but this would stall the pipeline until the texture fetch
/// finishes executing.
/// This algorithm inserts `texdepbar` at each use of the texture results,
/// simulating the texture queue execution.
///
/// Note that the texture queue has for each entry (texture data, register output)
/// and each register can be on the queue only once (we don't want to have multiple texture
/// operations in-flight that write to the same registers).
/// This can lead to a neat algorithm:
/// instead of tracking the queue directly, which can exponentially explode in complexity,
/// track the position of each register, which needs at most 255/63 positions.
/// For branches the state is duplicated in each basic block,
/// for joins instead we want to keep both the minimum position of each
/// entry and the maximum length og the queue to avoid overflows.
///
/// Note: If this pass is too slow, there are still optimizations left:
/// - Our data-flow computes barrier levels and discards them,
///   but since most CFG blocks do not need recomputation, we could save
///   the barrier levels in a vec and save a pass later.
/// - Instead of pushing by 1 each element in the queue on a `push` op,
///   we could keep track of an in-flight range and use a wrapping timestamp
///   this improves performance but needs careful implementation to avoid bugs
pub(super) fn insert_texture_barriers(f: &mut Function, sm: &dyn ShaderModel) {
    assert!(sm.is_kepler()); // Only kepler has texture barriers!

    let mut state_in: Vec<_> = (0..f.blocks.len())
        .map(|_| TexQueueSimulationState::new())
        .collect();
    let mut state_out: Vec<_> = (0..f.blocks.len())
        .map(|_| TexQueueSimulationState::new())
        .collect();
    ForwardDataflow {
        cfg: &f.blocks,
        block_in: &mut state_in[..],
        block_out: &mut state_out[..],
        transfer: |_block_idx,
                   block: &BasicBlock,
                   sim_out: &mut TexQueueSimulationState,
                   sim_in: &TexQueueSimulationState| {
            let mut sim = sim_in.clone();

            for instr in &block.instrs {
                // Ignore the barrier, we will recompute this later
                let _bar = sim.visit_instr(instr);
            }

            if *sim_out == sim {
                false
            } else {
                *sim_out = sim;
                true
            }
        },
        join: |sim_in: &mut TexQueueSimulationState, pred_sim_out: &TexQueueSimulationState| {
            sim_in.merge(pred_sim_out);
        },
    }
    .solve();

    for (block, mut sim) in f.blocks.iter_mut().zip(state_in.into_iter()) {
        block.map_instrs(|instr| {
            if let Some(textures_left) = sim.visit_instr(&instr) {
                let bar = Instr::new(OpTexDepBar { textures_left });
                MappedInstrs::Many(vec![bar, instr])
            } else {
                MappedInstrs::One(instr)
            }
        });
    }
}

pub(super) fn assign_barriers(f: &mut Function, sm: &dyn ShaderModel) {
    let mut uses = Box::new(RegTracker::new_with(&|| RegUse::None));
    let mut deps = DepGraph::new();

    for (bi, b) in f.blocks.iter().enumerate() {
        for (ip, instr) in b.instrs.iter().enumerate() {
            if instr.is_branch() {
                deps.add_barrier(bi, ip);
            } else {
                // Execution predicates are handled immediately and we don't
                // need barriers for them, regardless of whether or not it's a
                // fixed-latency instruction.
                let mut waits = Vec::new();
                uses.for_each_instr_pred_mut(instr, |u| {
                    let u = u.clear_write();
                    waits.extend_from_slice(u.deps());
                });

                if sm.op_needs_scoreboard(&instr.op) {
                    let (rd, wr) = deps.add_instr(bi, ip);
                    uses.for_each_instr_src_mut(instr, |_, u| {
                        // Only mark a dep as signaled if we actually have
                        // something that shows up in the register file as
                        // needing scoreboarding
                        deps.add_signal(rd);
                        let u = u.add_read(rd);
                        waits.extend_from_slice(u.deps());
                    });
                    uses.for_each_instr_dst_mut(instr, |_, u| {
                        // Only mark a dep as signaled if we actually have
                        // something that shows up in the register file as
                        // needing scoreboarding
                        deps.add_signal(wr);
                        let u = u.set_write(wr);
                        for dep in u.deps() {
                            // Don't wait on ourselves
                            if *dep != rd {
                                waits.push(*dep);
                            }
                        }
                    });
                } else {
                    // Delays will cover us here.  We just need to make sure
                    // that we wait on any uses that we consume.
                    uses.for_each_instr_src_mut(instr, |_, u| {
                        let u = u.clear_write();
                        waits.extend_from_slice(u.deps());
                    });
                    uses.for_each_instr_dst_mut(instr, |_, u| {
                        let u = u.clear();
                        waits.extend_from_slice(u.deps());
                    });
                }
                deps.add_waits(bi, ip, waits);
            }
        }
    }

    let mut bars = BarAlloc::new();

    for (bi, b) in f.blocks.iter_mut().enumerate() {
        for (ip, instr) in b.instrs.iter_mut().enumerate() {
            let mut wait_mask = 0_u8;
            for dep in deps.get_instr_waits(bi, ip) {
                if let Some(bar) = bars.get_bar_for_dep(*dep) {
                    wait_mask |= 1 << bar;
                    bars.free_bar(bar);
                }
            }
            instr.deps.add_wt_bar_mask(wait_mask);

            if instr.needs_yield() {
                instr.deps.set_yield(true);
            }

            if !sm.op_needs_scoreboard(&instr.op) {
                continue;
            }

            let (rd_dep, wr_dep) = deps.get_instr_deps(bi, ip);
            if deps.dep_is_waited_after(rd_dep, bi, ip) {
                let rd_bar = bars.try_find_free_bar().unwrap_or_else(|| {
                    let bar = bars.free_some_bar();
                    instr.deps.add_wt_bar(bar);
                    bar
                });
                bars.set_bar_dep(rd_bar, rd_dep);
                instr.deps.set_rd_bar(rd_bar);
            }
            if deps.dep_is_waited_after(wr_dep, bi, ip) {
                let wr_bar = bars.try_find_free_bar().unwrap_or_else(|| {
                    let bar = bars.free_some_bar();
                    instr.deps.add_wt_bar(bar);
                    bar
                });
                bars.set_bar_dep(wr_bar, wr_dep);
                instr.deps.set_wr_bar(wr_bar);
            }
        }
    }
}

pub(super) fn calc_delays(f: &mut Function, sm: &dyn ShaderModel) -> u64 {
    let mut instr_cycles: Vec<Vec<u32>> =
        f.blocks.iter().map(|b| vec![0; b.instrs.len()]).collect();

    let mut state_in: Vec<_> = vec![DelayRegTracker::default(); f.blocks.len()];
    let mut state_out: Vec<_> = vec![DelayRegTracker::default(); f.blocks.len()];

    let latency_upper_bound: u8 = sm
        .latency_upper_bound()
        .try_into()
        .expect("Latency upper bound too large!");

    // Compute instruction delays using an optimistic backwards data-flow
    // algorithm.  For back-cycles we assume the best and recompute when
    // new data is available.  This is yields correct results as long as
    // the data flow analysis is run until completion.
    BackwardDataflow {
        cfg: &f.blocks,
        block_in: &mut state_in[..],
        block_out: &mut state_out[..],
        transfer: |block_idx,
                   block: &BasicBlock,
                   reg_in: &mut DelayRegTracker,
                   reg_out: &DelayRegTracker| {
            let mut uses = reg_out.clone();

            let mut sched = BlockDelayScheduler {
                sm,
                f,
                // Barriers are handled by `assign_barriers`, and it does
                // not handle cross-block barrier signal/wait.
                // We can safely assume that no barrier is active at the
                // start and end of the block
                bars: [0_u32; 6],
                current_cycle: 0_u32,
                instr_cycles: &mut instr_cycles,
            };

            for ip in (0..block.instrs.len()).rev() {
                let loc = InstrIdx::new(block_idx, ip);
                sched.process_instr(loc, &mut uses);
            }

            // Update accumulated delay
            let block_cycles = sched.current_cycle;
            uses.retain(|reg_use| {
                reg_use.retain(|(_rw, k), v| {
                    let overcount = if k.loc.block_idx as usize == block_idx {
                        // Only instrs before instr_idx must be counted
                        instr_cycles[k.loc.block_idx as usize][k.loc.instr_idx as usize]
                    } else {
                        0
                    };
                    let instr_executed = (block_cycles - overcount).try_into().unwrap_or(u8::MAX);
                    // We only care about the accumulated delay until it
                    // is bigger than the maximum delay of an instruction.
                    // after that, it cannot cause hazards.
                    let (added, overflow) = (*v).overflowing_add(instr_executed);
                    *v = added;
                    // Stop keeping track of entries that happened too
                    // many cycles "in the future", and cannot affect
                    // scheduling anymore
                    !overflow && added <= latency_upper_bound
                });
                !reg_use.is_empty()
            });

            if *reg_in == uses {
                false
            } else {
                *reg_in = uses;
                true
            }
        },
        join: |curr_in: &mut DelayRegTracker, succ_out: &DelayRegTracker| {
            // We start with an optimistic assumption and gradually make it
            // less optimistic.  So in the join operation we need to keep
            // the "worst" accumulated latency, that is the lowest one.
            // i.e. if an instruction has an accumulated latency of 2 cycles,
            // it can interfere with the next block, while if it had 200 cycles
            // it's highly unlikely that it could interfere.
            curr_in.merge_with(succ_out, |a, b| a.merge_with(b, |ai, bi| (*ai).min(*bi)));
        },
    }
    .solve();

    // Update the deps.delay for each instruction and compute
    for (bi, b) in f.blocks.iter_mut().enumerate() {
        let cycles = &instr_cycles[bi];
        for (ip, i) in b.instrs.iter_mut().enumerate() {
            let delay = cycles[ip] - cycles.get(ip + 1).copied().unwrap_or(0);
            let delay: u8 = delay.try_into().expect("Delay overflow");
            i.deps.delay = delay.max(MIN_INSTR_DELAY);
        }
    }

    let min_num_static_cycles = instr_cycles
        .iter()
        .enumerate()
        .map(|(block_idx, cycles)| {
            let cycles = cycles.last().copied().unwrap_or(0);
            let block_weight = estimate_block_weight(&f.blocks, block_idx);
            u64::from(cycles)
                .checked_mul(block_weight)
                .expect("Cycle count estimate overflow")
        })
        .reduce(|a, b| a.checked_add(b).expect("Cycle count estimate overflow"))
        .unwrap_or(0);

    let max_instr_delay = sm.max_instr_delay();
    f.map_instrs(|mut instr, _| {
        if instr.deps.delay > max_instr_delay {
            let mut delay = instr.deps.delay - max_instr_delay;
            instr.deps.set_delay(max_instr_delay);
            let mut instrs = vec![instr];
            while delay > 0 {
                let mut nop = Instr::new(OpNop { label: None });
                nop.deps.set_delay(delay.min(max_instr_delay));
                delay -= nop.deps.delay;
                instrs.push(nop);
            }
            MappedInstrs::Many(instrs)
        } else if matches!(instr.op, Op::SrcBar(_)) {
            instr.op = Op::Nop(OpNop { label: None });
            MappedInstrs::One(instr)
        } else if sm.exec_latency(&instr.op) > 1 {
            // It's unclear exactly why but the blob inserts a Nop with a delay
            // of 2 after every instruction which has an exec latency.  Perhaps
            // it has something to do with .yld?  In any case, the extra 2
            // cycles aren't worth the chance of weird bugs.
            let mut nop = Instr::new(OpNop { label: None });
            nop.deps.set_delay(2);
            MappedInstrs::Many(vec![instr, nop])
        } else {
            MappedInstrs::One(instr)
        }
    });

    min_num_static_cycles
}
