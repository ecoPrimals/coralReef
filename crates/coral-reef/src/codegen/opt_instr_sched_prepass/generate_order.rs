// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Valve Corporation (2025)

use super::net_live::NetLive;
use crate::codegen::ir::*;
use crate::codegen::liveness::LiveSet;
use crate::codegen::opt_instr_sched_common::{
    DepGraph, EdgeLabel, FutureReadyInstr, NodeLabel, ReadyInstr, SideEffect,
    estimate_variable_latency, side_effect_type,
};
use coral_reef_stubs::fxhash::FxHashMap;
use std::cmp::{Reverse, max};
use std::collections::BTreeSet;

pub(super) fn generate_dep_graph(sm: &dyn ShaderModel, instrs: &[Instr]) -> DepGraph {
    let mut g = DepGraph::new((0..instrs.len()).map(|_| NodeLabel::default()));

    let mut defs = FxHashMap::<SSAValue, (usize, usize)>::default();

    let mut last_memory_op = None;
    let mut last_barrier_op = None;

    for ip in 0..instrs.len() {
        let instr = &instrs[ip];

        if let Some(bar_ip) = last_barrier_op {
            g.add_edge(bar_ip, ip, EdgeLabel { latency: 0 });
        }

        match side_effect_type(&instr.op) {
            SideEffect::None => (),
            SideEffect::Barrier => {
                let first_ip = last_barrier_op.unwrap_or(0);
                for other_ip in first_ip..ip {
                    g.add_edge(other_ip, ip, EdgeLabel { latency: 0 });
                }
                last_barrier_op = Some(ip);
            }
            SideEffect::Memory => {
                if let Some(mem_ip) = last_memory_op {
                    g.add_edge(mem_ip, ip, EdgeLabel { latency: 0 });
                }
                last_memory_op = Some(ip);
            }
        }

        for (i, src) in instr.srcs().iter().enumerate() {
            for ssa in src.reference.iter_ssa() {
                if let Some(&(def_ip, def_idx)) = defs.get(ssa) {
                    let def_instr = &instrs[def_ip];
                    let latency = if def_instr.op.is_virtual() || instr.op.is_virtual() {
                        0
                    } else {
                        max(
                            sm.raw_latency(&def_instr.op, def_idx, &instr.op, i),
                            estimate_variable_latency(sm, &def_instr.op),
                        )
                    };

                    g.add_edge(def_ip, ip, EdgeLabel { latency });
                }
            }
        }

        if let PredRef::SSA(ssa) = &instr.pred.predicate {
            if let Some(&(def_ip, def_idx)) = defs.get(ssa) {
                let def_instr = &instrs[def_ip];

                let latency = if def_instr.op.is_virtual() {
                    0
                } else {
                    max(
                        sm.paw_latency(&def_instr.op, def_idx),
                        estimate_variable_latency(sm, &def_instr.op),
                    )
                };

                g.add_edge(def_ip, ip, EdgeLabel { latency });
            }
        }

        for (i, dst) in instr.dsts().iter().enumerate() {
            for &ssa in dst.iter_ssa() {
                defs.insert(ssa, (ip, i));
            }
        }
    }

    g
}

/// The third element of each tuple is a weight meant to approximate the cost of
/// spilling a value from the first register file to the second. Right now, the
/// values are meant to approximate the cost of a spill + fill, in cycles
const SPILL_FILES: [(RegFile, RegFile, i32); 5] = [
    (RegFile::Bar, RegFile::GPR, 6 + 6),
    (RegFile::Pred, RegFile::GPR, 12 + 6),
    (RegFile::UPred, RegFile::UGPR, 12 + 6),
    (RegFile::UGPR, RegFile::GPR, 15 + 6),
    (RegFile::GPR, RegFile::Mem, 32 + 32),
];

/// Models how many gprs will be used after spilling other register files
pub(super) fn calc_used_gprs(mut p: PerRegFile<i32>, max_reg_count: PerRegFile<i32>) -> i32 {
    for (src, dest, _) in SPILL_FILES {
        if p[src] > max_reg_count[src] {
            p[dest] += p[src] - max_reg_count[src];
        }
    }

    p[RegFile::GPR]
}

