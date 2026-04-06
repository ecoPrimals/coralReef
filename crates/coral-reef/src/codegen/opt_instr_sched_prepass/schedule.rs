// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Valve Corporation (2025)

use super::*;

struct InstructionOrder {
    order: Vec<usize>,
}

impl InstructionOrder {
    fn apply<'a>(&'a self, instrs: Vec<Instr>) -> impl 'a + Iterator<Item = Instr> {
        assert_eq!(self.order.len(), instrs.len());

        let mut instrs: Vec<Option<Instr>> = instrs.into_iter().map(|instr| Some(instr)).collect();

        self.order
            .iter()
            .map(move |&i| std::mem::take(&mut instrs[i]).expect("Instruction scheduled twice"))
    }
}

fn sched_buffer(
    max_reg_count: PerRegFile<i32>,
    instrs: &[Instr],
    graph: &ScheduleUnitGraph,
    _live_in_count: PerRegFile<u32>,
    live_out: &LiveSet,
    thresholds: ScheduleThresholds,
) -> Option<InstructionOrder> {
    let (mut new_order, _live_in_count_backward) = GenerateOrder::new(
        max_reg_count,
        instrs,
        live_out,
    )
    .generate_order(&graph.g, &graph.init_ready_list, thresholds)?;

    #[cfg(debug_assertions)]
    {
        let expected = PerRegFile::new_with(|f| {
            _live_in_count[f]
                .try_into()
                .expect("live_in count must fit in i32")
        });
        debug_assert_eq!(
            _live_in_count_backward, expected,
            "opt_instr_sched_prepass: backward live-in count must match forward accounting"
        );
    }

    new_order.reverse();

    Some(InstructionOrder { order: new_order })
}

struct ScheduleUnitGraph {
    g: DepGraph,
    init_ready_list: Vec<usize>,
}

impl ScheduleUnitGraph {
    fn new(sm: &dyn ShaderModel, instrs: &[Instr]) -> Self {
        let mut g = generate_dep_graph(sm, instrs);

        let init_ready_list = calc_statistics(&mut g);

        g.reverse();

        Self { g, init_ready_list }
    }
}

#[derive(Default)]
pub(super) struct ScheduleUnit {
    /// live counts from after the phi_srcs
    pub(super) live_in_count: Option<PerRegFile<u32>>,
    /// live variables from before phi_dsts/branch
    pub(super) live_out: Option<LiveSet>,

    pub(super) instrs: Vec<Instr>,
    new_order: Option<InstructionOrder>,
    pub(super) last_tried_schedule_type: Option<ScheduleType>,
    pub(super) peak_gpr_count: i32,
    pub(super) skip_schedule: bool,

    // Phis and branches aren't scheduled. Phis and par copies are the only
    // instructions that can take an arbitrary number of srs/dests and therefore
    // can overflow in net_live tracking. We simplify that accounting by not
    // handling these instructions there.
    pub(super) phi_dsts: Option<Instr>,
    pub(super) phi_srcs: Option<Instr>,
    pub(super) branch: Option<Instr>,

    graph: Option<ScheduleUnitGraph>,
}

impl ScheduleUnit {
    pub(super) fn schedule(
        &mut self,
        sm: &dyn ShaderModel,
        max_reg_count: PerRegFile<i32>,
        schedule_type: ScheduleType,
        thresholds: ScheduleThresholds,
    ) {
        let graph = self
            .graph
            .get_or_insert_with(|| ScheduleUnitGraph::new(sm, &self.instrs));

        self.last_tried_schedule_type = Some(schedule_type);
        let new_order = sched_buffer(
            max_reg_count,
            &self.instrs,
            graph,
            self.live_in_count
                .expect("live_in_count must be set before scheduling"),
            self.live_out
                .as_ref()
                .expect("live_out must be set before scheduling"),
            thresholds,
        );

        if let Some(x) = new_order {
            self.new_order = Some(x);
        }
    }

    pub(super) fn finish_block(&mut self, live_out: &LiveSet) {
        if self.live_out.is_none() {
            self.live_out = Some(live_out.clone());
        }
    }

    pub(super) fn has_new_order(&self) -> bool {
        self.new_order.is_some()
    }

    pub(super) fn to_instrs(self) -> Vec<Instr> {
        let mut instrs = Vec::new();
        instrs.extend(self.phi_dsts);
        match self.new_order {
            Some(order) => instrs.extend(order.apply(self.instrs)),
            None => instrs.extend(self.instrs.into_iter()),
        }
        instrs.extend(self.phi_srcs);
        instrs.extend(self.branch);
        instrs
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScheduleType {
    RegLimit(u8),
    Spill,
}

impl ScheduleType {
    pub(super) fn thresholds(
        &self,
        max_reg_count: PerRegFile<i32>,
        schedule_unit: &ScheduleUnit,
    ) -> ScheduleThresholds {
        match self {
            Self::RegLimit(gpr_target) => ScheduleThresholds {
                heuristic_threshold: i32::from(*gpr_target) - TARGET_FREE,
                quit_threshold: i32::from(*gpr_target),
            },
            Self::Spill => ScheduleThresholds {
                heuristic_threshold: max_reg_count[RegFile::GPR]
                    - SW_RESERVED_GPRS_SPILL
                    - TARGET_FREE,
                quit_threshold: schedule_unit.peak_gpr_count,
            },
        }
    }
}

pub(super) fn get_schedule_types(
    sm: &dyn ShaderModel,
    max_reg_count: PerRegFile<i32>,
    min_gpr_target: i32,
    max_gpr_target: i32,
    reserved_gprs: i32,
) -> Vec<ScheduleType> {
    let mut out = Vec::new();

    let mut gpr_target = next_occupancy_cliff_with_reserved(sm, min_gpr_target, reserved_gprs);
    while gpr_target < max_reg_count[RegFile::GPR] {
        out.push(ScheduleType::RegLimit(
            gpr_target.try_into().expect("gpr_target must fit in u8"),
        ));

        // We want only 1 entry that's greater than or equal to the original
        // schedule (it can be greater in cases where increasing the number of
        // registers doesn't change occupancy)
        if gpr_target >= max_gpr_target {
            return out;
        }

        gpr_target = next_occupancy_cliff_with_reserved(sm, gpr_target + 1, reserved_gprs);
    }

    assert!(gpr_target >= max_reg_count[RegFile::GPR]);
    let limit = max_reg_count[RegFile::GPR] - SW_RESERVED_GPRS;
    out.push(ScheduleType::RegLimit(
        limit
            .try_into()
            .expect("max_reg_count - reserved must fit in u8"),
    ));

    // Only allow spilling if the original schedule spilled
    if max_gpr_target > max_reg_count[RegFile::GPR] {
        out.push(ScheduleType::Spill);
    }
    return out;
}
