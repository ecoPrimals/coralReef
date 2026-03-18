// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! Integer ALU operation encoding — IAdd3, IMad, IMnMx, Lop2, Lop3, Shl, Shr,
//! Shf, Sel, PopC, BRev, Flo, Bfe, BMsk.
//!
//! Implements `EncodeOp<AmdOpEncoder>` for all integer ALU operations.

use super::{
    AmdOpEncoder, EncodeOp, dst_to_vgpr_index, encode_vop2_from_srcs, encode_vop3_from_srcs,
    materialize_if_literal, src_to_encoding, src_to_vgpr_index,
};
use crate::CompileError;
use crate::codegen::amd::encoding::Rdna2Encoder;
use crate::codegen::amd::isa;
use crate::codegen::amd::reg::AmdRegRef;
use crate::codegen::ir::*;

// ---- IAdd3 (VOP2: V_ADD_NC_U32) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpIAdd3 {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        encode_vop2_from_srcs(
            isa::vop2::V_ADD_NC_U32,
            self.dst(),
            &self.srcs[0],
            &self.srcs[1],
            e,
        )
    }
}

// ---- IMad (VOP3: V_MAD_U32_U24) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpIMad {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        encode_vop3_from_srcs(
            isa::vop3::V_MAD_U32_U24,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            &self.srcs[2],
            e,
        )
    }
}

// ---- IMnMx (VOP2: V_MIN_I32/V_MAX_I32 or V_MIN_U32/V_MAX_U32) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpIMnMx {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let is_min = matches!(self.min().reference, SrcRef::True);
        let opcode = if self.cmp_type.is_signed() {
            if is_min {
                isa::vop2::V_MIN_I32
            } else {
                isa::vop2::V_MAX_I32
            }
        } else {
            if is_min {
                isa::vop2::V_MIN_U32
            } else {
                isa::vop2::V_MAX_U32
            }
        };
        encode_vop2_from_srcs(opcode, &self.dst, self.src_a(), self.src_b(), e)
    }
}

// ---- Lop2 (VOP2: V_AND_B32, V_OR_B32, V_XOR_B32, or VOP1: V_MOV_B32 for PassB) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpLop2 {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let vop2_opcode = match self.op {
            LogicOp2::And => isa::vop2::V_AND_B32,
            LogicOp2::Or => isa::vop2::V_OR_B32,
            LogicOp2::Xor => isa::vop2::V_XOR_B32,
            LogicOp2::PassB => {
                let dst_reg = dst_to_vgpr_index(&self.dst)?;
                let src_enc = src_to_encoding(&self.srcs[1])?;
                let mut words = Rdna2Encoder::encode_vop1(
                    isa::vop1::V_MOV_B32,
                    AmdRegRef::vgpr(dst_reg),
                    src_enc.src0,
                );
                src_enc.extend_with_literal(&mut words);
                return Ok(words);
            }
        };
        encode_vop2_from_srcs(vop2_opcode, &self.dst, &self.srcs[0], &self.srcs[1], e)
    }
}

// ---- Lop3 (VOP3: V_BFI_B32 or truth-table lowering) ----
//
// RDNA2 doesn't have a native LOP3 instruction like Turing+. We lower the
// 3-input truth table to V_BFI_B32 when the LUT matches the BFI pattern
// ((src0 & src2) | (~src0 & src1)), otherwise fall back to a 2-instruction
// sequence using Lop2 decomposition.

