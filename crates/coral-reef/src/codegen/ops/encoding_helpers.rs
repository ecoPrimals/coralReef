// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! Shared operand legalization and VOP2/VOP3/VOPC encoding helpers for AMD ops.

use super::AmdOpEncoder;
use super::gfx9::{vop2_opcode_for_gfx, vopc_opcode_for_gfx9};
use crate::CompileError;
use crate::codegen::amd::encoding::Rdna2Encoder;
use crate::codegen::amd::isa;
use crate::codegen::amd::reg::AmdRegRef;
use crate::codegen::ir::*;

// ---- Shared encoding helpers used across category modules ----

pub fn materialize_if_literal(scratch_vgpr: u16, enc: &SrcEncoding) -> (Vec<u32>, SrcEncoding) {
    if let Some(literal_val) = enc.literal {
        let mut mov =
            Rdna2Encoder::encode_vop1(isa::vop1::V_MOV_B32, AmdRegRef::vgpr(scratch_vgpr), 255);
        mov.push(literal_val);
        (mov, SrcEncoding::inline(256 + scratch_vgpr))
    } else {
        (Vec::new(), SrcEncoding::inline(enc.src0))
    }
}

/// Materialize a literal as an f64 VGPR pair for 64-bit operations.
///
/// When the optimizer collapses an f64 pair `{lo=0, hi=imm}` into a scalar
/// immediate, the encoder must reconstruct the full pair: lo VGPR = 0,
/// hi VGPR = the literal. The scratch pair must be adjacent (N, N+1).
pub fn materialize_f64_if_literal(
    scratch_pair_base: u16,
    enc: &SrcEncoding,
) -> (Vec<u32>, SrcEncoding) {
    if let Some(literal_val) = enc.literal {
        let mut prefix = Rdna2Encoder::encode_vop1(
            isa::vop1::V_MOV_B32,
            AmdRegRef::vgpr(scratch_pair_base),
            128, // inline constant 0
        );
        let mut mov_hi = Rdna2Encoder::encode_vop1(
            isa::vop1::V_MOV_B32,
            AmdRegRef::vgpr(scratch_pair_base + 1),
            255, // literal follows
        );
        mov_hi.push(literal_val);
        prefix.extend(mov_hi);
        (prefix, SrcEncoding::inline(256 + scratch_pair_base))
    } else {
        (Vec::new(), SrcEncoding::inline(enc.src0))
    }
}

pub fn dst_to_vgpr_index(dst: &Dst) -> Result<u16, CompileError> {
    match dst {
        Dst::None => Err(CompileError::InvalidInput("destination is None".into())),
        Dst::Reg(reg) => u16::try_from(reg.base_idx()).map_err(|_| {
            CompileError::InvalidInput(
                format!("VGPR dst index {} exceeds u16", reg.base_idx()).into(),
            )
        }),
        Dst::SSA(_) => Err(CompileError::InvalidInput(
            "SSA destination in encoder (not yet register-allocated)".into(),
        )),
    }
}

pub fn src_to_vgpr_index(src: &Src) -> Result<u16, CompileError> {
    match &src.reference {
        SrcRef::Reg(reg) => u16::try_from(reg.base_idx()).map_err(|_| {
            CompileError::InvalidInput(
                format!("VGPR src index {} exceeds u16", reg.base_idx()).into(),
            )
        }),
        SrcRef::Zero => Ok(0),
        _ => Err(CompileError::InvalidInput(
            "VOP2 VSRC1 must be a VGPR register".into(),
        )),
    }
}

/// Result of encoding a source operand — SRC0 field value + optional literal.
///
/// On RDNA2, the SRC0 field uses inline constants for common values (0..64,
/// -1..-16, common floats). Values outside that range require SRC0=255
/// followed by a literal DWORD in the instruction stream.
pub struct SrcEncoding {
    /// The 9-bit SRC0 field value (SGPR, VGPR, inline constant, or 255 for literal).
    pub src0: u16,
    /// Literal DWORD to append after the instruction word, if SRC0=255.
    pub literal: Option<u32>,
}

