// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! Control flow operation encoding — Exit, Nop, Bar, Bra.

use super::{AmdOpEncoder, EncodeOp};
use crate::CompileError;
use crate::codegen::amd::encoding::{self, Rdna2Encoder};
use crate::codegen::ir::*;

// ---- Exit (SOPP: S_ENDPGM) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpExit {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        Ok(encoding::encode_s_endpgm())
    }
}

// ---- Nop (SOPP: S_NOP) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpNop {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        Ok(encoding::encode_s_nop(0))
    }
}

// ---- Bar (SOPP: S_BARRIER) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpBar {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        Ok(encoding::encode_s_barrier())
    }
}

// ---- Bra (SOPP: S_BRANCH / S_CBRANCH_VCCNZ) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpBra {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let target_ip =
            e.labels.get(&self.target).copied().ok_or_else(|| {
                CompileError::InvalidInput("branch target label not found".into())
            })?;
        let next_ip = e.ip + 1; // SOPP is 1 word
        let target_i32 = i32::try_from(target_ip).map_err(|_| {
            CompileError::InvalidInput(format!("branch target IP {target_ip} exceeds i32").into())
        })?;
        let next_i32 = i32::try_from(next_ip).map_err(|_| {
            CompileError::InvalidInput(format!("branch source IP {next_ip} exceeds i32").into())
        })?;
        let offset = target_i32.wrapping_sub(next_i32);
        let offset_i16 = i16::try_from(offset).map_err(|_| {
            CompileError::InvalidInput(format!("branch offset {offset} exceeds i16 range").into())
        })?;

        if matches!(self.cond.reference, SrcRef::True) {
            Ok(Rdna2Encoder::encode_s_branch(offset_i16))
        } else if matches!(self.cond.modifier, SrcMod::BNot) {
            // Naga translator emits Bra(cond.bnot(), label) for if-then / if-then-else.
            // BNot means "branch when original condition is FALSE" → VCCZ.
            Ok(Rdna2Encoder::encode_s_cbranch_vccz(offset_i16))
        } else {
            // No negation — branch when condition is TRUE → VCCNZ (used by loop break_if).
            Ok(Rdna2Encoder::encode_s_cbranch_vccnz(offset_i16))
        }
    }
}