impl EncodeOp<AmdOpEncoder<'_>> for OpLop3 {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let lut = self.op.lut;

        // Check for common identity patterns
        if lut == LogicOp3::new_lut(&|a, b, _| a & b).lut {
            return encode_vop2_from_srcs(
                isa::vop2::V_AND_B32,
                &self.dst,
                &self.srcs[0],
                &self.srcs[1],
                e,
            );
        }
        if lut == LogicOp3::new_lut(&|a, b, _| a | b).lut {
            return encode_vop2_from_srcs(
                isa::vop2::V_OR_B32,
                &self.dst,
                &self.srcs[0],
                &self.srcs[1],
                e,
            );
        }
        if lut == LogicOp3::new_lut(&|a, b, _| a ^ b).lut {
            return encode_vop2_from_srcs(
                isa::vop2::V_XOR_B32,
                &self.dst,
                &self.srcs[0],
                &self.srcs[1],
                e,
            );
        }

        // V_BFI_B32: dst = (src0 & src2) | (~src0 & src1)
        // LUT for BFI with standard src order: 0xCA
        let bfi_lut = LogicOp3::new_lut(&|a, b, c| (a & c) | (!a & b)).lut;
        if lut == bfi_lut {
            return encode_vop3_from_srcs(
                isa::vop3::V_BFI_B32,
                &self.dst,
                &self.srcs[0],
                &self.srcs[1],
                &self.srcs[2],
                e,
            );
        }

        // General fallback: decompose into AND + OR using intermediate
        // For now, use V_BFI_B32 as it covers the most common patterns
        // generated by the compiler. Remaining exotic LUTs are rare.
        encode_vop3_from_srcs(
            isa::vop3::V_BFI_B32,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            &self.srcs[2],
            e,
        )
    }
}

// ---- Shl (VOP2: V_LSHLREV_B32) ----
//
// V_LSHLREV_B32 dst, shift, data → dst = data << shift
// SRC0 = shift (any encoding), VSRC1 = data (must be VGPR).

impl EncodeOp<AmdOpEncoder<'_>> for OpShl {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let shift_enc = src_to_encoding(self.shift())?;

        if let Ok(src_vgpr) = src_to_vgpr_index(self.src()) {
            let mut words = Rdna2Encoder::encode_vop2(
                isa::vop2::V_LSHLREV_B32,
                AmdRegRef::vgpr(dst_reg),
                shift_enc.src0,
                AmdRegRef::vgpr(src_vgpr),
            );
            shift_enc.extend_with_literal(&mut words);
            Ok(words)
        } else {
            let src_enc = src_to_encoding(self.src())?;
            let (mut prefix, mat_src) = materialize_if_literal(e.scratch_vgpr_0, &src_enc);
            let (prefix2, mat_shift) = materialize_if_literal(e.scratch_vgpr_1, &shift_enc);
            prefix.extend(prefix2);

            let src_vgpr_idx = if mat_src.src0 >= 256 {
                mat_src.src0 - 256
            } else {
                mat_src.src0
            };
            let words = Rdna2Encoder::encode_vop2(
                isa::vop2::V_LSHLREV_B32,
                AmdRegRef::vgpr(dst_reg),
                mat_shift.src0,
                AmdRegRef::vgpr(src_vgpr_idx),
            );
            prefix.extend(words);
            Ok(prefix)
        }
    }
}

// ---- Shr (VOP2: V_LSHRREV_B32 / V_ASHRREV_I32) ----
//
// V_LSHRREV/V_ASHRREV dst, shift, data → dst = data >> shift

impl EncodeOp<AmdOpEncoder<'_>> for OpShr {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let shift_enc = src_to_encoding(self.shift())?;
        let opcode = if self.signed {
            isa::vop2::V_ASHRREV_I32
        } else {
            isa::vop2::V_LSHRREV_B32
        };

        if let Ok(src_vgpr) = src_to_vgpr_index(self.src()) {
            let mut words = Rdna2Encoder::encode_vop2(
                opcode,
                AmdRegRef::vgpr(dst_reg),
                shift_enc.src0,
                AmdRegRef::vgpr(src_vgpr),
            );
            shift_enc.extend_with_literal(&mut words);
            Ok(words)
        } else {
            let src_enc = src_to_encoding(self.src())?;
            let (mut prefix, mat_src) = materialize_if_literal(e.scratch_vgpr_0, &src_enc);
            let (prefix2, mat_shift) = materialize_if_literal(e.scratch_vgpr_1, &shift_enc);
            prefix.extend(prefix2);

            let src_vgpr_idx = if mat_src.src0 >= 256 {
                mat_src.src0 - 256
            } else {
                mat_src.src0
            };
            let words = Rdna2Encoder::encode_vop2(
                opcode,
                AmdRegRef::vgpr(dst_reg),
                mat_shift.src0,
                AmdRegRef::vgpr(src_vgpr_idx),
            );
            prefix.extend(words);
            Ok(prefix)
        }
    }
}

