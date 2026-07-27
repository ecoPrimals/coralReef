// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! RDNA2 instruction encoding — binary emission for GFX10.3 (Navi 21).
//!
//! AMD RDNA2 instructions use fixed-width encoding formats (32 or 64 bits).
//! Each format has a distinct bit layout defined by AMD's ISA specification.
//!
//! ## Encoding Structure
//!
//! 32-bit formats (SOP1, SOP2, SOPC, SOPK, SOPP, VOP1, VOP2, VOPC):
//! ```text
//! [31        encoding prefix       OP     operand fields       0]
//! ```
//!
//! 64-bit formats (VOP3, SMEM, DS, FLAT, MUBUF, MTBUF, MIMG, EXP):
//! ```text
//! [63            word 1 (high)              32][31     word 0 (low)    0]
//! ```
//!
//! Instructions may be followed by a 32-bit literal constant if any
//! source operand references the literal value (encoding value 255).

use super::isa;
use super::isa::EncodingFormat;
use super::reg::AmdRegRef;

/// Encoder state for building AMD instruction words.
///
/// Analogous to `SM70Encoder` in the NVIDIA backend, but for RDNA2
/// variable-format instructions (32-bit or 64-bit base).
pub struct Rdna2Encoder {
    /// Instruction words being built (1 or 2 base words + optional literal).
    words: Vec<u32>,
}

impl Rdna2Encoder {
    /// Create a new encoder for a 32-bit instruction.
    pub fn new_32() -> Self {
        Self { words: vec![0] }
    }

    /// Create a new encoder for a 64-bit instruction.
    pub fn new_64() -> Self {
        Self { words: vec![0, 0] }
    }

    /// Create a new encoder for the given encoding format.
    pub fn for_format(fmt: EncodingFormat) -> Self {
        match fmt.word_count() {
            1 => Self::new_32(),
            2 => Self::new_64(),
            _ => Self::new_64(),
        }
    }

    /// Set a bit field in word 0 (low word).
    pub fn set_field_w0(&mut self, offset: u32, width: u32, value: u32) {
        let mask = if width >= 32 {
            u32::MAX
        } else {
            (1u32 << width) - 1
        };
        self.words[0] &= !(mask << offset);
        self.words[0] |= (value & mask) << offset;
    }

    /// Set a bit field in word 1 (high word, 64-bit instructions only).
    pub fn set_field_w1(&mut self, offset: u32, width: u32, value: u32) {
        debug_assert!(
            self.words.len() >= 2,
            "word 1 only available for 64-bit encodings"
        );
        let mask = if width >= 32 {
            u32::MAX
        } else {
            (1u32 << width) - 1
        };
        self.words[1] &= !(mask << offset);
        self.words[1] |= (value & mask) << offset;
    }

    /// Append a 32-bit literal constant after the instruction.
    pub fn set_literal(&mut self, value: u32) {
        if self.words.len() <= 2 {
            self.words.push(value);
        } else {
            *self.words.last_mut().expect("words is non-empty") = value;
        }
    }

    /// Get the encoded instruction words.
    pub fn words(&self) -> &[u32] {
        &self.words
    }

    /// Consume the encoder and return the instruction words.
    pub fn into_words(self) -> Vec<u32> {
        self.words
    }

    // ---- SOPP encoding (32-bit) ----
    // [31:23] = 10111111_1 (9-bit encoding prefix)
    // [22:16] = OP (7-bit opcode)
    // [15:0]  = SIMM16 (16-bit signed immediate)

    /// Encode a SOPP instruction (scalar program control).
    pub fn encode_sopp(opcode: u16, simm16: u16) -> Vec<u32> {
        let mut e = Self::new_32();
        e.set_field_w0(23, 9, 0b1_0111_1111);
        e.set_field_w0(16, 7, u32::from(opcode));
        e.set_field_w0(0, 16, u32::from(simm16));
        e.into_words()
    }

    // ---- SOP1 encoding (32-bit) ----
    // [31:23] = 10111110_1 (9-bit encoding prefix)
    // [22:16] = SDST (7-bit destination SGPR)
    // [15:8]  = OP (8-bit opcode)
    // [7:0]   = SSRC0 (8-bit source)

