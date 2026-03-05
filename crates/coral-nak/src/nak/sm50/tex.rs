// Copyright © 2023 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! SM50 texture instruction encoders.

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::encoder::*;

fn legalize_tex_instr(op: &mut impl SrcsAsSlice, _b: &mut LegalizeBuilder) {
    // Texture instructions have one or two sources.  When they have two, the
    // second one is optional and we can set rZ instead.
    let srcs = op.srcs_as_mut_slice();
    assert!(matches!(&srcs[0].src_ref, SrcRef::SSA(_)));
    if srcs.len() > 1 {
        debug_assert!(srcs.len() == 2);
        assert!(matches!(&srcs[1].src_ref, SrcRef::SSA(_) | SrcRef::Zero));
    }
}

impl SM50Op for OpTex {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_opcode(0x0380);
                e.set_field(36..49, idx);
                e.set_bit(54, self.offset_mode == TexOffsetMode::AddOffI);
                e.set_tex_lod_mode(55..57, self.lod_mode);
            }
            TexRef::CBuf { .. } => {
                panic!("SM50 doesn't have CBuf textures");
            }
            TexRef::Bindless => {
                e.set_opcode(0xdeb8);
                e.set_bit(36, self.offset_mode == TexOffsetMode::AddOffI);
                e.set_tex_lod_mode(37..39, self.lod_mode);
            }
        }

        e.set_dst(&self.dsts[0]);
        assert!(self.dsts[1].is_none());
        assert!(self.fault.is_none());
        e.set_reg_src(8..16, &self.srcs[0]);
        e.set_reg_src(20..28, &self.srcs[1]);

        e.set_tex_dim(28..31, self.dim);
        e.set_tex_channel_mask(31..35, self.channel_mask);
        e.set_tex_ndv(35, self.deriv_mode);
        e.set_bit(49, self.nodep);
        e.set_bit(50, self.z_cmpr);
    }
}

impl SM50Op for OpTld {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_opcode(0xdc38);
                e.set_field(36..49, idx);
            }
            TexRef::CBuf { .. } => {
                panic!("SM50 doesn't have CBuf textures");
            }
            TexRef::Bindless => {
                e.set_opcode(0xdd38);
            }
        }

        e.set_dst(&self.dsts[0]);
        assert!(self.dsts[1].is_none());
        assert!(self.fault.is_none());
        e.set_reg_src(8..16, &self.srcs[0]);
        e.set_reg_src(20..28, &self.srcs[1]);

        e.set_tex_dim(28..31, self.dim);
        e.set_tex_channel_mask(31..35, self.channel_mask);
        e.set_bit(35, self.offset_mode == TexOffsetMode::AddOffI);
        e.set_bit(49, self.nodep);
        e.set_bit(50, self.is_ms);

        assert!(self.lod_mode == TexLodMode::Zero || self.lod_mode == TexLodMode::Lod);
        e.set_bit(55, self.lod_mode == TexLodMode::Lod);
    }
}

impl SM50Op for OpTld4 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        let offset_mode = match self.offset_mode {
            TexOffsetMode::None => 0_u8,
            TexOffsetMode::AddOffI => 1_u8,
            TexOffsetMode::PerPx => 2_u8,
        };
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_opcode(0xc838);
                e.set_field(36..49, idx);
                e.set_field(54..56, offset_mode);
                e.set_field(56..58, self.comp);
            }
            TexRef::CBuf { .. } => {
                panic!("SM50 doesn't have CBuf textures");
            }
            TexRef::Bindless => {
                e.set_opcode(0xdef8);
                e.set_field(36..38, offset_mode);
                e.set_field(38..40, self.comp);
            }
        }

        e.set_dst(&self.dsts[0]);
        assert!(self.dsts[1].is_none());
        assert!(self.fault.is_none());
        e.set_reg_src(8..16, &self.srcs[0]);
        e.set_reg_src(20..28, &self.srcs[1]);

        e.set_tex_dim(28..31, self.dim);
        e.set_tex_channel_mask(31..35, self.channel_mask);
        e.set_bit(35, false); // ToDo: NDV
        e.set_bit(49, self.nodep);
        e.set_bit(50, self.z_cmpr);
    }
}

impl SM50Op for OpTmml {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_opcode(0xdf58);
                e.set_field(36..49, idx);
            }
            TexRef::CBuf { .. } => {
                panic!("SM50 doesn't have CBuf textures");
            }
            TexRef::Bindless => {
                e.set_opcode(0xdf60);
            }
        }

        e.set_dst(&self.dsts[0]);
        assert!(self.dsts[1].is_none());
        e.set_reg_src(8..16, &self.srcs[0]);
        e.set_reg_src(20..28, &self.srcs[1]);

        e.set_tex_dim(28..31, self.dim);
        e.set_tex_channel_mask(31..35, self.channel_mask);
        e.set_tex_ndv(35, self.deriv_mode);
        e.set_bit(49, self.nodep);
    }
}

impl SM50Op for OpTxd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_opcode(0xde38);
                e.set_field(36..49, idx);
            }
            TexRef::CBuf { .. } => {
                panic!("SM50 doesn't have CBuf textures");
            }
            TexRef::Bindless => {
                e.set_opcode(0xde78);
            }
        }

        e.set_dst(&self.dsts[0]);
        assert!(self.dsts[1].is_none());
        assert!(self.fault.is_none());
        e.set_reg_src(8..16, &self.srcs[0]);
        e.set_reg_src(20..28, &self.srcs[1]);

        e.set_tex_dim(28..31, self.dim);
        e.set_tex_channel_mask(31..35, self.channel_mask);
        e.set_bit(35, self.offset_mode == TexOffsetMode::AddOffI);
        e.set_bit(49, self.nodep);
    }
}

impl SM50Op for OpTxq {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match self.tex {
            TexRef::Bound(idx) => {
                e.set_opcode(0xdf48);
                e.set_field(36..49, idx);
            }
            TexRef::CBuf { .. } => {
                panic!("SM50 doesn't have CBuf textures");
            }
            TexRef::Bindless => {
                e.set_opcode(0xdf50);
            }
        }

        e.set_dst(&self.dsts[0]);
        assert!(self.dsts[1].is_none());
        e.set_reg_src(8..16, &self.src);

        e.set_field(
            22..28,
            match self.query {
                TexQuery::Dimension => 1_u8,
                TexQuery::TextureType => 2_u8,
                TexQuery::SamplerPos => 5_u8,
                // TexQuery::Filter => 0x10_u8,
                // TexQuery::Lod => 0x12_u8,
                // TexQuery::Wrap => 0x14_u8,
                // TexQuery::BorderColour => 0x16,
            },
        );
        e.set_tex_channel_mask(31..35, self.channel_mask);
        e.set_bit(49, self.nodep);
    }
}
