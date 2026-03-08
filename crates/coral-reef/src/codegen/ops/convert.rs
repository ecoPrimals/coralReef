// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! Type conversion operation encoding — F2F, F2I, I2F, I2I.

use super::{AmdOpEncoder, EncodeOp, dst_to_vgpr_index, src_to_encoding};
use crate::CompileError;
use crate::codegen::amd::encoding::Rdna2Encoder;
use crate::codegen::amd::isa;
use crate::codegen::amd::reg::AmdRegRef;
#[allow(
    clippy::wildcard_imports,
    reason = "op module re-exports are intentional for codegen"
)]
use crate::codegen::ir::*;

// ---- F2F (VOP1: V_MOV_B32 — same-size pass-through for now) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpF2F {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&self.src)?;
        let mut words =
            Rdna2Encoder::encode_vop1(isa::vop1::V_MOV_B32, AmdRegRef::vgpr(dst_reg), src_enc.src0);
        src_enc.extend_with_literal(&mut words);
        Ok(words)
    }
}

// ---- F2I (VOP1: V_CVT_I32_F32) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpF2I {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&self.src)?;
        let mut words = Rdna2Encoder::encode_vop1(
            isa::vop1::V_CVT_I32_F32,
            AmdRegRef::vgpr(dst_reg),
            src_enc.src0,
        );
        src_enc.extend_with_literal(&mut words);
        Ok(words)
    }
}

// ---- I2F (VOP1: V_CVT_F32_I32) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpI2F {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&self.src)?;
        let mut words = Rdna2Encoder::encode_vop1(
            isa::vop1::V_CVT_F32_I32,
            AmdRegRef::vgpr(dst_reg),
            src_enc.src0,
        );
        src_enc.extend_with_literal(&mut words);
        Ok(words)
    }
}

// ---- I2I (VOP1: V_MOV_B32 — bit-preserving pass-through) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpI2I {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&self.src)?;
        let mut words =
            Rdna2Encoder::encode_vop1(isa::vop1::V_MOV_B32, AmdRegRef::vgpr(dst_reg), src_enc.src0);
        src_enc.extend_with_literal(&mut words);
        Ok(words)
    }
}
