// SPDX-License-Identifier: AGPL-3.0-only
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
mod tests {
    use super::super::reg::AmdRegRef;
    use super::*;

    #[test]
    fn s_endpgm_encoding() {
        let words = encode_s_endpgm();
        assert_eq!(words.len(), 1);
        // s_endpgm = SOPP encoding prefix (0xBF800000) | opcode 1 << 16
        assert_eq!(words[0], 0xBF81_0000);
    }

    #[test]
    fn s_barrier_encoding() {
        let words = encode_s_barrier();
        assert_eq!(words.len(), 1);
        // s_barrier = SOPP prefix | opcode 10 << 16
        assert_eq!(words[0], 0xBF8A_0000);
    }

    #[test]
    fn s_nop_encoding() {
        let words = encode_s_nop(0);
        assert_eq!(words.len(), 1);
        // s_nop 0 = SOPP prefix | opcode 0 << 16 | 0
        assert_eq!(words[0], 0xBF80_0000);
    }

    #[test]
    fn s_waitcnt_encoding() {
        let words = encode_s_waitcnt(0, 0, 0);
        assert_eq!(words.len(), 1);
        // s_waitcnt 0 = SOPP prefix | opcode 12 << 16
        let expected_prefix = 0xBF8C_0000u32;
        assert_eq!(words[0], expected_prefix);
    }

    #[test]
    fn vop3_f64_fma_is_64bit() {
        let dst = AmdRegRef::vgpr_pair(0);
        let words = encode_v_fma_f64(dst, 256, 258, 260);
        assert_eq!(words.len(), 2, "VOP3 should be 2 words");
    }

    #[test]
    fn vop3_encoding_opcode_field() {
        let dst = AmdRegRef::vgpr_pair(4);
        let words = Rdna2Encoder::encode_vop3(
            isa::vop3::V_ADD_F64,
            dst,
            256, // v0
            258, // v2
            0,
        );
        let prefix = (words[0] >> 26) & 0x3F;
        assert_eq!(prefix, 0b11_0101);
        let opcode = (words[0] >> 16) & 0x3FF;
        assert_eq!(opcode, u32::from(isa::vop3::V_ADD_F64));
        let vdst = words[0] & 0xFF;
        assert_eq!(vdst, 4);
    }

    #[test]
    fn vop2_encoding_structure() {
        let dst = AmdRegRef::vgpr(0);
        let vsrc1 = AmdRegRef::vgpr(1);
        let words = Rdna2Encoder::encode_vop2(isa::vop2::V_ADD_F32, dst, 256, vsrc1);
        assert_eq!(words.len(), 1, "VOP2 should be 1 word");
        let opcode = (words[0] >> 25) & 0x3F;
        assert_eq!(opcode, u32::from(isa::vop2::V_ADD_F32));
    }

    #[test]
    fn sop2_encoding_structure() {
        let dst = AmdRegRef::sgpr(0);
        let src0 = AmdRegRef::sgpr(1);
        let src1 = AmdRegRef::sgpr(2);
        let words = Rdna2Encoder::encode_sop2(isa::sop2::S_ADD_U32, dst, src0, src1);
        assert_eq!(words.len(), 1, "SOP2 should be 1 word");
        let prefix = (words[0] >> 30) & 0x3;
        assert_eq!(prefix, 0b10);
    }

    #[test]
    fn vop1_encoding_structure() {
        let dst = AmdRegRef::vgpr(5);
        let words = Rdna2Encoder::encode_vop1(isa::vop1::V_MOV_B32, dst, 256);
        assert_eq!(words.len(), 1, "VOP1 should be 1 word");
        let prefix = (words[0] >> 25) & 0x7F;
        assert_eq!(prefix, 0b011_1111);
    }

    #[test]
    fn vop3_negate_abs_modifiers() {
        let dst = AmdRegRef::vgpr_pair(0);
        let words = Rdna2Encoder::encode_vop3_mod(
            isa::vop3::V_FMA_F64,
            dst,
            256,
            258,
            260,
            [true, false, false],
            [false, true, false],
        );
        assert_eq!(words.len(), 2);
        let abs_bits = (words[0] >> 8) & 0x7;
        assert_eq!(abs_bits, 0b010);
        let neg_bits = (words[1] >> 29) & 0x7;
        assert_eq!(neg_bits, 0b001);
    }

