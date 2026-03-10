// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! SM70 texture instruction encoders and helpers.

#![allow(clippy::wildcard_imports)]

use super::encoder::*;

impl SM70Encoder<'_> {
    fn set_tex_cb_ref(&mut self, range: Range<usize>, cb: TexCBufRef) {
        assert!(range.len() == 19);
        let mut v = new_subset(&mut self.inst[..], range.start, range.len());
        assert!(cb.offset % 4 == 0);
        v.set_field(0..14, cb.offset / 4);
        v.set_field(14..19, cb.idx);
    }

    fn set_tex_dim(&mut self, range: Range<usize>, dim: TexDim) {
        assert!(range.len() == 3);
        self.set_field(
            range,
            match dim {
                TexDim::_1D => 0_u8,
                TexDim::Array1D => 4_u8,
                TexDim::_2D => 1_u8,
                TexDim::Array2D => 5_u8,
                TexDim::_3D => 2_u8,
                TexDim::Cube => 3_u8,
                TexDim::ArrayCube => 7_u8,
            },
        );
    }

    fn set_tex_lod_mode(&mut self, range: Range<usize>, lod_mode: TexLodMode) {
        assert!(range.len() == 3);
        assert!(self.sm <= 100);

        self.set_field(
            range,
            match lod_mode {
                TexLodMode::Auto => 0_u8,
                TexLodMode::Zero => 1_u8,
                TexLodMode::Bias => 2_u8,
                TexLodMode::Lod => 3_u8,
                TexLodMode::Clamp => 4_u8,
                TexLodMode::BiasClamp => 5_u8,
            },
        );
    }

    fn set_tex_lod_mode2(
        &mut self,
        range1: Range<usize>,
        range2: Range<usize>,
        lod_mode: TexLodMode,
    ) {
        self.set_field2(
            range1,
            range2,
            match lod_mode {
                TexLodMode::Auto => 0_u8,
                TexLodMode::Zero => 1_u8,
                TexLodMode::Bias => 2_u8,
                TexLodMode::Lod => 3_u8,
                TexLodMode::Clamp => 4_u8,
                TexLodMode::BiasClamp => 5_u8,
            },
        );
    }

    pub(super) fn set_tex_ndv(&mut self, bit: usize, deriv_mode: TexDerivMode) {
        let ndv = match deriv_mode {
            TexDerivMode::Auto => false,
            TexDerivMode::NonDivergent => true,
            _ => panic!("{deriv_mode} is not supported"),
        };
        self.set_bit(bit, ndv);
    }

    fn set_tex_deriv_mode(&mut self, range: Range<usize>, deriv_mode: TexDerivMode) {
        assert!(range.len() == 2);
        assert!(self.sm >= 100);
        self.set_field(
            range,
            match deriv_mode {
                TexDerivMode::Auto => 0_u8,
                TexDerivMode::NonDivergent => 1_u8,
                TexDerivMode::ForceDivergent => {
                    assert!(self.sm >= 100 && self.sm < 110);
                    2_u8
                }
                TexDerivMode::DerivXY => {
                    assert!(self.sm >= 120);
                    3_u8
                }
            },
        );
    }

    pub(super) fn set_image_dim(&mut self, range: Range<usize>, dim: ImageDim) {
        assert!(range.len() == 3);
        self.set_field(
            range,
            match dim {
                ImageDim::_1D => 0_u8,
                ImageDim::_1DBuffer => 1_u8,
                ImageDim::_1DArray => 2_u8,
                ImageDim::_2D => 3_u8,
                ImageDim::_2DArray => 4_u8,
                ImageDim::_3D => 5_u8,
            },
        );
    }

    fn set_tex_channel_mask(&mut self, range: Range<usize>, channel_mask: ChannelMask) {
        self.set_field(range, channel_mask.to_bits());
    }

    pub(super) fn set_image_channel_mask(
        &mut self,
        range: Range<usize>,
        channel_mask: ChannelMask,
    ) {
        assert!(
            channel_mask.to_bits() == 0x1
                || channel_mask.to_bits() == 0x3
                || channel_mask.to_bits() == 0xf
        );
        self.set_field(range, channel_mask.to_bits());
    }
}

