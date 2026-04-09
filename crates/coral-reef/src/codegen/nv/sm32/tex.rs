// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2025)
//! SM32 texture and surface instruction encoders.
//!
//! Several texture opcodes use bits 32–34 for the phase selector: `0` = none, `1` = `.t`, `2` = `.p`.
//! Encoders here emit `.p` (`0x2`) where applicable.
//!
//! For `txq`, bits 25–31 encode the query kind. Besides the variants
//! handled in code, hardware assigns `Filter` → `0x10`, `Lod` → `0x12`, `Wrap` → `0x14`,
//! `BorderColour` → `0x16` (not modeled in the IR yet).

use super::encoder::*;
use crate::codegen::ir::{IMadSpSrcType, RegFile};

impl SM32Encoder<'_> {
    pub(super) fn set_tex_dim(&mut self, range: Range<usize>, dim: TexDim) {
        assert!(range.len() == 3);
        self.set_field(
            range,
            match dim {
                TexDim::_1D => 0_u8,
                TexDim::Array1D => 1_u8,
                TexDim::_2D => 2_u8,
                TexDim::Array2D => 3_u8,
                TexDim::_3D => 4_u8,
                // 5: array_3d
                TexDim::Cube => 6_u8,
                TexDim::ArrayCube => 7_u8,
            },
        );
    }

    pub(super) fn set_tex_lod_mode(&mut self, range: Range<usize>, lod_mode: TexLodMode) {
        assert!(range.len() == 3);
        self.set_field(
            range,
            match lod_mode {
                TexLodMode::Auto => 0_u8,
                TexLodMode::Zero => 1_u8,
                TexLodMode::Bias => 2_u8,
                TexLodMode::Lod => 3_u8,
                // 6: lba
                // 7: lla
                _ => crate::codegen::ice!("Unknown LOD mode"),
            },
        );
    }

    pub(super) fn set_tex_ndv(&mut self, bit: usize, deriv_mode: TexDerivMode) {
        let ndv = match deriv_mode {
            TexDerivMode::Auto => false,
            TexDerivMode::NonDivergent => true,
            _ => crate::codegen::ice!("{deriv_mode} is not supported"),
        };
        self.set_bit(bit, ndv);
    }
}

/// Helper to legalize texture instructions
pub(super) fn legalize_tex_instr(op: &mut impl SrcsAsSlice, _b: &mut LegalizeBuilder) {
    // Texture instructions have one or two sources.  When they have two, the
    // second one is optional and we can set rZ instead.
    let srcs = op.srcs_as_mut_slice();
    assert!(matches!(&srcs[0].reference, SrcRef::SSA(_)));
    if srcs.len() > 1 {
        debug_assert!(srcs.len() == 2);
        assert!(matches!(&srcs[1].reference, SrcRef::SSA(_) | SrcRef::Zero));
    }
}

impl SM32Op for OpTex {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_opcode(0x600, 1);
                e.set_field(47..60, idx);
            }
            TexRef::CBuf { .. } => {
                crate::codegen::ice!("SM32 doesn't have CBuf textures");
            }
            TexRef::Bindless => {
                e.set_opcode(0x7d8, 2);
            }
        }

        e.set_dst(&self.dsts[0]);
        assert!(self.dsts[1].is_none());
        assert!(self.fault().is_none());
        e.set_reg_src(10..18, &self.srcs[0]);
        e.set_reg_src(23..31, &self.srcs[1]);
        e.set_bit(31, self.nodep);
        e.set_field(32..34, 0x2_u8);

        e.set_field(34..38, self.channel_mask.to_bits());
        e.set_tex_dim(38..41, self.dim);
        e.set_tex_ndv(41, self.deriv_mode);
        e.set_bit(42, self.z_cmpr);
        e.set_bit(43, self.offset_mode == TexOffsetMode::AddOffI);
        e.set_tex_lod_mode(44..47, self.lod_mode);
    }
}

