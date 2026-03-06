// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2025)
//! SM20 instruction encoding and legalization.

#![allow(clippy::wildcard_imports)]

mod encoder;
pub use self::encoder::*;

mod alu;
mod control;
mod mem;
mod tex;

use super::sm30_instr_latencies::encode_kepler_shader;
use crate::codegen::ir::*;
use coral_reef_stubs::fxhash::FxHashMap;

pub(super) fn encode_sm20_shader(sm: &ShaderModel20, s: &Shader<'_>) -> Vec<u32> {
    assert!(s.functions.len() == 1);
    let func = &s.functions[0];

    let mut ip = 0_usize;
    let mut labels = FxHashMap::default();
    for b in &func.blocks {
        // We ensure blocks will have groups of 7 instructions with a
        // schedule instruction before each groups.  As we should never jump
        // to a schedule instruction, we account for that here.
        labels.insert(b.label, ip);
        ip += b.instrs.len() * 8;
    }

    let mut encoded = Vec::new();
    for b in &func.blocks {
        for instr in &b.instrs {
            let mut e = SM20Encoder {
                sm,
                ip: encoded.len() * 4,
                labels: &labels,
                inst: [0_u32; 2],
            };
            instr.op.encode(&mut e);
            e.set_pred(&instr.pred);
            encoded.extend(&e.inst[..]);
        }
    }

    encoded
}

pub(super) fn encode_sm30_shader(sm: &ShaderModel20, s: &Shader<'_>) -> Vec<u32> {
    assert!(sm.sm() >= 30);
    encode_kepler_shader(sm, s)
}