fn legalize_tex_instr(op: &mut impl SrcsAsSlice, b: &mut LegalizeBuilder) {
    // Texture instructions have one or two sources.  When they have two, the
    // second one is optional and we can set rZ instead.
    let srcs = op.srcs_as_mut_slice();
    assert!(matches!(&srcs[0].reference, SrcRef::SSA(_)));
    if let SrcRef::SSA(ssa) = &mut srcs[0].reference {
        b.copy_ssa_ref_if_uniform(ssa);
    }
    if srcs.len() > 1 {
        debug_assert!(srcs.len() == 2);
        assert!(matches!(&srcs[1].reference, SrcRef::SSA(_) | SrcRef::Zero));
        if let SrcRef::SSA(ssa) = &mut srcs[1].reference {
            b.copy_ssa_ref_if_uniform(ssa);
        }
    }
}

impl SM70Op for OpTex {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        match self.tex {
            TexRef::Bound(_) => {
                panic!("SM70+ doesn't have legacy bound textures");
            }
            TexRef::CBuf(cb) => {
                assert!(e.sm < 100);
                e.set_opcode(0xb60);
                e.set_tex_cb_ref(40..59, cb);
            }
            TexRef::Bindless => {
                if e.sm >= 100 {
                    e.set_opcode(0xd61);
                    e.set_bit(91, true);
                } else {
                    e.set_opcode(0x361);
                    e.set_bit(59, true); // .B
                }
            }
        }

        e.set_dst(&self.dsts[0]);
        if let Dst::Reg(reg) = self.dsts[1] {
            e.set_reg(64..72, reg);
        } else {
            e.set_field(64..72, 255_u8);
        }
        e.set_pred_dst(81..84, self.fault());

        e.set_reg_src(24..32, &self.srcs[0]);
        e.set_reg_src(32..40, &self.srcs[1]);
        if e.sm >= 100 {
            e.set_ureg_src(40..48, &Src::ZERO); // handle
            e.set_ureg_src(48..56, &Src::ZERO); // offset
        }

        e.set_tex_dim(61..64, self.dim);
        e.set_tex_channel_mask(72..76, self.channel_mask);
        if e.sm >= 100 {
            e.set_field(
                56..58,
                match self.offset_mode {
                    TexOffsetMode::None => 0_u8,
                    TexOffsetMode::AddOffI => 1_u8,
                    TexOffsetMode::PerPx => panic!("Illegal offset value"),
                },
            );
            e.set_tex_deriv_mode(76..78, self.deriv_mode);
        } else {
            e.set_bit(76, self.offset_mode == TexOffsetMode::AddOffI);
            e.set_tex_ndv(77, self.deriv_mode);
        }
        e.set_bit(78, self.z_cmpr);
        e.set_eviction_priority(&self.mem_eviction_priority);
        if e.sm >= 100 {
            e.set_tex_lod_mode2(59..60, 87..90, self.lod_mode);
        } else {
            e.set_tex_lod_mode(87..90, self.lod_mode);
        }
        e.set_bit(90, self.nodep);
    }
}

impl SM70Op for OpTld {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        match self.tex {
            TexRef::Bound(_) => {
                panic!("SM70+ doesn't have legacy bound textures");
            }
            TexRef::CBuf(cb) => {
                assert!(e.sm < 100);
                e.set_opcode(0xb66);
                e.set_tex_cb_ref(40..59, cb);
            }
            TexRef::Bindless => {
                if e.sm >= 100 {
                    e.set_opcode(0xd67);
                    e.set_bit(91, true);
                } else {
                    e.set_opcode(0x367);
                    e.set_bit(59, true); // .B
                }
            }
        }