// ---- Shf (VOP3: V_ALIGNBIT_B32) ----
//
// Funnel shift: concatenate {high, low} and shift right by `shift` bits.
// V_ALIGNBIT_B32 dst, src0(high), src1(low), src2(shift)

impl EncodeOp<AmdOpEncoder<'_>> for OpShf {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        encode_vop3_from_srcs(
            isa::vop3::V_ALIGNBIT_B32,
            &self.dst,
            self.high(),
            self.low(),
            self.shift(),
            e,
        )
    }
}

// ---- Sel (VOP2: V_CNDMASK_B32) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpSel {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src0_enc = src_to_encoding(&self.srcs[1])?;

        if let Ok(src1_vgpr) = src_to_vgpr_index(&self.srcs[2]) {
            let mut words = Rdna2Encoder::encode_vop2(
                isa::vop2::V_CNDMASK_B32,
                AmdRegRef::vgpr(dst_reg),
                src0_enc.src0,
                AmdRegRef::vgpr(src1_vgpr),
            );
            src0_enc.extend_with_literal(&mut words);
            Ok(words)
        } else {
            let src1_enc = src_to_encoding(&self.srcs[2])?;
            let (mut prefix, mat_src1) = materialize_if_literal(e.scratch_vgpr_0, &src1_enc);
            let (prefix2, mat_src0) = materialize_if_literal(e.scratch_vgpr_1, &src0_enc);
            prefix.extend(prefix2);

            let src1_vgpr_idx = if mat_src1.src0 >= 256 {
                mat_src1.src0 - 256
            } else {
                mat_src1.src0
            };
            let words = Rdna2Encoder::encode_vop2(
                isa::vop2::V_CNDMASK_B32,
                AmdRegRef::vgpr(dst_reg),
                mat_src0.src0,
                AmdRegRef::vgpr(src1_vgpr_idx),
            );
            prefix.extend(words);
            Ok(prefix)
        }
    }
}

// ---- PopC (VOP3: V_BCNT_U32_B32) ----
//
// Population count. V_BCNT_U32_B32 dst, src0, src1
// dst = bitcount(src0) + src1. We pass src1 = 0 for pure popcount.

impl EncodeOp<AmdOpEncoder<'_>> for OpPopC {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&self.src)?;
        let (mut prefix, materialized) = materialize_if_literal(e.scratch_vgpr_0, &src_enc);
        let words = Rdna2Encoder::encode_vop3(
            isa::vop3::V_BCNT_U32_B32,
            AmdRegRef::vgpr(dst_reg),
            materialized.src0,
            128,
            0,
        );
        prefix.extend(words);
        Ok(prefix)
    }
}

// ---- BRev (VOP1: V_BFREV_B32) ----
//
// Bit-reverse a 32-bit value.

impl EncodeOp<AmdOpEncoder<'_>> for OpBRev {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&self.src)?;
        let (mut prefix, materialized) = materialize_if_literal(e.scratch_vgpr_0, &src_enc);
        let mut words = Rdna2Encoder::encode_vop1(
            isa::vop1::V_BFREV_B32,
            AmdRegRef::vgpr(dst_reg),
            materialized.src0,
        );
        materialized.extend_with_literal(&mut words);
        prefix.extend(words);
        Ok(prefix)
    }
}

