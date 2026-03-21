// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! Type conversion operation encoding — F2F, F2I, I2F, I2I.

use super::{AmdOpEncoder, EncodeOp, dst_to_vgpr_index, src_to_encoding};
use crate::CompileError;
use crate::codegen::amd::encoding::Rdna2Encoder;
use crate::codegen::amd::isa;
use crate::codegen::amd::reg::AmdRegRef;
use crate::codegen::ir::*;

// ---- F2F (float width conversion) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpF2F {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&self.src)?;

        let opcode = match (self.src_type, self.dst_type) {
            (FloatType::F32, FloatType::F64) => isa::vop1::V_CVT_F64_F32,
            (FloatType::F64, FloatType::F32) => isa::vop1::V_CVT_F32_F64,
            _ => isa::vop1::V_MOV_B32,
        };

        let dst_ref = match self.dst_type {
            FloatType::F64 => AmdRegRef::vgpr_pair(dst_reg),
            _ => AmdRegRef::vgpr(dst_reg),
        };

        let mut words = Rdna2Encoder::encode_vop1(opcode, dst_ref, src_enc.src0);
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