        e.set_dst(&self.dsts[0]);
        if let Dst::Reg(reg) = self.dsts[1] {
            e.set_reg(64..72, reg);
        } else {
            e.set_field(64..72, 255_u8);
        }
        e.set_pred_dst(81..84, self.fault());

        e.set_reg_src(24..32, &self.srcs[0]);
        e.set_reg_src(32..40, &self.srcs[1]);
        if e.sm >= 100 {
            e.set_ureg_src(40..48, &Src::ZERO); // handle
            e.set_ureg_src(48..56, &Src::ZERO); // offset
        }

        if e.sm >= 100 {
            e.set_field(
                56..58,
                match self.offset_mode {
                    TexOffsetMode::None => 0_u8,
                    TexOffsetMode::AddOffI => 1_u8,
                    TexOffsetMode::PerPx => panic!("Illegal offset value"),
                },
            );
        } else {
            e.set_bit(76, self.offset_mode == TexOffsetMode::AddOffI);
        }
        e.set_tex_dim(61..64, self.dim);
        e.set_tex_channel_mask(72..76, self.channel_mask);

        if e.sm >= 120 {
            // MS vs UMS
            e.set_bit(77, self.is_ms);
        } else {
            // bit 77: .CL
            e.set_bit(78, self.is_ms);
        }
        // bits 79..81: .F16
        e.set_eviction_priority(&self.mem_eviction_priority);
        assert!(self.lod_mode.is_explicit_lod());
        if e.sm >= 100 {
            e.set_tex_lod_mode2(59..60, 87..90, self.lod_mode);
        } else {
            e.set_tex_lod_mode(87..90, self.lod_mode);
        }
        e.set_bit(90, self.nodep);
    }
}

impl SM70Op for OpTld4 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        match self.tex {
            TexRef::Bound(_) => {
                panic!("SM70+ doesn't have legacy bound textures");
            }
            TexRef::CBuf(cb) => {
                assert!(e.sm < 100);
                e.set_opcode(0xb63);
                e.set_tex_cb_ref(40..59, cb);
            }
            TexRef::Bindless => {
                if e.sm >= 100 {
                    e.set_opcode(0xd64);
                    e.set_bit(91, true);
                } else {
                    e.set_opcode(0x364);
                    e.set_bit(59, true); // .B
                }
            }
        }

        e.set_dst(&self.dsts[0]);
        if let Dst::Reg(reg) = self.dsts[1] {
            e.set_reg(64..72, reg);
        } else {
            e.set_field(64..72, 255_u8);
        }
        e.set_pred_dst(81..84, self.fault());

        e.set_reg_src(24..32, &self.srcs[0]);
        e.set_reg_src(32..40, &self.srcs[1]);
        if e.sm >= 100 {
            e.set_ureg_src(40..48, &Src::ZERO); // handle
            e.set_ureg_src(48..56, &Src::ZERO); // offset
        }

        e.set_tex_dim(61..64, self.dim);
        e.set_tex_channel_mask(72..76, self.channel_mask);
        e.set_field(
            76..78,
            match self.offset_mode {
                TexOffsetMode::None => 0_u8,
                TexOffsetMode::AddOffI => 1_u8,
                TexOffsetMode::PerPx => 2_u8,
            },
        );
        // bit 77: .CL
        e.set_bit(78, self.z_cmpr);
        e.set_eviction_priority(&self.mem_eviction_priority);
        e.set_field(87..89, self.comp);
        e.set_bit(90, self.nodep);
    }
}

impl SM70Op for OpTmml {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        match self.tex {
            TexRef::Bound(_) => {
                panic!("SM70+ doesn't have legacy bound textures");
            }
            TexRef::CBuf(cb) => {
                assert!(e.sm < 100);
                e.set_opcode(0xb69);
                e.set_tex_cb_ref(40..59, cb);
            }
            TexRef::Bindless => {
                e.set_opcode(0x36a);
                e.set_bit(59, true); // .B
            }
        }