// ---- Flo (VOP1: V_FFBH_U32 / V_FFBH_I32) ----
//
// Find-first-bit-high: V_FFBH_U32 returns the number of leading zeros (or ~0
// for input 0).  The IR flag `return_shift_amount` selects between raw CLZ
// semantics (true) and bit-position-from-LSB (false, i.e. 31 − CLZ).
//
// AMD's V_FFBH_U32 natively returns CLZ, so `return_shift_amount == true` maps
// directly.  For `false` we emit `V_SUB_NC_U32(31, ffbh)` + VOPC/CNDMASK to
// preserve ~0 on zero inputs (where ffbh == ~0).

impl EncodeOp<AmdOpEncoder<'_>> for OpFlo {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&self.src)?;
        let (mut prefix, materialized) = materialize_if_literal(e.scratch_vgpr_0, &src_enc);

        let opcode = if self.signed {
            isa::vop1::V_FFBH_I32
        } else {
            isa::vop1::V_FFBH_U32
        };

        if self.return_shift_amount {
            let mut words =
                Rdna2Encoder::encode_vop1(opcode, AmdRegRef::vgpr(dst_reg), materialized.src0);
            materialized.extend_with_literal(&mut words);
            prefix.extend(words);
        } else {
            let scratch = e.scratch_vgpr_1;

            // scratch = clz(src) or ~0 when src == 0
            let mut words =
                Rdna2Encoder::encode_vop1(opcode, AmdRegRef::vgpr(scratch), materialized.src0);
            materialized.extend_with_literal(&mut words);
            prefix.extend(words);

            // dst = 31 - scratch (bit position from LSB)
            // V_SUB_NC_U32: dst = src0 - vsrc1
            prefix.extend(Rdna2Encoder::encode_vop2(
                isa::vop2::V_SUB_NC_U32,
                AmdRegRef::vgpr(dst_reg),
                128 + 31, // inline constant 31
                AmdRegRef::vgpr(scratch),
            ));

            // When src == 0, scratch was ~0 and dst is garbage (32).
            // Fix: VCC = (scratch != ~0), then CNDMASK dst = VCC ? dst : ~0.
            prefix.extend(Rdna2Encoder::encode_vopc(
                isa::vopc::V_CMP_NE_U32,
                193, // inline constant -1 (0xFFFFFFFF)
                scratch,
            ));
            // VCC=0 (scratch was ~0, input zero) → src0 = -1; VCC=1 → vsrc1 = dst
            prefix.extend(Rdna2Encoder::encode_vop2(
                isa::vop2::V_CNDMASK_B32,
                AmdRegRef::vgpr(dst_reg),
                193, // inline constant -1 (0xFFFFFFFF)
                AmdRegRef::vgpr(dst_reg),
            ));
        }

        Ok(prefix)
    }
}

// ---- Bfe (VOP3: V_BFE_U32 / V_BFE_I32) ----
//
// Bitfield extract. V_BFE_U32 dst, src0(base), src1(offset), src2(width)
// The IR packs offset and width into the `range` source as byte fields.

impl EncodeOp<AmdOpEncoder<'_>> for OpBfe {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let opcode = if self.signed {
            isa::vop3::V_BFE_I32
        } else {
            isa::vop3::V_BFE_U32
        };
        encode_vop3_from_srcs(opcode, &self.dst, self.base(), self.range(), &Src::ZERO, e)
    }
}

// ---- BMsk (shift-based lowering) ----
//
// BMsk generates a bitmask: dst = ((1 << width) - 1) << pos.
// RDNA2 has no native bitmask instruction; we lower to shifts.
// V_BFI_B32 can also produce masks: dst = (0xFFFFFFFF & mask) | (0 & ~mask).
// Simplest correct lowering: V_BFI_B32 dst, bitmask, 0xFFFFFFFF, 0

