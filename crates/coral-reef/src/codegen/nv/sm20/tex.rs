// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2025)
//! SM20 texture instruction encoders.

#![allow(clippy::wildcard_imports)]

use super::encoder::*;

fn legalize_tex_instr(op: &mut impl SrcsAsSlice, _b: &mut LegalizeBuilder) {
    let srcs = op.srcs_as_mut_slice();
    assert!(matches!(&srcs[0].reference, SrcRef::SSA(_)));
    if srcs.len() > 1 {
        debug_assert!(srcs.len() == 2);
        assert!(matches!(&srcs[1].reference, SrcRef::SSA(_) | SrcRef::Zero));
    }
}

impl SM20Op for OpTex {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Tex, 0x20);
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_field(32..40, idx);
                e.set_bit(50, false);
            }
            TexRef::CBuf { .. } => panic!("SM20 doesn't have CBuf textures"),
            TexRef::Bindless => {
                assert!(e.sm.sm() >= 30);
                e.set_field(32..40, 0xff_u8);
                e.set_bit(50, true);
            }
        }
        e.set_field(7..9, 0x2_u8);
        e.set_bit(9, self.nodep);
        e.set_dst(14..20, &self.dsts[0]);
        assert!(self.dsts[1].is_none());
        assert!(self.fault.is_none());
        e.set_reg_src(20..26, &self.srcs[0]);
        e.set_reg_src(26..32, &self.srcs[1]);
        e.set_tex_ndv(45, self.deriv_mode);
        e.set_tex_channel_mask(46..50, self.channel_mask);
        e.set_tex_dim(51..54, self.dim);
        e.set_bit(54, self.offset_mode == TexOffsetMode::AddOffI);
        e.set_bit(56, self.z_cmpr);
        e.set_tex_lod_mode(57..59, self.lod_mode);
    }
}

impl SM20Op for OpTld {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Tex, 0x24);
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_field(32..40, idx);
                e.set_bit(50, false);
            }
            TexRef::CBuf { .. } => panic!("SM20 doesn't have CBuf textures"),
            TexRef::Bindless => {
                assert!(e.sm.sm() >= 30);
                e.set_field(32..40, 0xff_u8);
                e.set_bit(50, true);
            }
        }
        e.set_field(7..9, 0x2_u8);
        e.set_bit(9, self.nodep);
        e.set_dst(14..20, &self.dsts[0]);
        assert!(self.dsts[1].is_none());
        assert!(self.fault.is_none());
        e.set_reg_src(20..26, &self.srcs[0]);
        e.set_reg_src(26..32, &self.srcs[1]);
        e.set_tex_channel_mask(46..50, self.channel_mask);
        e.set_tex_dim(51..54, self.dim);
        e.set_bit(54, self.offset_mode == TexOffsetMode::AddOffI);
        e.set_bit(55, self.is_ms);
        e.set_bit(56, false);
        e.set_field(
            57..58,
            match self.lod_mode {
                TexLodMode::Zero => 0_u8,
                TexLodMode::Lod => 1_u8,
                _ => panic!("Tld does not support {}", self.lod_mode),
            },
        );
    }
}

impl SM20Op for OpTld4 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Tex, 0x28);
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_field(32..40, idx);
                e.set_bit(50, false);
            }
            TexRef::CBuf { .. } => panic!("SM20 doesn't have CBuf textures"),
            TexRef::Bindless => {
                assert!(e.sm.sm() >= 30);
                e.set_field(32..40, 0xff_u8);
                e.set_bit(50, true);
            }
        }
        e.set_field(5..7, self.comp);
        e.set_field(7..9, 0x2_u8);
        e.set_bit(9, self.nodep);
        e.set_dst(14..20, &self.dsts[0]);
        assert!(self.dsts[1].is_none());
        assert!(self.fault.is_none());
        e.set_reg_src(20..26, &self.srcs[0]);
        e.set_reg_src(26..32, &self.srcs[1]);
        e.set_bit(45, false);
        e.set_tex_channel_mask(46..50, self.channel_mask);
        e.set_tex_dim(51..54, self.dim);
        e.set_field(
            54..56,
            match self.offset_mode {
                TexOffsetMode::None => 0_u8,
                TexOffsetMode::AddOffI => 1_u8,
                TexOffsetMode::PerPx => 2_u8,
            },
        );
        e.set_bit(56, self.z_cmpr);
    }
}