    /// Encode a SOP1 instruction (scalar ALU, 1 source).
    pub fn encode_sop1(opcode: u16, dst: AmdRegRef, src0: AmdRegRef) -> Vec<u32> {
        let mut e = Self::new_32();
        e.set_field_w0(23, 9, 0b1_0111_1101);
        e.set_field_w0(16, 7, u32::from(dst.hw_encoding()));
        e.set_field_w0(8, 8, u32::from(opcode));
        e.set_field_w0(0, 8, u32::from(src0.hw_encoding()));
        e.into_words()
    }

    // ---- SOP2 encoding (32-bit) ----
    // [31:30] = 10 (2-bit encoding prefix)
    // [29:23] = OP (7-bit opcode)
    // [22:16] = SDST (7-bit destination SGPR)
    // [15:8]  = SSRC1 (8-bit source 1)
    // [7:0]   = SSRC0 (8-bit source 0)

    /// Encode a SOP2 instruction (scalar ALU, 2 sources).
    pub fn encode_sop2(opcode: u16, dst: AmdRegRef, src0: AmdRegRef, src1: AmdRegRef) -> Vec<u32> {
        let mut e = Self::new_32();
        e.set_field_w0(30, 2, 0b10);
        e.set_field_w0(23, 7, u32::from(opcode));
        e.set_field_w0(16, 7, u32::from(dst.hw_encoding()));
        e.set_field_w0(8, 8, u32::from(src1.hw_encoding()));
        e.set_field_w0(0, 8, u32::from(src0.hw_encoding()));
        e.into_words()
    }

    // ---- VOP1 encoding (32-bit) ----
    // [31:25] = 0111111 (7-bit encoding prefix)
    // [24:17] = VDST (8-bit destination VGPR)
    // [16:9]  = OP (8-bit opcode)
    // [8:0]   = SRC0 (9-bit source — VGPR, SGPR, constant, or literal)

    /// Encode a VOP1 instruction (vector ALU, 1 source).
    pub fn encode_vop1(opcode: u16, dst: AmdRegRef, src0: u16) -> Vec<u32> {
        let mut e = Self::new_32();
        e.set_field_w0(25, 7, 0b011_1111);
        e.set_field_w0(17, 8, u32::from(dst.index));
        e.set_field_w0(9, 8, u32::from(opcode));
        e.set_field_w0(0, 9, u32::from(src0));
        e.into_words()
    }

    // ---- VOP2 encoding (32-bit) ----
    // [31:31] = ENCODING (1-bit, must be 0 — distinguishes from VOP3/VOPC)
    // [30:25] = OP (6-bit opcode)
    // [24:17] = VDST (8-bit destination VGPR index)
    // [16:9]  = VSRC1 (8-bit source 1, VGPR only)
    // [8:0]   = SRC0 (9-bit source 0 — VGPR/SGPR/constant/literal)

    /// Encode a VOP2 instruction (vector ALU, 2 sources).
    pub fn encode_vop2(opcode: u16, dst: AmdRegRef, src0: u16, vsrc1: AmdRegRef) -> Vec<u32> {
        let mut e = Self::new_32();
        // bit 31 stays 0 (encoding prefix)
        e.set_field_w0(25, 6, u32::from(opcode));
        e.set_field_w0(17, 8, u32::from(dst.index));
        e.set_field_w0(9, 8, u32::from(vsrc1.index));
        e.set_field_w0(0, 9, u32::from(src0));
        e.into_words()
    }

    // ---- VOP3 encoding (64-bit) ----
    // Word 0 (bits [31:0]):
    //   [31:26] = 110101 (6-bit encoding prefix for VOP3a)
    //   [25:16] = OP (10-bit opcode)
    //   [15:11] = CLMP / OP_SEL_HI
    //   [10:8]  = ABS (3-bit absolute value modifiers for src0/1/2)
    //   [7:0]   = VDST (8-bit destination)
    // Word 1 (bits [63:32]):
    //   [31:29] = NEG (3-bit negate modifiers for src0/1/2)
    //   [28:27] = OMOD (2-bit output modifier)
    //   [26:18] = SRC2 (9-bit source 2)
    //   [17:9]  = SRC1 (9-bit source 1)
    //   [8:0]   = SRC0 (9-bit source 0)