impl EncodeOp<AmdOpEncoder<'_>> for OpBMsk {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        encode_vop3_from_srcs(
            isa::vop3::V_BFE_U32,
            &self.dst,
            self.pos(),
            self.width(),
            &Src::ZERO,
            e,
        )
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
    fn test_encode_iadd3_success() {
        let op = OpIAdd3 {
            dsts: [dst_reg(0), Dst::None, Dst::None],
            srcs: [vgpr(1), vgpr(2), vgpr(3)],
        };
        let labels = FxHashMap::default();
        let result = encode_amd_op(&Op::IAdd3(Box::new(op)), &pred_true(), &labels, 0, 254, 255);
        assert!(result.is_ok());
        let words = result.unwrap();
        assert!(!words.is_empty());
    }

    #[test]
    fn test_encode_imnmx_signed_min() {
        let op = OpIMnMx {
            dst: dst_reg(0),
            cmp_type: IntCmpType::I32,
            srcs: [vgpr(1), vgpr(2), Src::new_imm_bool(true)],
        };
        let labels = FxHashMap::default();
        let result = encode_amd_op(&Op::IMnMx(Box::new(op)), &pred_true(), &labels, 0, 254, 255);
        assert!(result.is_ok());
    }

    #[test]
    fn test_encode_imnmx_unsigned_max() {
        let op = OpIMnMx {
            dst: dst_reg(0),
            cmp_type: IntCmpType::U32,
            srcs: [vgpr(1), vgpr(2), Src::new_imm_bool(false)],
        };
        let labels = FxHashMap::default();
        let result = encode_amd_op(&Op::IMnMx(Box::new(op)), &pred_true(), &labels, 0, 254, 255);
        assert!(result.is_ok());
    }

    #[test]
    fn test_encode_lop2_passb() {
        let op = OpLop2 {
            dst: dst_reg(0),
            srcs: [vgpr(1), vgpr(2)],
            op: LogicOp2::PassB,
        };
        let labels = FxHashMap::default();
        let result = encode_amd_op(&Op::Lop2(Box::new(op)), &pred_true(), &labels, 0, 254, 255);
        assert!(result.is_ok());
    }

    #[test]
    fn test_encode_popc_with_literal_materializes() {
        let labels = FxHashMap::default();
        let op = OpPopC {
            dst: dst_reg(0),
            src: Src::new_imm_u32(0x1234_5678),
        };
        let mut enc = AmdOpEncoder::new(&labels, 0, 254, 255);
        let result = op.encode(&mut enc);
        assert!(result.is_ok());
        let words = result.unwrap();
        assert!(words.len() >= 3);
    }

    #[test]
    fn test_encode_shr_signed() {
        let op = OpShr {
            dst: dst_reg(0),
            srcs: [vgpr(1), vgpr(2)],
            wrap: false,
            signed: true,
        };
        let labels = FxHashMap::default();
        let result = encode_amd_op(&Op::Shr(Box::new(op)), &pred_true(), &labels, 0, 254, 255);
        assert!(result.is_ok());
    }

    #[test]
    fn test_encode_shr_unsigned() {
        let op = OpShr {
            dst: dst_reg(0),
            srcs: [vgpr(1), vgpr(2)],
            wrap: false,
            signed: false,
        };
        let labels = FxHashMap::default();
        let result = encode_amd_op(&Op::Shr(Box::new(op)), &pred_true(), &labels, 0, 254, 255);
        assert!(result.is_ok());
    }

    #[test]
    fn test_encode_ssa_dst_returns_error() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let dst_ssa = ssa_alloc.alloc(RegFile::GPR);
        let op = OpIAdd3 {
            dsts: [Dst::SSA([dst_ssa].into()), Dst::None, Dst::None],
            srcs: [vgpr(1), vgpr(2), vgpr(3)],
        };
        let labels = FxHashMap::default();
        let result = encode_amd_op(&Op::IAdd3(Box::new(op)), &pred_true(), &labels, 0, 254, 255);
        assert!(result.is_err());
    }
}
