// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Valve Corporation (2025)

use crate::codegen::ir::*;
use crate::codegen::liveness::LiveSet;
use coral_reef_stubs::fxhash::FxHashMap;
use std::ops::Index;

/// The net change in live values, from the end of an instruction to a
/// specific point during the instruction's execution
pub(super) struct InstrCount {
    /// The net change in live values across the whole instruction
    pub net: PerRegFile<i8>,

    /// peak1 is at the end of the instruction, where any immediately-killed
    /// defs are live
    pub peak1: PerRegFile<i8>,

    /// peak2 is just before sources are read, and after vector defs are live
    pub peak2: PerRegFile<i8>,
}

/// For each instruction, keep track of a "net live" value, which is how
/// much the size of the live values set will change if we schedule a given
/// instruction next. This is tracked per-register-file.
///
/// Assumes that we are iterating over instructions in reverse order
pub(super) struct NetLive {
    counts: Vec<InstrCount>,
    ssa_to_instr: FxHashMap<SSAValue, Vec<usize>>,
}

impl NetLive {
    pub(super) fn new(instrs: &[Instr], live_out: &LiveSet) -> Self {
        let mut use_set = LiveSet::new();
        let mut ssa_to_instr = FxHashMap::default();

        let mut counts: Vec<InstrCount> = instrs
            .iter()
            .enumerate()
            .map(|(instr_idx, instr)| {
                use_set.clear();
                for ssa in instr.ssa_uses() {
                    if !live_out.contains(ssa) {
                        if use_set.insert(*ssa) {
                            ssa_to_instr
                                .entry(*ssa)
                                .or_insert_with(Vec::new)
                                .push(instr_idx);
                        }
                    }
                }

                let net = PerRegFile::new_with(|f| {
                    use_set
                        .count(f)
                        .try_into()
                        .expect("live count must fit in i8")
                });
                InstrCount {
                    net,
                    peak1: PerRegFile::default(),
                    peak2: net,
                }
            })
            .collect();

        for (instr_idx, instr) in instrs.iter().enumerate() {
            for dst in instr.dsts() {
                let is_vector = dst.iter_ssa().len() > 1;
                let count = &mut counts[instr_idx];

                for &ssa in dst.iter_ssa() {
                    if ssa_to_instr.contains_key(&ssa) || live_out.contains(&ssa) {
                        count.net[ssa.file()] -= 1;
                    } else {
                        count.peak1[ssa.file()] += 1;
                        count.peak2[ssa.file()] += 1;
                    }

                    if !is_vector {
                        count.peak2[ssa.file()] -= 1;
                    }
                }
            }
        }

        Self {
            counts,
            ssa_to_instr,
        }
    }

    pub(super) fn remove(&mut self, ssa: SSAValue) -> bool {
        match self.ssa_to_instr.remove(&ssa) {
            Some(instr_idxs) => {
                assert!(!instr_idxs.is_empty());
                let file = ssa.file();
                for i in instr_idxs {
                    self.counts[i].net[file] -= 1;
                    self.counts[i].peak2[file] -= 1;
                }
                true
            }
            None => false,
        }
    }
}

impl Index<usize> for NetLive {
    type Output = InstrCount;

    fn index(&self, index: usize) -> &Self::Output {
        &self.counts[index]
    }
}
