// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! Float ALU operation encoding — FAdd, FMul, FFma, FMnMx, DAdd, DMul, DFma, etc.
//!
//! Also includes float/int/double comparisons (FSetP, DSetP, ISetP) since
//! they share the same VOP2/VOP3/VOPC encoding infrastructure.

use super::{
    AmdOpEncoder, EncodeOp, SrcEncoding, dst_to_vgpr_index, encode_vop2_from_srcs,
    encode_vop3_f64_from_srcs, encode_vop3_from_srcs, encode_vopc_legalized,
    materialize_f64_if_literal, materialize_if_literal, src_to_encoding,
};
use crate::CompileError;
use crate::codegen::amd::encoding::Rdna2Encoder;
use crate::codegen::amd::isa;
use crate::codegen::amd::reg::AmdRegRef;
use crate::codegen::ir::*;

// ---- FAdd (VOP2: V_ADD_F32) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpFAdd {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        encode_vop2_from_srcs(
            isa::vop2::V_ADD_F32,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            e,
        )
    }
}

// ---- FMul (VOP2: V_MUL_F32) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpFMul {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        encode_vop2_from_srcs(
            isa::vop2::V_MUL_F32,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            e,
        )
    }
}

// ---- FFma (VOP3: V_FMA_F32) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpFFma {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        encode_vop3_from_srcs(
            isa::vop3::V_FMA_F32,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            &self.srcs[2],
            e,
        )
    }
}

// ---- Transcendental (VOP1: f32 special functions) ----
//
// Maps TranscendentalOp → RDNA2 VOP1 instructions.

impl EncodeOp<AmdOpEncoder<'_>> for OpTranscendental {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&self.src)?;
        let vop1_opcode = match self.op {
            TranscendentalOp::Cos => isa::vop1::V_COS_F32,
            TranscendentalOp::Sin => isa::vop1::V_SIN_F32,
            TranscendentalOp::Exp2 => isa::vop1::V_EXP_F32,
            TranscendentalOp::Log2 => isa::vop1::V_LOG_F32,
            TranscendentalOp::Rcp => isa::vop1::V_RCP_F32,
            TranscendentalOp::Rsq => isa::vop1::V_RSQ_F32,
            TranscendentalOp::Sqrt => isa::vop1::V_SQRT_F32,
            TranscendentalOp::Rcp64H => {
                let (mut prefix, materialized) =
                    materialize_f64_if_literal(e.scratch_vgpr_0, &src_enc);
                let words = Rdna2Encoder::encode_vop3(
                    isa::vop3::V_RCP_F64,
                    AmdRegRef::vgpr_pair(dst_reg),
                    materialized.src0,
                    0,
                    0,
                );
                prefix.extend(words);
                return Ok(prefix);
            }
            TranscendentalOp::Rsq64H => {
                let (mut prefix, materialized) =
                    materialize_f64_if_literal(e.scratch_vgpr_0, &src_enc);
                let words = Rdna2Encoder::encode_vop3(
                    isa::vop3::V_RSQ_F64,
                    AmdRegRef::vgpr_pair(dst_reg),
                    materialized.src0,
                    0,
                    0,
                );
                prefix.extend(words);
                return Ok(prefix);
            }
            TranscendentalOp::Tanh => {
                return Err(CompileError::NotImplemented(
                    "AMD encoding for f32 tanh not yet available".into(),
                ));
            }
        };
        let mut words =
            Rdna2Encoder::encode_vop1(vop1_opcode, AmdRegRef::vgpr(dst_reg), src_enc.src0);
        src_enc.extend_with_literal(&mut words);
        Ok(words)
    }
}

// ---- FRnd (VOP1: V_TRUNC / V_FLOOR / V_CEIL / V_RNDNE) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpFRnd {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&self.src)?;

        if self.src_type == FloatType::F64 || self.dst_type == FloatType::F64 {
            let vop3_opcode = match self.rnd_mode {
                FRndMode::Zero => isa::vop3::V_TRUNC_F64,
                FRndMode::NegInf => isa::vop3::V_FLOOR_F64,
                FRndMode::PosInf => isa::vop3::V_CEIL_F64,
                FRndMode::NearestEven => isa::vop3::V_RNDNE_F64,
            };
            let (mut prefix, materialized) = materialize_if_literal(_e.scratch_vgpr_0, &src_enc);
            let words = Rdna2Encoder::encode_vop3(
                vop3_opcode,
                AmdRegRef::vgpr_pair(dst_reg),
                materialized.src0,
                0,
                0,
            );
            prefix.extend(words);
            Ok(prefix)
        } else {
            let vop1_opcode = match self.rnd_mode {
                FRndMode::Zero => isa::vop1::V_TRUNC_F32,
                FRndMode::NegInf => isa::vop1::V_FLOOR_F32,
                FRndMode::PosInf => isa::vop1::V_CEIL_F32,
                FRndMode::NearestEven => isa::vop1::V_RNDNE_F32,
            };
            let mut words =
                Rdna2Encoder::encode_vop1(vop1_opcode, AmdRegRef::vgpr(dst_reg), src_enc.src0);
            src_enc.extend_with_literal(&mut words);
            Ok(words)
        }
    }
}