    /// Encode a VOP3 instruction (vector ALU, 3 sources with modifiers).
    pub fn encode_vop3(opcode: u16, dst: AmdRegRef, src0: u16, src1: u16, src2: u16) -> Vec<u32> {
        let mut e = Self::new_64();
        // Word 0
        e.set_field_w0(26, 6, 0b11_0101);
        e.set_field_w0(16, 10, u32::from(opcode));
        e.set_field_w0(0, 8, u32::from(dst.index));
        // Word 1
        e.set_field_w1(0, 9, u32::from(src0));
        e.set_field_w1(9, 9, u32::from(src1));
        e.set_field_w1(18, 9, u32::from(src2));
        e.into_words()
    }

    /// Encode a VOP3 with negate/absolute value modifiers.
    pub fn encode_vop3_mod(
        opcode: u16,
        dst: AmdRegRef,
        src0: u16,
        src1: u16,
        src2: u16,
        neg: [bool; 3],
        abs: [bool; 3],
    ) -> Vec<u32> {
        let mut e = Self::new_64();
        // Word 0
        e.set_field_w0(26, 6, 0b11_0101);
        e.set_field_w0(16, 10, u32::from(opcode));
        let abs_bits = u32::from(abs[0]) | (u32::from(abs[1]) << 1) | (u32::from(abs[2]) << 2);
        e.set_field_w0(8, 3, abs_bits);
        e.set_field_w0(0, 8, u32::from(dst.index));
        // Word 1
        e.set_field_w1(0, 9, u32::from(src0));
        e.set_field_w1(9, 9, u32::from(src1));
        e.set_field_w1(18, 9, u32::from(src2));
        let neg_bits = u32::from(neg[0]) | (u32::from(neg[1]) << 1) | (u32::from(neg[2]) << 2);
        e.set_field_w1(29, 3, neg_bits);
        e.into_words()
    }
}

/// Encode `s_endpgm` — program terminator.
pub fn encode_s_endpgm() -> Vec<u32> {
    Rdna2Encoder::encode_sopp(isa::sopp::S_ENDPGM, 0)
}

/// Encode `s_barrier` — workgroup synchronization.
pub fn encode_s_barrier() -> Vec<u32> {
    Rdna2Encoder::encode_sopp(isa::sopp::S_BARRIER, 0)
}

/// Encode `s_waitcnt` with the given wait count fields.
///
/// RDNA2 `s_waitcnt` format: `SIMM16 = {VM_CNT[3:0], EXP_CNT[2:0], LGKM_CNT[5:0]}`
/// where each field saturates at its maximum (meaning "don't wait").
pub fn encode_s_waitcnt(vm_cnt: u8, exp_cnt: u8, lgkm_cnt: u8) -> Vec<u32> {
    let simm16 = u16::from(vm_cnt & 0xF)
        | (u16::from(exp_cnt & 0x7) << 4)
        | (u16::from(lgkm_cnt & 0x3F) << 8);
    Rdna2Encoder::encode_sopp(isa::sopp::S_WAITCNT, simm16)
}

/// Encode `s_nop` with the given delay count.
pub fn encode_s_nop(delay: u16) -> Vec<u32> {
    Rdna2Encoder::encode_sopp(isa::sopp::S_NOP, delay)
}

/// Encode `v_fma_f64` — the workhorse f64 operation for AMD.
///
/// `v_fma_f64 vdst, src0, src1, src2` → `vdst.d = src0.d * src1.d + src2.d`
pub fn encode_v_fma_f64(dst: AmdRegRef, src0: u16, src1: u16, src2: u16) -> Vec<u32> {
    Rdna2Encoder::encode_vop3(isa::vop3::V_FMA_F64, dst, src0, src1, src2)
}

/// Encode `v_add_f64` — f64 addition.
pub fn encode_v_add_f64(dst: AmdRegRef, src0: u16, src1: u16) -> Vec<u32> {
    Rdna2Encoder::encode_vop3(isa::vop3::V_ADD_F64, dst, src0, src1, 0)
}