    #[test]
    fn literal_constant_appended() {
        let mut e = Rdna2Encoder::new_32();
        e.set_literal(0xDEAD_BEEF);
        assert_eq!(e.words().len(), 2);
        assert_eq!(e.words()[1], 0xDEAD_BEEF);
    }

    #[test]
    fn minimal_compute_kernel() {
        let mut code = Vec::new();
        // v_mov_b32 v0, 42 (literal)
        let mut mov = Rdna2Encoder::encode_vop1(isa::vop1::V_MOV_B32, AmdRegRef::vgpr(0), 255);
        mov.push(42); // literal constant
        code.extend_from_slice(&mov);
        // s_endpgm
        code.extend_from_slice(&encode_s_endpgm());
        // 3 words: VOP1 + literal + SOPP
        assert_eq!(code.len(), 3);
    }

    // ---- LLVM cross-validation tests ----
    // These expected values are produced by:
    //   echo "<asm>" | llvm-mc --triple=amdgcn--amdpal --mcpu=gfx1030 --show-encoding
    // and converted from little-endian byte arrays to u32 words.

    #[test]
    fn llvm_validated_s_endpgm() {
        // LLVM: [0x00,0x00,0x81,0xbf] = 0xBF810000
        assert_eq!(encode_s_endpgm(), vec![0xBF81_0000]);
    }

    #[test]
    fn llvm_validated_s_barrier() {
        // LLVM: [0x00,0x00,0x8a,0xbf] = 0xBF8A0000
        assert_eq!(encode_s_barrier(), vec![0xBF8A_0000]);
    }

    #[test]
    fn llvm_validated_s_nop_0() {
        // LLVM: [0x00,0x00,0x80,0xbf] = 0xBF800000
        assert_eq!(encode_s_nop(0), vec![0xBF80_0000]);
    }

    #[test]
    fn llvm_validated_s_waitcnt_0() {
        // LLVM: [0x00,0x00,0x8c,0xbf] = 0xBF8C0000
        assert_eq!(encode_s_waitcnt(0, 0, 0), vec![0xBF8C_0000]);
    }

    #[test]
    fn llvm_validated_v_add_f64() {
        // v_add_f64 v[0:1], v[2:3], v[4:5]
        // LLVM: [0x00,0x00,0x64,0xd5, 0x02,0x09,0x02,0x00]
        let words = encode_v_add_f64(AmdRegRef::vgpr_pair(0), 258, 260);
        assert_eq!(words, vec![0xD564_0000, 0x0002_0902]);
    }

    #[test]
    fn llvm_validated_v_fma_f64() {
        // v_fma_f64 v[0:1], v[2:3], v[4:5], v[6:7]
        // LLVM: [0x00,0x00,0x4c,0xd5, 0x02,0x09,0x1a,0x04]
        let words = encode_v_fma_f64(AmdRegRef::vgpr_pair(0), 258, 260, 262);
        assert_eq!(words, vec![0xD54C_0000, 0x041A_0902]);
    }

    #[test]
    fn llvm_validated_v_add_f32() {
        // v_add_f32 v0, v1, v2  (VOP2 with v1 as SRC0, v2 as VSRC1)
        // LLVM: [0x01,0x05,0x00,0x06] = 0x06000501
        // src0 = 256+1 = 257 (v1), vsrc1 = v2 (index 2)
        let words = Rdna2Encoder::encode_vop2(
            isa::vop2::V_ADD_F32,
            AmdRegRef::vgpr(0),
            257, // v1 encoded as 256+1
            AmdRegRef::vgpr(2),
        );
        assert_eq!(words, vec![0x0600_0501]);
    }

