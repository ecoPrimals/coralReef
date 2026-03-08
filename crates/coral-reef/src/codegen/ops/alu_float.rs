// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! Float ALU operation encoding — FAdd, FMul, FFma, FMnMx, DAdd, DMul, DFma, etc.
//!
//! Also includes float/int/double comparisons (FSetP, DSetP, ISetP) since
//! they share the same VOP2/VOP3/VOPC encoding infrastructure.

use super::{
    AmdOpEncoder, EncodeOp, dst_to_vgpr_index, encode_vop2_from_srcs, encode_vop3_from_srcs,
    src_to_encoding,
};
use crate::CompileError;
use crate::codegen::amd::encoding::Rdna2Encoder;
use crate::codegen::amd::isa;
use crate::codegen::amd::reg::AmdRegRef;
#[allow(clippy::wildcard_imports)]
use crate::codegen::ir::*;

// ---- FAdd (VOP2: V_ADD_F32) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpFAdd {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        encode_vop2_from_srcs(
            isa::vop2::V_ADD_F32,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
        )
    }
}

// ---- FMul (VOP2: V_MUL_F32) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpFMul {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        encode_vop2_from_srcs(
            isa::vop2::V_MUL_F32,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
        )
    }
}

// ---- FFma (VOP3: V_FMA_F32) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpFFma {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        encode_vop3_from_srcs(
            isa::vop3::V_FMA_F32,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            &self.srcs[2],
        )
    }
}

// ---- FMnMx (VOP2: V_MIN_F32 / V_MAX_F32) ----
//
// FMnMx selects min or max based on a predicate source:
//   min=True  → V_MIN_F32
//   min=False → V_MAX_F32

impl EncodeOp<AmdOpEncoder<'_>> for OpFMnMx {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let is_min = matches!(self.min.reference, SrcRef::True);
        let opcode = if is_min {
            isa::vop2::V_MIN_F32
        } else {
            isa::vop2::V_MAX_F32
        };
        encode_vop2_from_srcs(opcode, &self.dst, &self.srcs[0], &self.srcs[1])
    }
}

// ---- DAdd (VOP3: V_ADD_F64) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpDAdd {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        encode_vop3_from_srcs(
            isa::vop3::V_ADD_F64,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            &Src::ZERO,
        )
    }
}

// ---- DMul (VOP3: V_MUL_F64) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpDMul {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        encode_vop3_from_srcs(
            isa::vop3::V_MUL_F64,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            &Src::ZERO,
        )
    }
}

// ---- DFma (VOP3: V_FMA_F64) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpDFma {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        encode_vop3_from_srcs(
            isa::vop3::V_FMA_F64,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            &self.srcs[2],
        )
    }
}

// ---- DMnMx (VOP3: V_MIN_F64 / V_MAX_F64) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpDMnMx {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let is_min = matches!(self.min.reference, SrcRef::True);
        let opcode = if is_min {
            isa::vop3::V_MIN_F64
        } else {
            isa::vop3::V_MAX_F64
        };
        encode_vop3_from_srcs(opcode, &self.dst, &self.srcs[0], &self.srcs[1], &Src::ZERO)
    }
}

// ---- F64Sqrt (VOP3: V_SQRT_F64) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpF64Sqrt {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&Src::from(self.src.reference.clone()))?;
        if src_enc.literal.is_some() {
            return Err(CompileError::NotImplemented(
                "VOP3 F64Sqrt does not support literal constants".into(),
            ));
        }
        Ok(Rdna2Encoder::encode_vop3(
            isa::vop3::V_SQRT_F64,
            AmdRegRef::vgpr_pair(dst_reg),
            src_enc.src0,
            0,
            0,
        ))
    }
}

// ---- F64Rcp (VOP3: V_RCP_F64) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpF64Rcp {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&Src::from(self.src.reference.clone()))?;
        if src_enc.literal.is_some() {
            return Err(CompileError::NotImplemented(
                "VOP3 F64Rcp does not support literal constants".into(),
            ));
        }
        Ok(Rdna2Encoder::encode_vop3(
            isa::vop3::V_RCP_F64,
            AmdRegRef::vgpr_pair(dst_reg),
            src_enc.src0,
            0,
            0,
        ))
    }
}

// ---- FSetP (VOPC: V_CMP_*_F32) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpFSetP {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let src0_enc = src_to_encoding(&self.srcs[0])?;
        let src1_vgpr = super::src_to_vgpr_index(&self.srcs[1])?;
        let vopc_opcode = float_cmp_to_vopc_f32(self.cmp_op);
        let mut words = Rdna2Encoder::encode_vopc(vopc_opcode, src0_enc.src0, src1_vgpr);
        src0_enc.extend_with_literal(&mut words);
        Ok(words)
    }
}