impl SrcEncoding {
    pub const fn inline(src0: u16) -> Self {
        Self {
            src0,
            literal: None,
        }
    }
    pub const fn literal(val: u32) -> Self {
        Self {
            src0: 255,
            literal: Some(val),
        }
    }
    /// Append any literal DWORD to the encoded instruction words.
    pub fn extend_with_literal(&self, words: &mut Vec<u32>) {
        if let Some(lit) = self.literal {
            words.push(lit);
        }
    }
}

pub fn src_to_encoding(src: &Src) -> Result<SrcEncoding, CompileError> {
    match &src.reference {
        SrcRef::Reg(reg) => {
            let idx = u16::try_from(reg.base_idx()).map_err(|_| {
                CompileError::InvalidInput(
                    format!("register index {} exceeds u16", reg.base_idx()).into(),
                )
            })?;
            match reg.file() {
                RegFile::GPR => Ok(SrcEncoding::inline(256 + idx)),
                RegFile::UGPR => Ok(SrcEncoding::inline(idx)),
                _ => Ok(SrcEncoding::inline(idx)),
            }
        }
        SrcRef::Zero => Ok(SrcEncoding::inline(128)),
        SrcRef::Imm32(val) => Ok(imm32_to_src_encoding(*val)),
        SrcRef::SSA(_) => Err(CompileError::InvalidInput(
            "SSA source in encoder (not yet register-allocated)".into(),
        )),
        SrcRef::CBuf(cb) => cbuf_to_user_sgpr_encoding(&cb.buf, cb.offset).map(SrcEncoding::inline),
        _ => Ok(SrcEncoding::inline(128)),
    }
}

/// Encode a u32 immediate as an RDNA2 inline constant or literal.
///
/// Inline constant map (no extra DWORD):
///   128     → 0
///   129-192 → 1..64
///   193-208 → -1..-16  (as u32: 0xFFFF_FFFF .. 0xFFFF_FFF0)
/// Everything else requires a literal (SRC0=255 + trailing DWORD).
fn imm32_to_src_encoding(val: u32) -> SrcEncoding {
    match val {
        0 => SrcEncoding::inline(128),
        1..=64 => SrcEncoding::inline(128 + val as u16),
        // -1..-16 in two's complement
        0xFFFF_FFF0..=0xFFFF_FFFF => {
            let neg = val.wrapping_neg(); // 1..16
            SrcEncoding::inline(192 + neg as u16)
        }
        _ => SrcEncoding::literal(val),
    }
}

/// Map a constant buffer reference to an AMD user SGPR encoding value.
///
/// On AMD, constant buffer data is passed via COMPUTE_USER_DATA registers
/// which populate SGPRs s[0..N]. The naga translation lays out storage
/// buffer addresses as: `CBuf::Binding(group)[binding * 8 + component]`.
///
/// Returns the SGPR register index (0..105) suitable for VOP1/VOP2 src fields.
pub fn cbuf_to_user_sgpr_encoding(buf: &CBuf, byte_offset: u16) -> Result<u16, CompileError> {
    let CBuf::Binding(_buf_idx) = buf else {
        return Err(CompileError::NotImplemented(
            "bindless constant buffer access on AMD".into(),
        ));
    };
    // Within a binding group, offsets are laid out sequentially.
    // byte_offset / 4 gives the DWORD (SGPR) index.
    let sgpr_idx = byte_offset / 4;
    Ok(sgpr_idx)
}