/// Encode `v_mul_f64` — f64 multiplication.
pub fn encode_v_mul_f64(dst: AmdRegRef, src0: u16, src1: u16) -> Vec<u32> {
    Rdna2Encoder::encode_vop3(isa::vop3::V_MUL_F64, dst, src0, src1, 0)
}

// ---- FLAT encoding (64-bit) ----
// Word 0 (bits [31:0]):
//   [31:26] = 110111 (FLAT encoding prefix)
//   [25:18] = OP (7-bit opcode)
//   [17]    = SLC
//   [16]    = GLC
//   [15:14] = SEG (00=flat, 01=scratch, 10=global)
//   [13]    = LDS
//   [12]    = DLC
//   [11:0]  = OFFSET (12-bit signed)
// Word 1 (bits [63:32]):
//   [63:56] = VDST (8-bit)
//   [55:48] = SADDR (7-bit scalar address, or 0x7F=disabled)
//   [47:40] = DATA (8-bit VGPR data source for stores)
//   [39:32] = ADDR (8-bit VGPR 64-bit address)

impl Rdna2Encoder {
    /// Encode a FLAT/GLOBAL load instruction.
    ///
    /// Uses GLOBAL segment (SEG=10) to bypass flat aperture lookup
    /// and access global memory directly — required for DRM compute dispatch
    /// where the flat aperture may not be configured.
    pub fn encode_flat_load(opcode: u16, addr_vgpr: u16, dst_vgpr: u16, offset: i16) -> Vec<u32> {
        let mut e = Self::new_64();
        e.set_field_w0(26, 6, 0b11_0111);
        e.set_field_w0(18, 7, u32::from(opcode));
        e.set_field_w0(14, 2, 2); // SEG = GLOBAL
        e.set_field_w0(0, 12, (offset as u16 as u32) & 0xFFF);
        // Word 1
        e.set_field_w1(0, 8, u32::from(addr_vgpr));
        e.set_field_w1(16, 7, 0x7F); // SADDR disabled
        e.set_field_w1(24, 8, u32::from(dst_vgpr));
        e.into_words()
    }

    /// Encode a FLAT/GLOBAL store instruction.
    ///
    /// Uses GLOBAL segment (SEG=10) — see `encode_flat_load` rationale.
    pub fn encode_flat_store(opcode: u16, addr_vgpr: u16, data_vgpr: u16, offset: i16) -> Vec<u32> {
        let mut e = Self::new_64();
        e.set_field_w0(26, 6, 0b11_0111);
        e.set_field_w0(18, 7, u32::from(opcode));
        e.set_field_w0(14, 2, 2); // SEG = GLOBAL
        e.set_field_w0(0, 12, (offset as u16 as u32) & 0xFFF);
        // Word 1
        e.set_field_w1(0, 8, u32::from(addr_vgpr));
        e.set_field_w1(8, 8, u32::from(data_vgpr));
        e.set_field_w1(16, 7, 0x7F); // SADDR disabled
        e.into_words()
    }

    /// Encode a FLAT/GLOBAL atomic instruction (returns original value to VDST).
    ///
    /// Uses GLOBAL segment (SEG=10) — see `encode_flat_load` rationale.
    pub fn encode_flat_atomic(
        opcode: u16,
        addr_vgpr: u16,
        data_vgpr: u16,
        dst_vgpr: u16,
        offset: i16,
    ) -> Vec<u32> {
        let mut e = Self::new_64();
        e.set_field_w0(26, 6, 0b11_0111);
        e.set_field_w0(18, 7, u32::from(opcode));
        e.set_field_w0(16, 1, 1); // GLC=1 for return value
        e.set_field_w0(14, 2, 2); // SEG = GLOBAL
        e.set_field_w0(0, 12, (offset as u16 as u32) & 0xFFF);
        // Word 1
        e.set_field_w1(0, 8, u32::from(addr_vgpr));
        e.set_field_w1(8, 8, u32::from(data_vgpr));
        e.set_field_w1(16, 7, 0x7F); // SADDR disabled
        e.set_field_w1(24, 8, u32::from(dst_vgpr));
        e.into_words()
    }