// ---- FMnMx (VOP2: V_MIN_F32 / V_MAX_F32) ----
//
// FMnMx selects min or max based on a predicate source:
//   min=True  -> V_MIN_F32
//   min=False -> V_MAX_F32

impl EncodeOp<AmdOpEncoder<'_>> for OpFMnMx {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let is_min = matches!(self.min().reference, SrcRef::True);
        let opcode = if is_min {
            isa::vop2::V_MIN_F32
        } else {
            isa::vop2::V_MAX_F32
        };
        encode_vop2_from_srcs(opcode, &self.dst, &self.srcs[0], &self.srcs[1], e)
    }
}

// ---- DAdd (VOP3: V_ADD_F64) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpDAdd {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        encode_vop3_f64_from_srcs(
            isa::vop3::V_ADD_F64,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            &Src::ZERO,
            e,
        )
    }
}

// ---- DMul (VOP3: V_MUL_F64) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpDMul {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        encode_vop3_f64_from_srcs(
            isa::vop3::V_MUL_F64,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            &Src::ZERO,
            e,
        )
    }
}

// ---- DFma (VOP3: V_FMA_F64) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpDFma {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        encode_vop3_f64_from_srcs(
            isa::vop3::V_FMA_F64,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            &self.srcs[2],
            e,
        )
    }
}

// ---- DMnMx (VOP3: V_MIN_F64 / V_MAX_F64) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpDMnMx {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let is_min = matches!(self.min().reference, SrcRef::True);
        let opcode = if is_min {
            isa::vop3::V_MIN_F64
        } else {
            isa::vop3::V_MAX_F64
        };
        encode_vop3_f64_from_srcs(
            opcode,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            &Src::ZERO,
            e,
        )
    }
}

// ---- F64Sqrt (VOP3: V_SQRT_F64) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpF64Sqrt {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&Src::from(self.src.reference.clone()))?;
        let (mut prefix, materialized) =
            materialize_f64_if_literal(e.scratch_vgpr_0, &src_enc);
        let words = Rdna2Encoder::encode_vop3(
            isa::vop3::V_SQRT_F64,
            AmdRegRef::vgpr_pair(dst_reg),
            materialized.src0,
            0,
            0,
        );
        prefix.extend(words);
        Ok(prefix)
    }
}

// ---- F64Rcp (VOP3: V_RCP_F64) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpF64Rcp {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&Src::from(self.src.reference.clone()))?;
        let (mut prefix, materialized) =
            materialize_f64_if_literal(e.scratch_vgpr_0, &src_enc);
        let words = Rdna2Encoder::encode_vop3(
            isa::vop3::V_RCP_F64,
            AmdRegRef::vgpr_pair(dst_reg),
            materialized.src0,
            0,
            0,
        );
        prefix.extend(words);
        Ok(prefix)
    }
}

// ---- F64Exp2 — V_CVT_F32_F64 + V_EXP_F32 + V_CVT_F64_F32 ----
//
// RDNA2 has no native f64 exp2. Software path: convert to f32,
// apply f32 transcendental (~23 bits), convert back.

impl EncodeOp<AmdOpEncoder<'_>> for OpF64Exp2 {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&Src::from(self.src.reference.clone()))?;
        let (mut words, materialized) = materialize_if_literal(e.scratch_vgpr_0, &src_enc);
        let scratch = e.scratch_vgpr_0;
        words.extend(Rdna2Encoder::encode_vop1(
            isa::vop1::V_CVT_F32_F64,
            AmdRegRef::vgpr(scratch),
            materialized.src0,
        ));
        words.extend(Rdna2Encoder::encode_vop1(
            isa::vop1::V_EXP_F32,
            AmdRegRef::vgpr(scratch),
            256 + scratch,
        ));
        words.extend(Rdna2Encoder::encode_vop1(
            isa::vop1::V_CVT_F64_F32,
            AmdRegRef::vgpr_pair(dst_reg),
            256 + scratch,
        ));
        Ok(words)
    }
}