    #[test]
    fn llvm_validated_s_add_u32() {
        // s_add_u32 s0, s1, s2
        // LLVM: [0x01,0x02,0x00,0x80] = 0x80000201
        let words = Rdna2Encoder::encode_sop2(
            isa::sop2::S_ADD_U32,
            AmdRegRef::sgpr(0),
            AmdRegRef::sgpr(1),
            AmdRegRef::sgpr(2),
        );
        assert_eq!(words, vec![0x8000_0201]);
    }

    #[test]
    fn llvm_validated_v_mov_b32() {
        // v_mov_b32 v5, v0
        // LLVM: [0x00,0x03,0x0a,0x7e] = 0x7E0A0300
        // src0 = 256+0 = 256 (v0)
        let words = Rdna2Encoder::encode_vop1(
            isa::vop1::V_MOV_B32,
            AmdRegRef::vgpr(5),
            256, // v0
        );
        assert_eq!(words, vec![0x7E0A_0300]);
    }

    #[test]
    fn generated_opcode_table_coverage() {
        use super::super::isa_generated;
        // Verify key opcodes from the generated tables match LLVM-validated values.
        assert_eq!(isa_generated::sopp::S_ENDPGM, 1);
        assert_eq!(isa_generated::sopp::S_BARRIER, 10);
        assert_eq!(isa_generated::sopp::S_WAITCNT, 12);
        assert_eq!(isa_generated::vop3::V_ADD_F64, 356);
        assert_eq!(isa_generated::vop3::V_FMA_F64, 332);
        assert_eq!(isa_generated::vop3::V_MUL_F64, 357);
        assert_eq!(isa_generated::vop1::V_MOV_B32, 1);
        assert_eq!(isa_generated::vop2::V_ADD_F32, 3);
        assert_eq!(isa_generated::sop2::S_ADD_U32, 0);
        assert_eq!(isa_generated::sop1::S_MOV_B32, 3);
    }

    #[test]
    fn generated_table_lookup() {
        use super::super::isa_generated;
        let entry = isa_generated::sopp::lookup(1).expect("S_ENDPGM should exist");
        assert_eq!(entry.name, "S_ENDPGM");
        assert!(entry.is_terminator);
        assert!(!entry.is_branch);

        let branch = isa_generated::sopp::lookup(2).expect("S_BRANCH should exist");
        assert_eq!(branch.name, "S_BRANCH");
        assert!(branch.is_branch);
    }

    /// Exercise every generated `TABLE` + `lookup` (llvm-cov: isa_generated tables
    /// are otherwise unused dead data).
    #[test]
    fn generated_isa_tables_lookup_all_encodings() {
        use super::super::isa;
        use super::super::isa_generated;

        assert_eq!(isa_generated::TOTAL_INSTRUCTIONS, 1446);
        assert_eq!(isa_generated::encoding_bits("ENC_DS"), Some(64));
        assert_eq!(isa_generated::encoding_bits("ENC_VOP3"), Some(64));
        assert_eq!(isa_generated::encoding_bits("ENC_VOP3P"), Some(64));
        assert!(isa_generated::encoding_bits("ENC_UNKNOWN").is_none());

        assert!(isa_generated::ds::lookup(0).is_some());
        assert!(isa_generated::flat::lookup(20).is_some());
        assert!(isa_generated::flat_glbl::lookup(8).is_some());
        assert!(isa_generated::flat_scratch::lookup(8).is_some());
        assert!(isa_generated::mimg::lookup(0).is_some());
        assert!(isa_generated::mtbuf::lookup(0).is_some());
        assert!(isa_generated::mubuf::lookup(0).is_some());
        assert!(isa_generated::smem::lookup(0).is_some());
        assert!(isa_generated::sop1::lookup(3).is_some());
        assert!(isa_generated::sop2::lookup(0).is_some());
        assert!(isa_generated::sopc::lookup(0).is_some());
        assert!(isa_generated::sopk::lookup(0).is_some());
        assert!(isa_generated::vop1::lookup(1).is_some());
        assert!(isa_generated::vop2::lookup(3).is_some());
        assert!(isa_generated::vop3::lookup(356).is_some());
        assert!(isa_generated::vop3p::lookup(0).is_some());
        assert!(isa_generated::vopc::lookup(0).is_some());

        let _vop3_full = isa_generated::vop3::table();
        assert!(!_vop3_full.is_empty());
        let _vopc_full = isa_generated::vopc::table();
        assert!(!_vopc_full.is_empty());

        assert_eq!(
            isa_generated::flat::lookup(20)
                .expect("FLAT_LOAD_DWORD")
                .name,
            "FLAT_LOAD_DWORD"
        );
        assert_eq!(
            isa_generated::ds::lookup(0).expect("DS_ADD_U32").name,
            "DS_ADD_U32"
        );
        assert_eq!(isa::flat::FLAT_LOAD_DWORD, 20);
    }

