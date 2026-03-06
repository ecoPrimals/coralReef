// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2025)
//! SM32 instruction encoding and legalization.

#![allow(clippy::wildcard_imports)]

mod encoder;
pub use self::encoder::*;

mod alu;
mod control;
mod mem;
mod tex;

use super::sm30_instr_latencies::encode_kepler_shader;

fn encode_sm32_shader(sm: &ShaderModel32, s: &Shader<'_>) -> Vec<u32> {
    encode_kepler_shader(sm, s)
}