impl SM20Op for OpTmml {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Tex, 0x2c);
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_field(32..40, idx);
                e.set_bit(50, false);
            }
            TexRef::CBuf { .. } => panic!("SM20 doesn't have CBuf textures"),
            TexRef::Bindless => {
                assert!(e.sm.sm() >= 30);
                e.set_field(32..40, 0xff_u8);
                e.set_bit(50, true);
            }
        }
        e.set_field(7..9, 0x2_u8);
        e.set_bit(9, self.nodep);
        e.set_dst(14..20, &self.dsts[0]);
        assert!(self.dsts[1].is_none());
        e.set_reg_src(20..26, &self.srcs[0]);
        e.set_reg_src(26..32, &self.srcs[1]);
        e.set_tex_ndv(45, self.deriv_mode);
        e.set_tex_channel_mask(46..50, self.channel_mask);
        e.set_tex_dim(51..54, self.dim);
    }
}

impl SM20Op for OpTxd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Tex, 0x38);
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_field(32..40, idx);
                e.set_bit(50, false);
            }
            TexRef::CBuf { .. } => panic!("SM20 doesn't have CBuf textures"),
            TexRef::Bindless => {
                assert!(e.sm.sm() >= 30);
                e.set_field(32..40, 0xff_u8);
                e.set_bit(50, true);
            }
        }
        e.set_field(7..9, 0x2_u8);
        e.set_bit(9, self.nodep);
        e.set_dst(14..20, &self.dsts[0]);
        assert!(self.dsts[1].is_none());
        e.set_reg_src(20..26, &self.srcs[0]);
        e.set_reg_src(26..32, &self.srcs[1]);
        e.set_tex_channel_mask(46..50, self.channel_mask);
        e.set_tex_dim(51..54, self.dim);
        e.set_bit(54, self.offset_mode == TexOffsetMode::AddOffI);
    }
}

impl SM20Op for OpTxq {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Tex, 0x30);
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_field(32..40, idx);
                e.set_bit(50, false);
            }
            TexRef::CBuf { .. } => panic!("SM20 doesn't have CBuf textures"),
            TexRef::Bindless => {
                assert!(e.sm.sm() >= 30);
                e.set_field(32..40, 0xff_u8);
                e.set_bit(50, true);
            }
        }
        e.set_field(7..9, 0x2_u8);
        e.set_bit(9, self.nodep);
        e.set_dst(14..20, &self.dsts[0]);
        assert!(self.dsts[1].is_none());
        e.set_reg_src(20..26, &self.src);
        e.set_reg_src(26..32, &0.into());
        e.set_tex_channel_mask(46..50, self.channel_mask);
        e.set_field(
            54..57,
            match self.query {
                TexQuery::Dimension => 0_u8,
                TexQuery::TextureType => 1_u8,
                TexQuery::SamplerPos => 2_u8,
            },
        );
    }
}

impl SM20Op for OpTexDepBar {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Tex, 0x3c);
        e.set_field(5..9, 0xf_u8);
        e.set_field(26..32, self.textures_left);
    }
}

impl SM20Op for OpViLd {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Tex, 0x0);
        e.set_dst(14..20, &self.dst);
        e.set_reg_src(20..26, &self.idx);
        e.set_field(26..42, self.off);
    }
}

impl SM20Op for OpPixLd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Tex, 0x4);
        e.set_field(
            5..8,
            match &self.val {
                PixVal::CovMask => 1_u8,
                PixVal::Covered => 2_u8,
                PixVal::Offset => 3_u8,
                PixVal::CentroidOffset => 4_u8,
                PixVal::MyIndex => 5_u8,
                other => panic!("Unsupported PixVal: {other}"),
            },
        );
        e.set_dst(14..20, &self.dst);
        e.set_reg_src(20..26, &0.into());
        e.set_field(26..34, 0_u16);
        e.set_pred_dst(53..56, &Dst::None);
    }
}

impl SM20Op for OpS2R {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Move, 0xb);
        e.set_dst(14..20, &self.dst);
        e.set_field(26..36, self.idx);
    }
}

impl SM20Op for OpVote {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Move, 0x12);
        e.set_field(
            5..7,
            match self.op {
                VoteOp::All => 0_u8,
                VoteOp::Any => 1_u8,
                VoteOp::Eq => 2_u8,
            },
        );
        e.set_dst(14..20, &self.ballot);
        e.set_pred_src(20..24, &self.pred);
        e.set_pred_dst(54..57, &self.vote);
    }
}

impl SM20Op for OpOut {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use crate::codegen::ir::RegFile;
        b.copy_alu_src_if_not_reg(&mut self.handle, RegFile::GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(&mut self.stream, RegFile::GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Tex,
            0x7,
            &self.dst,
            &self.handle,
            &self.stream,
            None,
        );
        e.set_field(
            5..7,
            match self.out_type {
                OutType::Emit => 1_u8,
                OutType::Cut => 2_u8,
                OutType::EmitThenCut => 3_u8,
            },
        );
    }
}