impl SM32Op for OpTld {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_opcode(0x700, 2);
                e.set_field(47..60, idx);
            }
            TexRef::CBuf { .. } => {
                crate::codegen::ice!("SM32 doesn't have CBuf textures");
            }
            TexRef::Bindless => {
                e.set_opcode(0x780, 2);
            }
        }

        e.set_dst(&self.dsts[0]);
        assert!(self.dsts[1].is_none());
        assert!(self.fault().is_none());
        e.set_reg_src(10..18, &self.srcs[0]);
        e.set_reg_src(23..31, &self.srcs[1]);
        e.set_bit(31, self.nodep);
        e.set_field(32..34, 0x2_u8);

        e.set_field(34..38, self.channel_mask.to_bits());
        e.set_tex_dim(38..41, self.dim);
        e.set_bit(41, self.offset_mode == TexOffsetMode::AddOffI);
        e.set_bit(42, false); // z_cmpr
        e.set_bit(43, self.is_ms);

        assert!(matches!(self.lod_mode, TexLodMode::Lod | TexLodMode::Zero));
        e.set_bit(44, self.lod_mode == TexLodMode::Lod);
    }
}

impl SM32Op for OpTld4 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_opcode(0x700, 1);
                e.set_field(47..60, idx);
            }
            TexRef::CBuf { .. } => {
                crate::codegen::ice!("SM32 doesn't have CBuf textures");
            }
            TexRef::Bindless => {
                e.set_opcode(0x7dc, 2);
            }
        }

        e.set_dst(&self.dsts[0]);
        assert!(self.dsts[1].is_none());
        assert!(self.fault().is_none());
        e.set_reg_src(10..18, &self.srcs[0]);
        e.set_reg_src(23..31, &self.srcs[1]);
        e.set_bit(31, self.nodep);
        e.set_field(32..34, 0x2_u8);

        e.set_field(34..38, self.channel_mask.to_bits());
        e.set_tex_dim(38..41, self.dim);
        e.set_bit(42, self.z_cmpr);
        e.set_field(
            43..45,
            match self.offset_mode {
                TexOffsetMode::None => 0_u8,
                TexOffsetMode::AddOffI => 1_u8,
                TexOffsetMode::PerPx => 2_u8,
            },
        );
        e.set_field(45..47, self.comp);
    }
}

impl SM32Op for OpTmml {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_opcode(0x768, 1);
                e.set_field(47..60, idx);
            }
            TexRef::CBuf { .. } => {
                crate::codegen::ice!("SM32 doesn't have CBuf textures");
            }
            TexRef::Bindless => {
                e.set_opcode(0x7e8, 2);
            }
        }

        e.set_dst(&self.dsts[0]);
        assert!(self.dsts[1].is_none());
        e.set_reg_src(10..18, &self.srcs[0]);
        e.set_reg_src(23..31, &self.srcs[1]);
        e.set_bit(31, self.nodep);
        e.set_field(32..34, 0x2_u8);

        e.set_field(34..38, self.channel_mask.to_bits());
        e.set_tex_dim(38..41, self.dim);
        e.set_tex_ndv(41, self.deriv_mode);
    }
}

impl SM32Op for OpTxd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_opcode(0x760, 1);
                e.set_field(47..60, idx);
            }
            TexRef::CBuf { .. } => {
                crate::codegen::ice!("SM32 doesn't have CBuf textures");
            }
            TexRef::Bindless => {
                e.set_opcode(0x7e0, 2);
            }
        }

        e.set_dst(&self.dsts[0]);
        assert!(self.dsts[1].is_none());
        assert!(self.fault().is_none());
        e.set_reg_src(10..18, &self.srcs[0]);
        e.set_reg_src(23..31, &self.srcs[1]);
        e.set_bit(31, self.nodep);
        e.set_field(32..34, 0x2_u8);

        e.set_field(34..38, self.channel_mask.to_bits());
        e.set_tex_dim(38..41, self.dim);
        e.set_bit(54, self.offset_mode == TexOffsetMode::AddOffI);
    }
}

impl SM32Op for OpTxq {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_opcode(0x754, 2);
                e.set_field(41..54, idx);
            }
            TexRef::CBuf { .. } => {
                crate::codegen::ice!("SM32 doesn't have CBuf textures");
            }
            TexRef::Bindless => {
                e.set_opcode(0x7d4, 2);
            }
        }

        e.set_dst(&self.dsts[0]);
        assert!(self.dsts[1].is_none());
        e.set_reg_src(10..18, &self.src);

        e.set_field(
            25..31,
            match self.query {
                TexQuery::Dimension => 1_u8,
                TexQuery::TextureType => 2_u8,
                TexQuery::SamplerPos => 5_u8,
            },
        );
        e.set_bit(31, self.nodep);
        e.set_field(32..34, 0x2_u8);
        e.set_field(34..38, self.channel_mask.to_bits());
    }
}