fn calc_score_part(mut p: PerRegFile<i32>, max_reg_count: PerRegFile<i32>) -> (i32, i32) {
    // We separate "badness" and "goodness" because we don't want eg. two extra
    // free predicates to offset the weight of spilling a UGPR - the spill is
    // always more important than keeping extra registers free
    let mut badness: i32 = 0;
    let mut goodness: i32 = 0;

    for (src, dest, weight) in SPILL_FILES {
        if p[src] > max_reg_count[src] {
            let spill_count = p[src] - max_reg_count[src];
            p[dest] += spill_count;
            badness += spill_count * weight;
        } else {
            let free_count = max_reg_count[src] - p[src];
            goodness += free_count * weight;
        }
    }
    (badness, goodness)
}

type Score = (bool, Reverse<i32>, i32);

fn calc_score(
    net: PerRegFile<i32>,
    peak1: PerRegFile<i32>,
    peak2: PerRegFile<i32>,
    max_reg_count: PerRegFile<i32>,
    delay_cycles: u32,
    thresholds: ScheduleThresholds,
) -> Score {
    let peak_gprs = max(
        calc_used_gprs(peak1, max_reg_count),
        calc_used_gprs(peak2, max_reg_count),
    );
    let instruction_usable = peak_gprs <= thresholds.quit_threshold;
    if !instruction_usable {
        return (false, Reverse(0), 0);
    }

    let (mut badness, goodness) = calc_score_part(net, max_reg_count);
    badness += i32::try_from(delay_cycles).expect("delay_cycles must fit in i32");

    (true, Reverse(badness), goodness)
}

#[derive(Copy, Clone)]
pub(super) struct ScheduleThresholds {
    /// Start scheduling for pressure if we use this many gprs
    pub heuristic_threshold: i32,

    /// Give up if we use this many gprs
    pub quit_threshold: i32,
}

pub(super) struct GenerateOrder<'a> {
    max_reg_count: PerRegFile<i32>,
    net_live: NetLive,
    live: LiveSet,
    instrs: &'a [Instr],
}

impl<'a> GenerateOrder<'a> {
    pub(super) fn new(
        max_reg_count: PerRegFile<i32>,
        instrs: &'a [Instr],
        live_out: &LiveSet,
    ) -> Self {
        let net_live = NetLive::new(instrs, live_out);
        let live: LiveSet = live_out.clone();

        GenerateOrder {
            max_reg_count,
            net_live,
            live,
            instrs,
        }
    }

    fn new_used_regs(&self, net: PerRegFile<i8>) -> PerRegFile<i32> {
        PerRegFile::new_with(|file| {
            i32::try_from(self.live.count(file)).expect("live count must fit in i32")
                + (net[file] as i32)
        })
    }

    pub(super) fn current_used_gprs(&self) -> i32 {
        calc_used_gprs(
            PerRegFile::new_with(|f| {
                self.live
                    .count(f)
                    .try_into()
                    .expect("live count must fit in i32")
            }),
            self.max_reg_count,
        )
    }

    fn new_used_gprs_net(&self, instr_index: usize) -> i32 {
        calc_used_gprs(
            self.new_used_regs(self.net_live[instr_index].net),
            self.max_reg_count,
        )
    }

    fn new_used_gprs_peak1(&self, instr_index: usize) -> i32 {
        calc_used_gprs(
            self.new_used_regs(self.net_live[instr_index].peak1),
            self.max_reg_count,
        )
    }

    fn new_used_gprs_peak2(&self, instr_index: usize) -> i32 {
        calc_used_gprs(
            self.new_used_regs(self.net_live[instr_index].peak2),
            self.max_reg_count,
        )
    }

    fn new_score(
        &self,
        instr_index: usize,
        delay_cycles: u32,
        thresholds: ScheduleThresholds,
    ) -> Score {
        calc_score(
            self.new_used_regs(self.net_live[instr_index].net),
            self.new_used_regs(self.net_live[instr_index].peak1),
            self.new_used_regs(self.net_live[instr_index].peak2),
            self.max_reg_count,
            delay_cycles,
            thresholds,
        )
    }