/// Encode a VOP2 instruction with automatic operand legalization.
///
/// RDNA2 VOP2 requires VSRC1 to be a VGPR. If `src1` is not a VGPR:
/// 1. Swap operands (valid for commutative ops like add/mul/min/max).
/// 2. Fall back to VOP3 encoding (opcode + 256) which allows any
///    9-bit source in all three operand slots.
pub fn encode_vop2_from_srcs(
    opcode: u16,
    dst: &Dst,
    src0: &Src,
    src1: &Src,
    enc: &AmdOpEncoder<'_>,
) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(dst)?;
    let hw_op = vop2_opcode_for_gfx(opcode, enc.gfx_major);

    let src1_is_vgpr = src_to_vgpr_index(src1).is_ok();
    let src0_is_vgpr = src_to_vgpr_index(src0).is_ok();

    if src1_is_vgpr {
        let src0_enc = src_to_encoding(src0)?;
        let src1_idx = src_to_vgpr_index(src1)?;
        let mut words = Rdna2Encoder::encode_vop2(
            hw_op,
            AmdRegRef::vgpr(dst_reg),
            src0_enc.src0,
            AmdRegRef::vgpr(src1_idx),
        );
        src0_enc.extend_with_literal(&mut words);
        Ok(words)
    } else if src0_is_vgpr {
        let src1_enc = src_to_encoding(src1)?;
        let src0_idx = src_to_vgpr_index(src0)?;
        let mut words = Rdna2Encoder::encode_vop2(
            hw_op,
            AmdRegRef::vgpr(dst_reg),
            src1_enc.src0,
            AmdRegRef::vgpr(src0_idx),
        );
        src1_enc.extend_with_literal(&mut words);
        Ok(words)
    } else {
        let vop3_opcode = vop2_opcode_for_gfx(opcode, enc.gfx_major) + 256;
        let src0_enc = src_to_encoding(src0)?;
        let src1_enc = src_to_encoding(src1)?;
        let (mut prefix0, mat0) = materialize_if_literal(enc.scratch_vgpr_0, &src0_enc);
        let (prefix1, mat1) = materialize_if_literal(enc.scratch_vgpr_1, &src1_enc);
        prefix0.extend(prefix1);
        let words = Rdna2Encoder::encode_vop3(
            vop3_opcode,
            AmdRegRef::vgpr(dst_reg),
            mat0.src0,
            mat1.src0,
            0,
        );
        prefix0.extend(words);
        Ok(prefix0)
    }
}

/// Encode a VOPC comparison with automatic operand legalization.
///
/// RDNA2 VOPC e32 requires VSRC1 to be a VGPR. When that constraint cannot
/// be met, promote to VOP3 encoding which accepts any 9-bit source in all
/// operand slots. VOPC opcodes occupy the 0-255 range of the VOP3 opcode
/// space (no offset).
pub fn encode_vopc_legalized(
    opcode: u16,
    src0: &Src,
    src1: &Src,
    enc: &AmdOpEncoder<'_>,
) -> Result<Vec<u32>, CompileError> {
    let src0_enc = src_to_encoding(src0)?;

    if src_to_vgpr_index(src1).is_ok() {
        let src1_idx = src_to_vgpr_index(src1)?;
        let mut words = Rdna2Encoder::encode_vopc(opcode, src0_enc.src0, src1_idx);
        src0_enc.extend_with_literal(&mut words);
        Ok(words)
    } else {
        let src1_enc = src_to_encoding(src1)?;
        let (mut prefix0, mat0) = materialize_if_literal(enc.scratch_vgpr_0, &src0_enc);
        let (prefix1, mat1) = materialize_if_literal(enc.scratch_vgpr_1, &src1_enc);
        prefix0.extend(prefix1);
        // VOPC opcodes map 1:1 into VOP3 opcode space (0-255 range, no offset).
        let vop3_opcode = if enc.gfx_major < 10 {
            vopc_opcode_for_gfx9(opcode)
        } else {
            opcode
        };
        let words =
            Rdna2Encoder::encode_vop3(vop3_opcode, AmdRegRef::sgpr(106), mat0.src0, mat1.src0, 0);
        prefix0.extend(words);
        Ok(prefix0)
    }
}

pub fn encode_vop3_from_srcs(
    opcode: u16,
    dst: &Dst,
    src0: &Src,
    src1: &Src,
    src2: &Src,
    enc: &AmdOpEncoder<'_>,
) -> Result<Vec<u32>, CompileError> {
    encode_vop3_from_srcs_inner(opcode, dst, src0, src1, src2, enc, false)
}

/// VOP3 encoder for f64 operations — materializes literals as VGPR pairs.
pub fn encode_vop3_f64_from_srcs(
    opcode: u16,
    dst: &Dst,
    src0: &Src,
    src1: &Src,
    src2: &Src,
    enc: &AmdOpEncoder<'_>,
) -> Result<Vec<u32>, CompileError> {
    encode_vop3_from_srcs_inner(opcode, dst, src0, src1, src2, enc, true)
}