impl SM32Op for OpSuClamp {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;

        b.copy_alu_src_if_not_reg(self.coords_mut(), GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(self.params_mut(), GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xb00,
            0x180,
            Some(self.dst()),
            self.coords(),
            self.params(),
            None,
            false,
        );

        e.set_field(42..48, self.imm);
        e.set_pred_dst(48..51, self.out_of_bounds());

        e.set_bit(51, self.is_s32);
        let round = match self.round {
            SuClampRound::R1 => 0,
            SuClampRound::R2 => 1,
            SuClampRound::R4 => 2,
            SuClampRound::R8 => 3,
            SuClampRound::R16 => 4,
        };
        let mode = match self.mode {
            SuClampMode::StoredInDescriptor => 0_u8,
            SuClampMode::PitchLinear => 5,
            SuClampMode::BlockLinear => 10,
        };
        e.set_field(52..56, mode + round);
        e.set_bit(56, self.is_2d); // .1d
    }
}

impl SM32Op for OpSuBfm {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;

        b.copy_alu_src_if_not_reg(&mut self.srcs[0], GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(&mut self.srcs[1], GPR, SrcType::ALU);
        b.copy_alu_src_if_not_reg(&mut self.srcs[2], GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xb68,
            0x1e8,
            Some(self.dst()),
            &self.srcs[0],
            &self.srcs[1],
            Some(&self.srcs[2]),
            false,
        );

        e.set_bit(50, self.is_3d);
        e.set_pred_dst(51..54, self.pdst());
    }
}

impl SM32Op for OpSuEau {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;

        b.copy_alu_src_if_not_reg(self.off_mut(), GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(self.bit_field_mut(), GPR, SrcType::ALU);
        b.copy_alu_src_if_not_reg(self.addr_mut(), GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xb6c,
            0x1ec,
            Some(&self.dst),
            self.off(),
            self.bit_field(),
            Some(self.addr()),
            false,
        );
    }
}

impl SM32Op for OpIMadSp {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;

        let [src0, src1, src2] = &mut self.srcs;

        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
        b.copy_alu_src_if_not_reg(src2, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xa40,
            0x140,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            Some(&self.srcs[2]),
            false,
        );

        match self.mode {
            IMadSpMode::Explicit([src0, src1, src2]) => {
                use IMadSpSrcType::*;
                assert!(
                    src2.sign() == (src1.sign() || src0.sign()),
                    "Cannot encode imadsp signed combination"
                );

                e.set_bit(51, src0.sign());
                e.set_field(
                    52..54,
                    match src0.unsigned() {
                        U32 => 0_u8,
                        U24 => 1,
                        U16Lo => 2,
                        U16Hi => 3,
                        S32 | S24 | S16Hi | S16Lo => {
                            unreachable!("src0.unsigned() removes signed IMadSpSrcType variants")
                        }
                    },
                );

                e.set_field(
                    54..56,
                    match src2.unsigned() {
                        U32 => 0_u8,
                        U24 => 1,
                        U16Lo => 2,
                        U16Hi => unreachable!("SM32 legalization rejects IMadSp src2 U16Hi"),
                        _ => unreachable!(
                            "IMadSp src2 unsigned() is U32/U24/U16Lo after legalization"
                        ),
                    },
                );
                e.set_bit(56, src1.sign());

                // Don't trust nvdisasm on this, this is inverted
                e.set_field(
                    57..58,
                    match src1.unsigned() {
                        U24 => 1_u8,
                        U16Lo => 0,
                        _ => unreachable!("SM32 legalization rejects IMadSp src1 non-U16Lo/U24"),
                    },
                );
            }
            IMadSpMode::FromSrc1 => {
                e.set_field(54..56, 3_u8);
            }
        }
    }
}