// ---- DSetP (VOP3: V_CMP_*_F64) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpDSetP {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let src0_enc = src_to_encoding(&self.srcs[0])?;
        let src1_enc = src_to_encoding(&self.srcs[1])?;
        if src0_enc.literal.is_some() || src1_enc.literal.is_some() {
            return Err(CompileError::NotImplemented(
                "VOP3 DSetP does not support literal constants".into(),
            ));
        }
        let vop3_opcode = float_cmp_to_vop3_f64(self.cmp_op);
        let dst = AmdRegRef::vgpr(0);
        Ok(Rdna2Encoder::encode_vop3(
            vop3_opcode,
            dst,
            src0_enc.src0,
            src1_enc.src0,
            0,
        ))
    }
}

// ---- ISetP (VOPC: V_CMP_*_I32 / V_CMP_*_U32) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpISetP {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let src0_enc = src_to_encoding(&self.srcs[0])?;
        let src1_vgpr = super::src_to_vgpr_index(&self.srcs[1])?;
        let vopc_opcode = int_cmp_to_vopc(self.cmp_op, self.cmp_type);
        let mut words = Rdna2Encoder::encode_vopc(vopc_opcode, src0_enc.src0, src1_vgpr);
        src0_enc.extend_with_literal(&mut words);
        Ok(words)
    }
}

// ---- Comparison lookup tables ----

fn float_cmp_to_vopc_f32(cmp: FloatCmpOp) -> u16 {
    match cmp {
        FloatCmpOp::OrdEq => isa::vopc::V_CMP_EQ_F32,
        FloatCmpOp::OrdNe => isa::vopc::V_CMP_NEQ_F32,
        FloatCmpOp::OrdLt => isa::vopc::V_CMP_LT_F32,
        FloatCmpOp::OrdLe => isa::vopc::V_CMP_LE_F32,
        FloatCmpOp::OrdGt => isa::vopc::V_CMP_GT_F32,
        FloatCmpOp::OrdGe => isa::vopc::V_CMP_GE_F32,
        FloatCmpOp::UnordEq => isa::vopc::V_CMP_NLG_F32,
        FloatCmpOp::UnordNe => isa::vopc::V_CMP_NLE_F32,
        FloatCmpOp::UnordLt => isa::vopc::V_CMP_NGE_F32,
        FloatCmpOp::UnordLe => isa::vopc::V_CMP_NGT_F32,
        FloatCmpOp::UnordGt => isa::vopc::V_CMP_NLE_F32,
        FloatCmpOp::UnordGe => isa::vopc::V_CMP_NLT_F32,
        FloatCmpOp::IsNum => isa::vopc::V_CMP_O_F32,
        FloatCmpOp::IsNan => isa::vopc::V_CMP_U_F32,
    }
}

fn float_cmp_to_vop3_f64(cmp: FloatCmpOp) -> u16 {
    match cmp {
        FloatCmpOp::OrdEq => isa::vop3::V_CMP_EQ_F64,
        FloatCmpOp::OrdNe => isa::vop3::V_CMP_NEQ_F64,
        FloatCmpOp::OrdLt => isa::vop3::V_CMP_LT_F64,
        FloatCmpOp::OrdLe => isa::vop3::V_CMP_LE_F64,
        FloatCmpOp::OrdGt => isa::vop3::V_CMP_GT_F64,
        FloatCmpOp::OrdGe => isa::vop3::V_CMP_GE_F64,
        FloatCmpOp::UnordEq
        | FloatCmpOp::UnordNe
        | FloatCmpOp::UnordLt
        | FloatCmpOp::UnordLe
        | FloatCmpOp::UnordGt
        | FloatCmpOp::UnordGe => isa::vop3::V_CMP_U_F64,
        FloatCmpOp::IsNum => isa::vop3::V_CMP_O_F64,
        FloatCmpOp::IsNan => isa::vop3::V_CMP_U_F64,
    }
}

fn int_cmp_to_vopc(cmp: IntCmpOp, cmp_type: IntCmpType) -> u16 {
    if cmp_type.is_signed() {
        match cmp {
            IntCmpOp::False => isa::vopc::V_CMP_F_I32,
            IntCmpOp::True => isa::vopc::V_CMP_T_I32,
            IntCmpOp::Eq => isa::vopc::V_CMP_EQ_I32,
            IntCmpOp::Ne => isa::vopc::V_CMP_NE_I32,
            IntCmpOp::Lt => isa::vopc::V_CMP_LT_I32,
            IntCmpOp::Le => isa::vopc::V_CMP_LE_I32,
            IntCmpOp::Gt => isa::vopc::V_CMP_GT_I32,
            IntCmpOp::Ge => isa::vopc::V_CMP_GE_I32,
        }
    } else {
        match cmp {
            IntCmpOp::False => isa::vopc::V_CMP_F_U32,
            IntCmpOp::True => isa::vopc::V_CMP_T_U32,
            IntCmpOp::Eq => isa::vopc::V_CMP_EQ_U32,
            IntCmpOp::Ne => isa::vopc::V_CMP_NE_U32,
            IntCmpOp::Lt => isa::vopc::V_CMP_LT_U32,
            IntCmpOp::Le => isa::vopc::V_CMP_LE_U32,
            IntCmpOp::Gt => isa::vopc::V_CMP_GT_U32,
            IntCmpOp::Ge => isa::vopc::V_CMP_GE_U32,
        }
    }
}
