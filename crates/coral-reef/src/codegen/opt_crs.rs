// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

use super::ir::*;

use coral_reef_stubs::fxhash::FxHashSet;

fn opt_crs(f: &mut Function) {
    let mut live_targets: FxHashSet<Label> = FxHashSet::default();
    for b in &f.blocks {
        let Some(instr) = b.instrs.last() else {
            continue;
        };

        match &instr.op {
            Op::Sync(OpSync { target })
            | Op::Brk(OpBrk { target })
            | Op::Cont(OpCont { target }) => {
                live_targets.insert(*target);
            }
            _ => (),
        }
    }

    f.map_instrs(|instr, _| match &instr.op {
        Op::SSy(OpSSy { target }) | Op::PBk(OpPBk { target }) | Op::PCnt(OpPCnt { target }) => {
            if live_targets.contains(target) {
                MappedInstrs::One(instr)
            } else {
                MappedInstrs::None
            }
        }
        _ => MappedInstrs::One(instr),
    });
}

impl Shader<'_> {
    pub fn opt_crs(&mut self) {
        for f in &mut self.functions {
            opt_crs(f);
        }
    }
}