// ---- F64Log2 — V_CVT_F32_F64 + V_LOG_F32 + V_CVT_F64_F32 ----

impl EncodeOp<AmdOpEncoder<'_>> for OpF64Log2 {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&Src::from(self.src.reference.clone()))?;
        let (mut words, materialized) = materialize_if_literal(e.scratch_vgpr_0, &src_enc);
        let scratch = e.scratch_vgpr_0;
        words.extend(Rdna2Encoder::encode_vop1(
            isa::vop1::V_CVT_F32_F64,
            AmdRegRef::vgpr(scratch),
            materialized.src0,
        ));
        words.extend(Rdna2Encoder::encode_vop1(
            isa::vop1::V_LOG_F32,
            AmdRegRef::vgpr(scratch),
            256 + scratch,
        ));
        words.extend(Rdna2Encoder::encode_vop1(
            isa::vop1::V_CVT_F64_F32,
            AmdRegRef::vgpr_pair(dst_reg),
            256 + scratch,
        ));
        Ok(words)
    }
}

// ---- F64Sin — V_CVT_F32_F64 + V_SIN_F32 + V_CVT_F64_F32 ----

impl EncodeOp<AmdOpEncoder<'_>> for OpF64Sin {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&Src::from(self.src.reference.clone()))?;
        let (mut words, materialized) = materialize_if_literal(e.scratch_vgpr_0, &src_enc);
        let scratch = e.scratch_vgpr_0;
        words.extend(Rdna2Encoder::encode_vop1(
            isa::vop1::V_CVT_F32_F64,
            AmdRegRef::vgpr(scratch),
            materialized.src0,
        ));
        words.extend(Rdna2Encoder::encode_vop1(
            isa::vop1::V_SIN_F32,
            AmdRegRef::vgpr(scratch),
            256 + scratch,
        ));
        words.extend(Rdna2Encoder::encode_vop1(
            isa::vop1::V_CVT_F64_F32,
            AmdRegRef::vgpr_pair(dst_reg),
            256 + scratch,
        ));
        Ok(words)
    }
}

// ---- F64Cos — V_CVT_F32_F64 + V_COS_F32 + V_CVT_F64_F32 ----

impl EncodeOp<AmdOpEncoder<'_>> for OpF64Cos {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&Src::from(self.src.reference.clone()))?;
        let (mut words, materialized) = materialize_if_literal(e.scratch_vgpr_0, &src_enc);
        let scratch = e.scratch_vgpr_0;
        words.extend(Rdna2Encoder::encode_vop1(
            isa::vop1::V_CVT_F32_F64,
            AmdRegRef::vgpr(scratch),
            materialized.src0,
        ));
        words.extend(Rdna2Encoder::encode_vop1(
            isa::vop1::V_COS_F32,
            AmdRegRef::vgpr(scratch),
            256 + scratch,
        ));
        words.extend(Rdna2Encoder::encode_vop1(
            isa::vop1::V_CVT_F64_F32,
            AmdRegRef::vgpr_pair(dst_reg),
            256 + scratch,
        ));
        Ok(words)
    }
}

// ---- FSetP (VOPC: V_CMP_*_F32) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpFSetP {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let vopc_opcode = float_cmp_to_vopc_f32(self.cmp_op);
        encode_vopc_legalized(vopc_opcode, &self.srcs[0], &self.srcs[1], e)
    }
}

// ---- DSetP (VOP3: V_CMP_*_F64) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpDSetP {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let src0_enc = src_to_encoding(&self.srcs[0])?;
        let src1_enc = src_to_encoding(&self.srcs[1])?;
        let mut next_scratch = e.scratch_vgpr_0;
        let (mut prefix0, mat0) = if src0_enc.literal.is_some() {
            let (p, m) = materialize_if_literal(next_scratch, &src0_enc);
            next_scratch = e.scratch_vgpr_1;
            (p, m)
        } else {
            (Vec::new(), SrcEncoding::inline(src0_enc.src0))
        };
        let (prefix1, mat1) = if src1_enc.literal.is_some() {
            materialize_if_literal(next_scratch, &src1_enc)
        } else {
            (Vec::new(), SrcEncoding::inline(src1_enc.src0))
        };
        prefix0.extend(prefix1);
        let vop3_opcode = float_cmp_to_vop3_f64(self.cmp_op);
        let dst = AmdRegRef::vgpr(0);
        let words = Rdna2Encoder::encode_vop3(vop3_opcode, dst, mat0.src0, mat1.src0, 0);
        prefix0.extend(words);
        Ok(prefix0)
    }
}