    #[test]
    fn flat_load_encoding_structure() {
        let words = Rdna2Encoder::encode_flat_load(isa::flat::FLAT_LOAD_DWORD, 0, 5, 0);
        assert_eq!(words.len(), 2, "FLAT is 64-bit");
        let prefix = (words[0] >> 26) & 0x3F;
        assert_eq!(prefix, 0b11_0111, "FLAT encoding prefix");
        let opcode = (words[0] >> 18) & 0x7F;
        assert_eq!(opcode, u32::from(isa::flat::FLAT_LOAD_DWORD));
        let seg = (words[0] >> 14) & 3;
        assert_eq!(seg, 2, "SEG must be GLOBAL (10)");
    }

    #[test]
    fn flat_store_encoding_structure() {
        let words = Rdna2Encoder::encode_flat_store(isa::flat::FLAT_STORE_DWORD, 0, 1, 0);
        assert_eq!(words.len(), 2);
        let opcode = (words[0] >> 18) & 0x7F;
        assert_eq!(opcode, u32::from(isa::flat::FLAT_STORE_DWORD));
        let seg = (words[0] >> 14) & 3;
        assert_eq!(seg, 2, "SEG must be GLOBAL (10)");
    }

    #[test]
    fn flat_atomic_encoding_has_glc() {
        let words = Rdna2Encoder::encode_flat_atomic(isa::flat::FLAT_ATOMIC_ADD, 0, 1, 2, 0);
        assert_eq!(words.len(), 2);
        let glc = (words[0] >> 16) & 1;
        assert_eq!(glc, 1, "GLC must be set for atomic return");
        let seg = (words[0] >> 14) & 3;
        assert_eq!(seg, 2, "SEG must be GLOBAL (10)");
    }

    #[test]
    fn vopc_encoding_structure() {
        let words = Rdna2Encoder::encode_vopc(isa::vopc::V_CMP_EQ_F32, 256, 1);
        assert_eq!(words.len(), 1, "VOPC is 32-bit");
        let prefix = (words[0] >> 25) & 0x7F;
        assert_eq!(prefix, 0b011_1110, "VOPC encoding prefix");
    }

    #[test]
    fn s_branch_encoding() {
        let words = Rdna2Encoder::encode_s_branch(4);
        assert_eq!(words.len(), 1);
        let opcode = (words[0] >> 16) & 0x7F;
        assert_eq!(opcode, u32::from(isa::sopp::S_BRANCH));
        let simm16 = words[0] & 0xFFFF;
        assert_eq!(simm16, 4);
    }

    #[test]
    fn s_cbranch_scc1_encoding() {
        let words = Rdna2Encoder::encode_s_cbranch_scc1(0);
        let opcode = (words[0] >> 16) & 0x7F;
        assert_eq!(opcode, u32::from(isa::sopp::S_CBRANCH_SCC1));
    }

    #[test]
    fn s_cbranch_vccnz_encoding() {
        let words = Rdna2Encoder::encode_s_cbranch_vccnz(-2i16);
        let opcode = (words[0] >> 16) & 0x7F;
        assert_eq!(opcode, u32::from(isa::sopp::S_CBRANCH_VCCNZ));
        let simm16 = words[0] & 0xFFFF;
        assert_eq!(simm16, (-2i16 as u16) as u32);
    }
}
