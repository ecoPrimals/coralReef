// Copyright © 2025 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! SM32 instruction encoding and legalization.

#![allow(clippy::wildcard_imports)]

mod encoder;
pub use self::encoder::*;

mod alu;
mod control;
mod mem;
mod tex;

use super::ir::*;
use super::sm30_instr_latencies::encode_kepler_shader;

fn encode_sm32_shader(sm: &ShaderModel32, s: &Shader<'_>) -> Vec<u32> {
    encode_kepler_shader(sm, s)
}