// ---- ISetP (VOPC: V_CMP_*_I32 / V_CMP_*_U32) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpISetP {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let vopc_opcode = int_cmp_to_vopc(self.cmp_op, self.cmp_type);
        encode_vopc_legalized(vopc_opcode, &self.srcs[0], &self.srcs[1], e)
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

#[cfg(test)]
mod tests {
    use super::super::{AmdOpEncoder, EncodeOp, encode_amd_op};
    use super::*;
    use crate::codegen::ir::{Dst, Pred, PredRef, RegFile, RegRef, Src, SrcRef, SrcSwizzle};
    use coral_reef_stubs::fxhash::FxHashMap;

    fn vgpr(i: u32) -> Src {
        Src {
            reference: SrcRef::Reg(RegRef::new(RegFile::GPR, i, 1)),
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        }
    }

    fn dst_reg(i: u32) -> Dst {
        Dst::Reg(RegRef::new(RegFile::GPR, i, 1))
    }

    fn pred_true() -> Pred {
        Pred {
            predicate: PredRef::None,
            inverted: false,
        }
    }

    #[test]
    fn test_encode_f64sqrt_with_literal_materializes() {
        let labels = FxHashMap::default();
        let op = OpF64Sqrt {
            dst: dst_reg(0),
            src: Src::new_imm_u32(0x1234_5678),
        };
        let mut enc = AmdOpEncoder::new(&labels, 0, 254, 255, 10, 2);
        let result = op.encode(&mut enc);
        assert!(result.is_ok());
        let words = result.unwrap();
        assert!(words.len() >= 3);
    }

    #[test]
    fn test_encode_f64rcp_with_literal_materializes() {
        let labels = FxHashMap::default();
        let op = OpF64Rcp {
            dst: dst_reg(0),
            src: Src::new_imm_u32(0x1234_5678),
        };
        let mut enc = AmdOpEncoder::new(&labels, 0, 254, 255, 10, 2);
        let result = op.encode(&mut enc);
        assert!(result.is_ok());
        let words = result.unwrap();
        assert!(words.len() >= 3);
    }

    #[test]
    fn test_encode_dsetp_with_literal_materializes() {
        let labels = FxHashMap::default();
        let op = OpDSetP {
            dst: Dst::None,
            set_op: PredSetOp::And,
            cmp_op: FloatCmpOp::OrdEq,
            srcs: [
                Src::new_imm_u32(0x1234_5678),
                Src::new_imm_u32(0x1234_5678),
                Src::new_imm_bool(true),
            ],
        };
        let mut enc = AmdOpEncoder::new(&labels, 0, 254, 255, 10, 2);
        let result = op.encode(&mut enc);
        assert!(result.is_ok());
        let words = result.unwrap();
        assert!(words.len() >= 5);
    }

    #[test]
    fn test_encode_fadd_success() {
        let op = OpFAdd {
            dst: dst_reg(0),
            srcs: [vgpr(1), vgpr(2)],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
        };
        let labels = FxHashMap::default();
        let result = encode_amd_op(&Op::FAdd(Box::new(op)), &pred_true(), &labels, 0, 254, 255, 10, 2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_encode_fmnmx_min() {
        let op = OpFMnMx {
            dst: dst_reg(0),
            srcs: [vgpr(1), vgpr(2), Src::new_imm_bool(true)],
            ftz: false,
        };
        let labels = FxHashMap::default();
        let result = encode_amd_op(&Op::FMnMx(Box::new(op)), &pred_true(), &labels, 0, 254, 255, 10, 2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_encode_fmnmx_max() {
        let op = OpFMnMx {
            dst: dst_reg(0),
            srcs: [vgpr(1), vgpr(2), Src::new_imm_bool(false)],
            ftz: false,
        };
        let labels = FxHashMap::default();
        let result = encode_amd_op(&Op::FMnMx(Box::new(op)), &pred_true(), &labels, 0, 254, 255, 10, 2);
        assert!(result.is_ok());
    }
}
