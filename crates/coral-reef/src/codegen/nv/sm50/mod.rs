// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)
//! SM50 instruction encoding and legalization.

mod encoder;
pub use self::encoder::*;

mod alu;
mod control;
mod mem;
mod tex;

#[cfg(test)]
#[path = "control_tests.rs"]
mod control_tests;

use coral_reef_stubs::fxhash::FxHashMap;

fn encode_instr(
    instr_index: usize,
    instr: Option<&Instr>,
    sm: &ShaderModel50,
    labels: &FxHashMap<Label, usize>,
    ip: &mut usize,
    sched_instr: &mut [u32; 2],
) -> [u32; 2] {
    let mut e = SM50Encoder {
        sm,
        ip: *ip,
        labels,
        inst: [0_u32; 2],
        sched: 0,
    };

    if let Some(instr) = instr {
        instr.op.encode(&mut e);
        e.set_pred(&instr.pred);
        e.set_instr_deps(&instr.deps);
    } else {
        let nop = OpNop { label: None };
        nop.encode(&mut e);
        e.set_pred(&true.into());
        e.set_instr_deps(&InstrDeps::new());
    }

    *ip += 8;

    sched_instr.set_field(21 * instr_index..21 * (instr_index + 1), e.sched);

    e.inst
}

pub(super) fn encode_sm50_shader(sm: &ShaderModel50, s: &Shader<'_>) -> Vec<u32> {
    assert!(s.functions.len() == 1);
    let func = &s.functions[0];

    let mut instr_count = 0_usize;
    let mut labels = FxHashMap::default();
    for b in &func.blocks {
        // We ensure blocks will have groups of 3 instructions with a
        // schedule instruction before each groups.  As we should never jump
        // to a schedule instruction, we account for that here.
        labels.insert(b.label, instr_count + 8);

        let block_num_instrs = b.instrs.len().next_multiple_of(3);

        // Every 3 instructions, we have a new schedule instruction so we
        // need to account for that.
        instr_count += (block_num_instrs + (block_num_instrs / 3)) * 8;
    }

    let mut encoded = Vec::new();
    for b in &func.blocks {
        // A block is composed of groups of 3 instructions.
        let block_num_instrs = b.instrs.len().next_multiple_of(3);

        let mut instrs_iter = b.instrs.iter();

        for _ in 0..(block_num_instrs / 3) {
            let mut ip = ((encoded.len() / 2) + 1) * 8;

            let mut sched_instr = [0x0; 2];

            let instr0 = encode_instr(
                0,
                instrs_iter.next(),
                sm,
                &labels,
                &mut ip,
                &mut sched_instr,
            );
            let instr1 = encode_instr(
                1,
                instrs_iter.next(),
                sm,
                &labels,
                &mut ip,
                &mut sched_instr,
            );
            let instr2 = encode_instr(
                2,
                instrs_iter.next(),
                sm,
                &labels,
                &mut ip,
                &mut sched_instr,
            );

            encoded.extend_from_slice(&sched_instr[..]);
            encoded.extend_from_slice(&instr0[..]);
            encoded.extend_from_slice(&instr1[..]);
            encoded.extend_from_slice(&instr2[..]);
        }
    }

    encoded
}