        e.set_dst(&self.dsts[0]);
        if let Dst::Reg(reg) = self.dsts[1] {
            e.set_reg(64..72, reg);
        } else {
            e.set_field(64..72, 255_u8);
        }

        e.set_reg_src(24..32, &self.srcs[0]);
        e.set_reg_src(32..40, &self.srcs[1]);
        if e.sm >= 100 {
            e.set_ureg_src(40..48, &Src::ZERO); // handle
        }

        e.set_tex_dim(61..64, self.dim);
        e.set_tex_channel_mask(72..76, self.channel_mask);
        if e.sm >= 100 {
            e.set_tex_deriv_mode(76..78, self.deriv_mode);
        } else {
            e.set_tex_ndv(77, self.deriv_mode);
        }
        e.set_bit(90, self.nodep);
    }
}

impl SM70Op for OpTxd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        match self.tex {
            TexRef::Bound(_) => {
                panic!("SM70+ doesn't have legacy bound textures");
            }
            TexRef::CBuf(cb) => {
                assert!(e.sm < 100);
                e.set_opcode(0xb6c);
                e.set_tex_cb_ref(40..59, cb);
            }
            TexRef::Bindless => {
                if e.sm >= 100 {
                    e.set_opcode(0xd6d);
                    e.set_bit(91, true);
                } else {
                    e.set_opcode(0x36d);
                    e.set_bit(59, true); // .B
                }
            }
        }

        e.set_dst(&self.dsts[0]);
        if let Dst::Reg(reg) = self.dsts[1] {
            e.set_reg(64..72, reg);
        } else {
            e.set_field(64..72, 255_u8);
        }
        e.set_pred_dst(81..84, self.fault());

        e.set_reg_src(24..32, &self.srcs[0]);
        e.set_reg_src(32..40, &self.srcs[1]);
        if e.sm >= 100 {
            e.set_ureg_src(40..48, &Src::ZERO); // handle
            e.set_ureg_src(48..56, &Src::ZERO); // offset
        }

        if e.sm >= 100 {
            e.set_field(
                56..58,
                match self.offset_mode {
                    TexOffsetMode::None => 0_u8,
                    TexOffsetMode::AddOffI => 1_u8,
                    TexOffsetMode::PerPx => panic!("Illegal offset value"),
                },
            );
        } else {
            e.set_bit(76, self.offset_mode == TexOffsetMode::AddOffI);
        }
        e.set_tex_dim(61..64, self.dim);
        e.set_tex_channel_mask(72..76, self.channel_mask);

        e.set_eviction_priority(&self.mem_eviction_priority);
        e.set_bit(90, self.nodep);
    }
}

impl SM70Op for OpTxq {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_tex_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        match self.tex {
            TexRef::Bound(_) => {
                panic!("SM70+ doesn't have legacy bound textures");
            }
            TexRef::CBuf(cb) => {
                assert!(e.sm < 100);
                e.set_opcode(0xb6f);
                e.set_tex_cb_ref(40..59, cb);
            }
            TexRef::Bindless => {
                e.set_opcode(0x370);
                e.set_bit(59, true); // .B
            }
        }

        e.set_dst(&self.dsts[0]);
        if let Dst::Reg(reg) = self.dsts[1] {
            e.set_reg(64..72, reg);
        } else {
            e.set_field(64..72, 255_u8);
        }

        e.set_reg_src(24..32, &self.src);
        if e.sm >= 100 {
            e.set_ureg_src(40..48, &Src::ZERO); // handle
        }

        e.set_field(
            62..64,
            match self.query {
                TexQuery::Dimension => 0_u8,
                TexQuery::TextureType => 1_u8,
                TexQuery::SamplerPos => 2_u8,
            },
        );
        e.set_tex_channel_mask(72..76, self.channel_mask);
        e.set_bit(90, self.nodep);
    }
}