fn encode_vop3_from_srcs_inner(
    opcode: u16,
    dst: &Dst,
    src0: &Src,
    src1: &Src,
    src2: &Src,
    enc: &AmdOpEncoder<'_>,
    f64_mode: bool,
) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(dst)?;
    let src0_enc = src_to_encoding(src0)?;
    let src1_enc = src_to_encoding(src1)?;
    let src2_enc = src_to_encoding(src2)?;
    let literal_count = [&src0_enc, &src1_enc, &src2_enc]
        .iter()
        .filter(|e| e.literal.is_some())
        .count();
    if !f64_mode && literal_count > 2 {
        return Err(CompileError::NotImplemented(
            "VOP3: third literal would require additional scratch VGPR".into(),
        ));
    }
    if f64_mode && literal_count > 1 {
        return Err(CompileError::NotImplemented(
            "VOP3 f64: two literals would require 4 scratch VGPRs".into(),
        ));
    }
    let materialize = if f64_mode {
        materialize_f64_if_literal
    } else {
        materialize_if_literal
    };
    let mut next_scratch = enc.scratch_vgpr_0;
    let (mut prefix0, mat0) = if src0_enc.literal.is_some() {
        let (p, m) = materialize(next_scratch, &src0_enc);
        next_scratch = enc.scratch_vgpr_1;
        (p, m)
    } else {
        (Vec::new(), SrcEncoding::inline(src0_enc.src0))
    };
    let (prefix1, mat1) = if src1_enc.literal.is_some() {
        let (p, m) = materialize(next_scratch, &src1_enc);
        next_scratch = enc.scratch_vgpr_1;
        (p, m)
    } else {
        (Vec::new(), SrcEncoding::inline(src1_enc.src0))
    };
    let (prefix2, mat2) = if src2_enc.literal.is_some() {
        materialize(next_scratch, &src2_enc)
    } else {
        (Vec::new(), SrcEncoding::inline(src2_enc.src0))
    };
    prefix0.extend(prefix1);
    prefix0.extend(prefix2);

    let neg = [
        matches!(src0.modifier, SrcMod::FNeg | SrcMod::FNegAbs),
        matches!(src1.modifier, SrcMod::FNeg | SrcMod::FNegAbs),
        matches!(src2.modifier, SrcMod::FNeg | SrcMod::FNegAbs),
    ];
    let abs = [
        matches!(src0.modifier, SrcMod::FAbs | SrcMod::FNegAbs),
        matches!(src1.modifier, SrcMod::FAbs | SrcMod::FNegAbs),
        matches!(src2.modifier, SrcMod::FAbs | SrcMod::FNegAbs),
    ];

    let has_mods = neg.iter().any(|&n| n) || abs.iter().any(|&a| a);
    let words = if has_mods {
        Rdna2Encoder::encode_vop3_mod(
            opcode,
            AmdRegRef::vgpr(dst_reg),
            mat0.src0,
            mat1.src0,
            mat2.src0,
            neg,
            abs,
        )
    } else {
        Rdna2Encoder::encode_vop3(
            opcode,
            AmdRegRef::vgpr(dst_reg),
            mat0.src0,
            mat1.src0,
            mat2.src0,
        )
    };
    prefix0.extend(words);
    Ok(prefix0)
}

#[cfg(test)]
mod amd_encoding_helpers_tests {
    use super::super::gfx9::vop3_promoted_opcode_for_gfx;
    use super::*;

    #[test]
    fn imm32_to_src_encoding_inline_and_literal() {
        let z = imm32_to_src_encoding(0);
        assert_eq!(z.src0, 128);
        assert!(z.literal.is_none());

        let lit = imm32_to_src_encoding(0x1234_5678);
        assert_eq!(lit.src0, 255);
        assert_eq!(lit.literal, Some(0x1234_5678));
    }

    #[test]
    fn vop3_promoted_opcode_for_gfx9_remaps_vop2_base() {
        assert_eq!(
            vop3_promoted_opcode_for_gfx(256 + 3, 9),
            256 + 1,
            "V_ADD_F32 RDNA2 op 3 → GFX9 op 1 inside VOP3+256"
        );
    }
}
