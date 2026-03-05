// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! SM70 instruction encoding and legalization.

#![allow(clippy::wildcard_imports)]

mod encoder;
pub use self::encoder::*;

mod alu;
mod control;
mod mem;
mod tex;

use super::ir::*;
use super::legalize::LegalizeBuilder;
use super::sm70::ShaderModel70;
use rustc_hash::FxHashMap;

pub fn legalize_sm70_op(_sm: &ShaderModel70, b: &mut LegalizeBuilder, op: &mut Op) {
    op.legalize(b);
}

pub fn encode_sm70_shader(sm: &ShaderModel70, s: &Shader<'_>) -> Vec<u32> {
    assert!(s.functions.len() == 1);
    let func = &s.functions[0];

    let mut ip = 0_usize;
    let mut labels = FxHashMap::default();
    for b in &func.blocks {
        labels.insert(b.label, ip);
        for instr in &b.instrs {
            if let Op::Nop(op) = &instr.op {
                if let Some(label) = op.label {
                    labels.insert(label, ip);
                }
            }
            ip += 4;
        }
    }

    let mut encoded = Vec::new();
    for b in &func.blocks {
        for instr in &b.instrs {
            let mut e = SM70Encoder {
                sm: sm.sm(),
                ip: encoded.len(),
                labels: &labels,
                inst: [0_u32; 4],
            };
            instr.op.encode(&mut e);
            e.set_pred(&instr.pred);
            e.set_instr_deps(&instr.deps);
            encoded.extend_from_slice(&e.inst[..]);
        }
    }
    encoded
}