impl SM32Encoder<'_> {
    pub(super) fn set_ld_cache_op(&mut self, range: Range<usize>, op: LdCacheOp) {
        let cache_op = match op {
            LdCacheOp::CacheAll => 0_u8,
            LdCacheOp::CacheGlobal => 1_u8,
            LdCacheOp::CacheStreaming => 2_u8,
            LdCacheOp::CacheInvalidate => 3_u8,
            LdCacheOp::CacheIncoherent => crate::codegen::ice!("Unsupported cache op: ld{op}"),
        };
        self.set_field(range, cache_op);
    }

    pub(super) fn set_st_cache_op(&mut self, range: Range<usize>, op: StCacheOp) {
        let cache_op = match op {
            StCacheOp::WriteBack => 0_u8,
            StCacheOp::CacheGlobal => 1_u8,
            StCacheOp::CacheStreaming => 2_u8,
            StCacheOp::WriteThrough => 3_u8,
        };
        self.set_field(range, cache_op);
    }

    pub(super) fn set_su_ga_offset_mode(&mut self, range: Range<usize>, off_type: SuGaOffsetMode) {
        assert!(range.len() == 2);
        self.set_field(
            range,
            match off_type {
                SuGaOffsetMode::U32 => 0_u8,
                SuGaOffsetMode::S32 => 1_u8,
                SuGaOffsetMode::U8 => 2_u8,
                SuGaOffsetMode::S8 => 3_u8,
            },
        );
    }
}

impl SM32Op for OpSuLdGa {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        match &self.format().reference {
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x300, 2);
                e.set_mem_type(56..59, self.mem_type);

                e.set_ld_cache_op(54..56, self.cache_op);
                e.set_src_cbuf(23..42, cb);
            }
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x798, 2);
                e.set_mem_type(33..36, self.mem_type);

                e.set_ld_cache_op(31..33, self.cache_op);
                e.set_reg_src(23..31, self.format());
            }
            _ => crate::codegen::ice!("Unhandled format src type"),
        }

        // surface pred: 42..46
        e.set_pred_src(42..46, self.out_of_bounds());

        // Surface clamp:
        // 0: zero
        // 1: trap
        // 3: sdcl
        e.set_field(46..48, 0_u8);

        e.set_su_ga_offset_mode(52..54, self.offset_mode);

        e.set_dst(&self.dst);

        // address
        e.set_reg_src(10..18, self.addr());
    }
}

impl SM32Op for OpSuStGa {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        match &self.format().reference {
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x380, 2);

                // Surface clamp: [ignore, trap, invalid, sdcl]
                e.set_field(2..4, 0_u8);

                match self.image_access {
                    ImageAccess::Binary(mem_type) => {
                        e.set_field(4..8, 0); // channel mask
                        e.set_mem_type(56..59, mem_type); // mem_type
                    }
                    ImageAccess::Formatted(mask) => {
                        e.set_field(4..8, mask.to_bits()); // channel mask
                        e.set_field(56..59, 0_u8); // mem_type
                    }
                }

                e.set_su_ga_offset_mode(8..10, self.offset_mode);
                e.set_src_cbuf(23..42, cb);
                e.set_st_cache_op(54..56, self.cache_op);
            }
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x79c, 2);

                e.set_reg_src(2..10, self.format());

                // Surface clamp: [ignore, trap, invalid, sdcl]
                e.set_field(23..25, 0_u8);

                match self.image_access {
                    ImageAccess::Binary(mem_type) => {
                        e.set_field(25..29, 0); // channel mask
                        e.set_mem_type(33..36, mem_type); // mem_type
                    }
                    ImageAccess::Formatted(mask) => {
                        e.set_field(25..29, mask.to_bits()); // channel mask
                        e.set_field(33..36, 0_u8); // mem_type
                    }
                }

                e.set_su_ga_offset_mode(29..31, self.offset_mode);
                e.set_st_cache_op(31..33, self.cache_op);
            }
            _ => crate::codegen::ice!("Unhandled format src type"),
        }

        // out_of_bounds pred
        e.set_pred_src(50..54, self.out_of_bounds());

        // address
        e.set_reg_src(10..18, self.addr());
        e.set_reg_src(42..50, self.data());
    }
}

// Tests live in `tex_sm32_tests.rs` so this encoder module stays under the 1000-line cap.
#[cfg(test)]
#[path = "tex_sm32_tests.rs"]
mod tests;