    pub(super) fn generate_order(
        mut self,
        g: &DepGraph,
        init_ready_list: &[usize],
        thresholds: ScheduleThresholds,
    ) -> Option<(Vec<usize>, PerRegFile<i32>)> {
        let mut ready_instrs: BTreeSet<ReadyInstr> = init_ready_list
            .iter()
            .map(|&i| ReadyInstr::new(g, i))
            .collect();
        let mut future_ready_instrs = BTreeSet::new();

        struct InstrInfo {
            use_count: u32,
            ready_cycle: u32,
        }
        let mut instr_info: Vec<InstrInfo> = g
            .nodes
            .iter()
            .map(|node| InstrInfo {
                use_count: node.label.use_count,
                ready_cycle: node.label.ready_cycle,
            })
            .collect();

        let mut current_cycle = 0;
        let mut instr_order = Vec::with_capacity(g.nodes.len());
        loop {
            let used_gprs = self.current_used_gprs();

            loop {
                match future_ready_instrs.last() {
                    None => break,
                    Some(FutureReadyInstr {
                        ready_cycle: std::cmp::Reverse(ready_cycle),
                        index,
                    }) => {
                        if current_cycle >= *ready_cycle {
                            ready_instrs.insert(ReadyInstr::new(g, *index));
                            future_ready_instrs.pop_last();
                        } else {
                            break;
                        }
                    }
                }
            }

            if ready_instrs.is_empty() {
                match future_ready_instrs.last() {
                    None => break,
                    Some(&FutureReadyInstr {
                        ready_cycle: Reverse(ready_cycle),
                        ..
                    }) => {
                        assert!(ready_cycle > current_cycle);
                        current_cycle = ready_cycle;
                        continue;
                    }
                }
            }

            let next_idx = if used_gprs <= thresholds.heuristic_threshold {
                let ReadyInstr { index, .. } = ready_instrs
                    .pop_last()
                    .expect("ready_instrs must not be empty when used");
                index
            } else {
                let (new_score, ready_instr) = ready_instrs
                    .iter()
                    .map(|ready_instr| {
                        (
                            self.new_score(ready_instr.index, 0, thresholds),
                            ready_instr.clone(),
                        )
                    })
                    .max()
                    .expect("ready_instrs must not be empty when scheduling for pressure");

                let better_candidate = future_ready_instrs
                    .iter()
                    .filter_map(|future_ready_instr| {
                        let ready_cycle = future_ready_instr.ready_cycle.0;
                        let s = self.new_score(
                            future_ready_instr.index,
                            ready_cycle - current_cycle,
                            thresholds,
                        );
                        if s > new_score {
                            Some((s, future_ready_instr.clone()))
                        } else {
                            None
                        }
                    })
                    .max();

                if let Some((_, future_ready_instr)) = better_candidate {
                    future_ready_instrs.remove(&future_ready_instr);
                    let ready_cycle = future_ready_instr.ready_cycle.0;
                    assert!(ready_cycle > current_cycle);
                    current_cycle = ready_cycle;
                    future_ready_instr.index
                } else {
                    ready_instrs.remove(&ready_instr);
                    ready_instr.index
                }
            };

            let predicted_new_used_gprs_peak = max(
                self.new_used_gprs_peak1(next_idx),
                self.new_used_gprs_peak2(next_idx),
            );
            let predicted_new_used_gprs_net = self.new_used_gprs_net(next_idx);

            if predicted_new_used_gprs_peak > thresholds.quit_threshold {
                return None;
            }

            let outgoing_edges = &g.nodes[next_idx].outgoing_edges;
            for edge in outgoing_edges {
                let dep_instr = &mut instr_info[edge.head_idx];
                dep_instr.ready_cycle =
                    max(dep_instr.ready_cycle, current_cycle + edge.label.latency);
                dep_instr.use_count -= 1;
                if dep_instr.use_count <= 0 {
                    future_ready_instrs.insert(FutureReadyInstr::new(g, edge.head_idx));
                }
            }

            let instr = &self.instrs[next_idx];
            for dst in instr.dsts() {
                for ssa in dst.iter_ssa() {
                    self.live.remove(ssa);
                }
            }

            for &ssa in instr.ssa_uses() {
                if self.net_live.remove(ssa) {
                    self.live.insert(ssa);
                } else {
                    debug_assert!(!self.live.insert(ssa));
                }
            }

            instr_order.push(next_idx);
            current_cycle += 1;

            debug_assert_eq!(self.current_used_gprs(), predicted_new_used_gprs_net);
        }

        Some((
            instr_order,
            PerRegFile::new_with(|f| {
                self.live
                    .count(f)
                    .try_into()
                    .expect("live count must fit in i32")
            }),
        ))
    }
}
