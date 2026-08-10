// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! PLop3 (predicate logic) AMD RDNA2 encoding.
//!
//! NVIDIA's PLOP3 combines up to 3 predicate inputs via an 8-bit LUT.
//! AMD has no native 3-input predicate logic instruction. We decompose the
//! LUT into SOP2 scalar logic on VCC/SGPR pairs.
//!
//! Common patterns (src2 = True, ops[1] = const false):
//!   - a & b  → `S_AND_B32` vcc_lo, src0, src1
//!   - a | b  → `S_OR_B32`  vcc_lo, src0, src1
//!   - a ^ b  → `S_XOR_B32` vcc_lo, src0, src1
//!   - a &~b  → `S_ANDN2_B32` vcc_lo, src0, src1
//!   - a |~b  → `S_ORN2_B32`  vcc_lo, src0, src1
//!
//! Predicate registers on AMD live in VCC_LO (SGPR 106) for wave32.

use super::{AmdOpEncoder, EncodeOp};
use crate::CompileError;
use crate::codegen::amd::encoding::Rdna2Encoder;
use crate::codegen::amd::isa;
use crate::codegen::amd::reg::AmdRegRef;
use crate::codegen::ir::*;

/// Map a PLop3 predicate source to an SOP2 8-bit SSRC encoding value.
///
/// - Pred register → VCC_LO (106)
/// - True → inline constant −1 (193 = all lanes set)
/// - False → inline constant 0 (128 = no lanes set)
fn plop3_src_to_ssrc(src: &Src) -> Result<u16, CompileError> {
    match &src.reference {
        SrcRef::Reg(reg) if reg.file() == RegFile::Pred || reg.file() == RegFile::UPred => Ok(106),
        SrcRef::True => Ok(193),
        SrcRef::False => Ok(128),
        _ => Err(CompileError::InvalidInput(
            "PLop3 source must be pred register or boolean constant".into(),
        )),
    }
}

/// Map a PLop3 predicate destination to an SOP2 7-bit SDST encoding value.
fn plop3_dst_to_sdst(dst: &Dst) -> Result<u16, CompileError> {
    match dst {
        Dst::Reg(reg) if reg.file() == RegFile::Pred || reg.file() == RegFile::UPred => Ok(106),
        Dst::None => Ok(106),
        _ => Err(CompileError::InvalidInput(
            "PLop3 dest must be pred register or None".into(),
        )),
    }
}

impl EncodeOp<AmdOpEncoder<'_>> for OpPLop3 {
    #[allow(clippy::too_many_lines, reason = "exhaustive LUT pattern matching")]
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let lut = self.ops[0].lut;
        let sdst = plop3_dst_to_sdst(&self.dsts[0])?;
        let dst = AmdRegRef::sgpr(sdst);

        let not_x = LogicOp3::new_lut(&|x, _, _| !x).lut;
        let not_y = LogicOp3::new_lut(&|_, y, _| !y).lut;
        let pass_x = LogicOp3::new_lut(&|x, _, _| x).lut;
        let pass_y = LogicOp3::new_lut(&|_, y, _| y).lut;

        if lut == not_x {
            let ssrc0 = plop3_src_to_ssrc(&self.srcs[0])?;
            return Ok(Rdna2Encoder::encode_sop1(
                isa::sop1::S_NOT_B32,
                dst,
                AmdRegRef::sgpr(ssrc0),
            ));
        }
        if lut == not_y {
            let ssrc1 = plop3_src_to_ssrc(&self.srcs[1])?;
            return Ok(Rdna2Encoder::encode_sop1(
                isa::sop1::S_NOT_B32,
                dst,
                AmdRegRef::sgpr(ssrc1),
            ));
        }
        if lut == pass_x {
            let ssrc0 = plop3_src_to_ssrc(&self.srcs[0])?;
            return Ok(Rdna2Encoder::encode_sop1(
                isa::sop1::S_MOV_B32,
                dst,
                AmdRegRef::sgpr(ssrc0),
            ));
        }
        if lut == pass_y {
            let ssrc1 = plop3_src_to_ssrc(&self.srcs[1])?;
            return Ok(Rdna2Encoder::encode_sop1(
                isa::sop1::S_MOV_B32,
                dst,
                AmdRegRef::sgpr(ssrc1),
            ));
        }
        if lut == 0x00 {
            return Ok(Rdna2Encoder::encode_sop1(
                isa::sop1::S_MOV_B32,
                dst,
                AmdRegRef::sgpr(128),
            ));
        }
        if lut == 0xFF {
            return Ok(Rdna2Encoder::encode_sop1(
                isa::sop1::S_MOV_B32,
                dst,
                AmdRegRef::sgpr(193),
            ));
        }

        let ssrc0 = plop3_src_to_ssrc(&self.srcs[0])?;
        let ssrc1 = plop3_src_to_ssrc(&self.srcs[1])?;
        let src0 = AmdRegRef::sgpr(ssrc0);
        let src1 = AmdRegRef::sgpr(ssrc1);

        let and_lut = LogicOp3::new_lut(&|x, y, _| x & y).lut;
        let or_lut = LogicOp3::new_lut(&|x, y, _| x | y).lut;
        let xor_lut = LogicOp3::new_lut(&|x, y, _| x ^ y).lut;
        let andn2_lut = LogicOp3::new_lut(&|x, y, _| x & !y).lut;
        let orn2_lut = LogicOp3::new_lut(&|x, y, _| x | !y).lut;
        let nand_lut = LogicOp3::new_lut(&|x, y, _| !(x & y)).lut;
        let nor_lut = LogicOp3::new_lut(&|x, y, _| !(x | y)).lut;
        let rev_andn2 = LogicOp3::new_lut(&|x, y, _| !x & y).lut;
        let rev_orn2 = LogicOp3::new_lut(&|x, y, _| !x | y).lut;

        let opcode = if lut == and_lut {
            isa::sop2::S_AND_B32
        } else if lut == or_lut {
            isa::sop2::S_OR_B32
        } else if lut == xor_lut {
            isa::sop2::S_XOR_B32
        } else if lut == andn2_lut {
            isa::sop2::S_ANDN2_B32
        } else if lut == rev_andn2 {
            return Ok(Rdna2Encoder::encode_sop2(
                isa::sop2::S_ANDN2_B32,
                dst,
                src1,
                src0,
            ));
        } else if lut == orn2_lut {
            isa::sop2::S_ORN2_B32
        } else if lut == rev_orn2 {
            return Ok(Rdna2Encoder::encode_sop2(
                isa::sop2::S_ORN2_B32,
                dst,
                src1,
                src0,
            ));
        } else if lut == nand_lut {
            isa::sop2::S_NAND_B32
        } else if lut == nor_lut {
            isa::sop2::S_NOR_B32
        } else {
            return Err(CompileError::NotImplemented(
                format!("AMD PLop3 LUT 0x{lut:02X} requires multi-instruction decomposition")
                    .into(),
            ));
        };

        Ok(Rdna2Encoder::encode_sop2(opcode, dst, src0, src1))
    }
}