    // ---- VOPC encoding (32-bit) ----
    // [31:25] = 0111110 (7-bit encoding prefix)
    // [24:17] = OP (8-bit opcode)
    // [16:9]  = VSRC1 (8-bit VGPR index)
    // [8:0]   = SRC0 (9-bit source — VGPR/SGPR/const/literal)

    /// Encode a VOPC instruction (vector comparison → VCC).
    pub fn encode_vopc(opcode: u16, src0: u16, vsrc1: u16) -> Vec<u32> {
        let mut e = Self::new_32();
        e.set_field_w0(25, 7, 0b011_1110);
        e.set_field_w0(17, 8, u32::from(opcode));
        e.set_field_w0(9, 8, u32::from(vsrc1));
        e.set_field_w0(0, 9, u32::from(src0));
        e.into_words()
    }

    /// Encode `s_branch` — unconditional relative branch.
    pub fn encode_s_branch(offset_words: i16) -> Vec<u32> {
        Self::encode_sopp(isa::sopp::S_BRANCH, offset_words as u16)
    }

    /// Encode `s_cbranch_scc1` — branch if SCC == 1.
    pub fn encode_s_cbranch_scc1(offset_words: i16) -> Vec<u32> {
        Self::encode_sopp(isa::sopp::S_CBRANCH_SCC1, offset_words as u16)
    }

    /// Encode `s_cbranch_scc0` — branch if SCC == 0.
    pub fn encode_s_cbranch_scc0(offset_words: i16) -> Vec<u32> {
        Self::encode_sopp(isa::sopp::S_CBRANCH_SCC0, offset_words as u16)
    }

    /// Encode `s_cbranch_vccnz` — branch if VCC != 0 (any lane set).
    pub fn encode_s_cbranch_vccnz(offset_words: i16) -> Vec<u32> {
        Self::encode_sopp(isa::sopp::S_CBRANCH_VCCNZ, offset_words as u16)
    }

    /// Encode `s_cbranch_vccz` — branch if VCC == 0 (no lanes set).
    pub fn encode_s_cbranch_vccz(offset_words: i16) -> Vec<u32> {
        Self::encode_sopp(isa::sopp::S_CBRANCH_VCCZ, offset_words as u16)
    }

    /// Encode `s_cbranch_execnz` — branch if EXEC != 0.
    pub fn encode_s_cbranch_execnz(offset_words: i16) -> Vec<u32> {
        Self::encode_sopp(isa::sopp::S_CBRANCH_EXECNZ, offset_words as u16)
    }

    /// Encode `s_cbranch_execz` — branch if EXEC == 0.
    pub fn encode_s_cbranch_execz(offset_words: i16) -> Vec<u32> {
        Self::encode_sopp(isa::sopp::S_CBRANCH_EXECZ, offset_words as u16)
    }

    // ---- SMEM encoding (64-bit) ----
    // Word 0 (bits [31:0]):
    //   [31:26] = 111101 (6-bit SMEM encoding prefix)
    //   [25:18] = OP (8-bit opcode)
    //   [16]    = GLC
    //   [14]    = DLC
    //   [12:6]  = SDATA (7-bit destination SGPR index)
    //   [5:0]   = SBASE (6-bit, SGPR pair index — actual SGPR# >> 1)
    // Word 1 (bits [63:32]):
    //   [63:57] = SOFFSET (7-bit scalar offset register, 0x7F = none)
    //   [52:32] = OFFSET (21-bit unsigned byte offset)

    /// Encode an SMEM instruction (scalar memory load from buffer descriptor).
    pub fn encode_smem(opcode: u16, dst: AmdRegRef, sbase: u16, offset: u32) -> Vec<u32> {
        let mut e = Self::new_64();
        // Word 0
        e.set_field_w0(26, 6, 0b11_1101);
        e.set_field_w0(18, 8, u32::from(opcode));
        e.set_field_w0(6, 7, u32::from(dst.index));
        e.set_field_w0(0, 6, u32::from(sbase));
        // Word 1
        e.set_field_w1(0, 21, offset & 0x1F_FFFF);
        e.set_field_w1(25, 7, 0x7F); // SOFFSET = none
        e.into_words()
    }
}

#[cfg(test)]
#[path = "encoding_tests.rs"]
mod tests;
