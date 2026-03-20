// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! SM70 texture instruction encoders and helpers.

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
            _ => crate::codegen::ice!("{deriv_mode} is not supported"),
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
                crate::codegen::ice!("SM70+ doesn't have legacy bound textures");
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
                    TexOffsetMode::PerPx => crate::codegen::ice!("Illegal offset value"),
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
                crate::codegen::ice!("SM70+ doesn't have legacy bound textures");
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
                    TexOffsetMode::PerPx => crate::codegen::ice!("Illegal offset value"),
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
                crate::codegen::ice!("SM70+ doesn't have legacy bound textures");
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
                crate::codegen::ice!("SM70+ doesn't have legacy bound textures");
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
                crate::codegen::ice!("SM70+ doesn't have legacy bound textures");
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
                    TexOffsetMode::PerPx => crate::codegen::ice!("Illegal offset value"),
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
                crate::codegen::ice!("SM70+ doesn't have legacy bound textures");
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

#[cfg(test)]
mod tests {
    use super::super::encoder::{SM70Encoder, SM70Op};
    use super::*;
    use bitview::BitViewable;
    use coral_reef_stubs::fxhash::FxHashMap;

    use crate::codegen::ir::{
        ChannelMask, Dst, ImageDim, Label, MemEvictionPriority, RegFile, RegRef, Src, SrcMod,
        SrcSwizzle, TexCBufRef, TexDerivMode, TexDim, TexLodMode, TexOffsetMode, TexQuery, TexRef,
    };

    fn gpr_src(idx: u32) -> Src {
        Src {
            reference: RegRef::new(RegFile::GPR, idx, 1).into(),
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        }
    }

    fn encoder(sm: u8) -> SM70Encoder<'static> {
        let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
        SM70Encoder {
            sm,
            ip: 0,
            labels,
            inst: [0_u32; 4],
        }
    }

    fn opcode(e: &SM70Encoder<'_>) -> u64 {
        e.get_field(0..12)
    }

    #[test]
    fn op_tex_cbuf_sm70_encodes_cb_ref_and_fields() {
        let op = OpTex {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::CBuf(TexCBufRef {
                idx: 3,
                offset: 0x80,
            }),
            srcs: [gpr_src(5), gpr_src(6)],
            dim: TexDim::Array2D,
            lod_mode: TexLodMode::Bias,
            deriv_mode: TexDerivMode::NonDivergent,
            z_cmpr: true,
            offset_mode: TexOffsetMode::AddOffI,
            mem_eviction_priority: MemEvictionPriority::Last,
            nodep: true,
            channel_mask: ChannelMask::new(0b1010),
        };
        let mut e = encoder(70);
        op.encode(&mut e);
        assert_eq!(opcode(&e), 0xb60);
        assert_eq!(e.get_field(40..54), 0x20, "cb.offset / 4");
        assert_eq!(e.get_field(54..59), 3, "cb.idx");
        assert_eq!(e.get_field(61..64), 5, "TexDim::Array2D");
        assert_eq!(e.get_field(72..76), 0xa, "channel mask nibble");
        assert!(e.get_bit(76), "offset_mode AddOffI");
        assert!(e.get_bit(77), "TexDerivMode::NonDivergent (ndv)");
        assert!(e.get_bit(78), "z_cmpr");
        assert_eq!(e.get_field(84..87), 2, "eviction Last");
        assert_eq!(e.get_field(87..90), 2, "TexLodMode::Bias");
        assert!(e.get_bit(90), "nodep");
        assert_eq!(e.get_field(24..32), 5);
        assert_eq!(e.get_field(32..40), 6);
    }

    #[test]
    fn op_tex_cbuf_sm75_matches_sm70_encoding() {
        let op = OpTex {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::CBuf(TexCBufRef {
                idx: 1,
                offset: 0x40,
            }),
            srcs: [gpr_src(1), gpr_src(2)],
            dim: TexDim::_1D,
            lod_mode: TexLodMode::Auto,
            deriv_mode: TexDerivMode::Auto,
            z_cmpr: false,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask: ChannelMask::for_comps(4),
        };
        let mut e75 = encoder(75);
        op.encode(&mut e75);
        let mut e70 = encoder(70);
        op.encode(&mut e70);
        assert_eq!(e75.inst, e70.inst);
    }

    #[test]
    fn op_tex_cbuf_sm89_matches_sm70_encoding() {
        let op = OpTex {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::CBuf(TexCBufRef { idx: 2, offset: 0 }),
            srcs: [gpr_src(0), gpr_src(0)],
            dim: TexDim::Cube,
            lod_mode: TexLodMode::Zero,
            deriv_mode: TexDerivMode::Auto,
            z_cmpr: false,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask: ChannelMask::for_comps(1),
        };
        let mut e89 = encoder(89);
        op.encode(&mut e89);
        let mut e70 = encoder(70);
        op.encode(&mut e70);
        assert_eq!(e89.inst, e70.inst);
    }

    #[test]
    fn op_tex_bindless_sm70_sets_b_bit() {
        let op = OpTex {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bindless,
            srcs: [gpr_src(4), gpr_src(5)],
            dim: TexDim::_2D,
            lod_mode: TexLodMode::Clamp,
            deriv_mode: TexDerivMode::Auto,
            z_cmpr: false,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask: ChannelMask::for_comps(4),
        };
        let mut e = encoder(70);
        op.encode(&mut e);
        assert_eq!(opcode(&e), 0x361);
        assert!(e.get_bit(59), ".B bindless");
    }

    #[test]
    fn op_tex_bindless_sm100_uses_extended_opcode_and_uniform_bit() {
        let op = OpTex {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bindless,
            srcs: [gpr_src(2), gpr_src(3)],
            dim: TexDim::_3D,
            lod_mode: TexLodMode::Lod,
            deriv_mode: TexDerivMode::ForceDivergent,
            z_cmpr: false,
            offset_mode: TexOffsetMode::AddOffI,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask: ChannelMask::for_comps(4),
        };
        let mut e = encoder(100);
        op.encode(&mut e);
        assert_eq!(opcode(&e), 0xd61);
        assert!(e.get_bit(91));
        assert_eq!(e.get_field(56..58), 1, "offset_mode on sm100+");
        assert_eq!(e.get_field(76..78), 2, "TexDerivMode::ForceDivergent");
        assert_eq!(e.get_field(59..60), 1, "lod split low bit for Lod");
        assert_eq!(e.get_field(87..90), 1, "lod split high bits for Lod");
    }

    #[test]
    fn op_tld_cbuf_encodes() {
        let op = OpTld {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::CBuf(TexCBufRef {
                idx: 4,
                offset: 0x100,
            }),
            srcs: [gpr_src(8), gpr_src(9)],
            dim: TexDim::_2D,
            is_ms: true,
            lod_mode: TexLodMode::Lod,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask: ChannelMask::for_comps(3),
        };
        let mut e = encoder(70);
        op.encode(&mut e);
        assert_eq!(opcode(&e), 0xb66);
        assert_eq!(e.get_field(40..54), 0x40);
        assert_eq!(e.get_field(54..59), 4);
        assert!(e.get_bit(78), "is_ms on sm < 120");
        assert_eq!(e.get_field(87..90), 3, "TexLodMode::Lod");
    }

    #[test]
    fn op_tld_bindless_sm70_and_sm100() {
        let op = OpTld {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bindless,
            srcs: [gpr_src(1), gpr_src(2)],
            dim: TexDim::Array1D,
            is_ms: false,
            lod_mode: TexLodMode::Zero,
            offset_mode: TexOffsetMode::AddOffI,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: true,
            channel_mask: ChannelMask::for_comps(2),
        };
        let mut e70 = encoder(70);
        op.encode(&mut e70);
        assert_eq!(opcode(&e70), 0x367);
        assert!(e70.get_bit(59));

        let mut e100 = encoder(100);
        op.encode(&mut e100);
        assert_eq!(opcode(&e100), 0xd67);
        assert!(e100.get_bit(91));
        assert_eq!(e100.get_field(56..58), 1);
    }

    #[test]
    fn op_tld_sm120_is_ms_on_bit_77() {
        let op = OpTld {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bindless,
            srcs: [gpr_src(1), gpr_src(2)],
            dim: TexDim::_2D,
            is_ms: true,
            lod_mode: TexLodMode::Zero,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask: ChannelMask::for_comps(4),
        };
        let mut e = encoder(120);
        op.encode(&mut e);
        assert!(e.get_bit(77), "is_ms on sm >= 120");
    }

    #[test]
    fn op_tld4_offset_modes_none_addoffi_perpx() {
        let base = |offset_mode: TexOffsetMode| OpTld4 {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::CBuf(TexCBufRef { idx: 0, offset: 0 }),
            srcs: [gpr_src(3), gpr_src(4)],
            dim: TexDim::_2D,
            comp: 2,
            offset_mode,
            z_cmpr: false,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask: ChannelMask::for_comps(4),
        };

        let mut e = encoder(70);
        base(TexOffsetMode::None).encode(&mut e);
        assert_eq!(e.get_field(76..78), 0);

        let mut e = encoder(70);
        base(TexOffsetMode::AddOffI).encode(&mut e);
        assert_eq!(e.get_field(76..78), 1);

        let mut e = encoder(70);
        base(TexOffsetMode::PerPx).encode(&mut e);
        assert_eq!(e.get_field(76..78), 2);
    }

    #[test]
    fn op_tmml_cbuf_and_bindless() {
        let op_cb = OpTmml {
            dsts: [Dst::None, Dst::None],
            tex: TexRef::CBuf(TexCBufRef {
                idx: 6,
                offset: 0x20,
            }),
            srcs: [gpr_src(10), gpr_src(11)],
            dim: TexDim::Cube,
            deriv_mode: TexDerivMode::Auto,
            nodep: false,
            channel_mask: ChannelMask::for_comps(4),
        };
        let mut e = encoder(70);
        op_cb.encode(&mut e);
        assert_eq!(opcode(&e), 0xb69);
        assert!(!e.get_bit(77), "ndv off");

        let op_bl = OpTmml {
            dsts: [Dst::None, Dst::None],
            tex: TexRef::Bindless,
            srcs: [gpr_src(1), gpr_src(2)],
            dim: TexDim::_1D,
            deriv_mode: TexDerivMode::NonDivergent,
            nodep: true,
            channel_mask: ChannelMask::for_comps(2),
        };
        let mut e = encoder(70);
        op_bl.encode(&mut e);
        assert_eq!(opcode(&e), 0x36a);
        assert!(e.get_bit(59));
        assert!(e.get_bit(77), "ndv on");
        assert!(e.get_bit(90));
    }

    #[test]
    fn op_tmml_sm100_deriv_mode_field() {
        let op = OpTmml {
            dsts: [Dst::None, Dst::None],
            tex: TexRef::Bindless,
            srcs: [gpr_src(1), gpr_src(2)],
            dim: TexDim::_2D,
            deriv_mode: TexDerivMode::ForceDivergent,
            nodep: false,
            channel_mask: ChannelMask::for_comps(4),
        };
        let mut e = encoder(105);
        op.encode(&mut e);
        assert_eq!(e.get_field(76..78), 2, "ForceDivergent");
    }

    #[test]
    fn op_txd_cbuf_and_bindless() {
        let op_cb = OpTxd {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::CBuf(TexCBufRef {
                idx: 2,
                offset: 0x44,
            }),
            srcs: [gpr_src(12), gpr_src(13)],
            dim: TexDim::ArrayCube,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority: MemEvictionPriority::First,
            nodep: false,
            channel_mask: ChannelMask::for_comps(4),
        };
        let mut e = encoder(70);
        op_cb.encode(&mut e);
        assert_eq!(opcode(&e), 0xb6c);
        assert_eq!(e.get_field(61..64), 7, "TexDim::ArrayCube");

        let op_bl = OpTxd {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bindless,
            srcs: [gpr_src(1), gpr_src(2)],
            dim: TexDim::_2D,
            offset_mode: TexOffsetMode::AddOffI,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: true,
            channel_mask: ChannelMask::for_comps(3),
        };
        let mut e = encoder(70);
        op_bl.encode(&mut e);
        assert_eq!(opcode(&e), 0x36d);
        assert!(e.get_bit(76));
        assert!(e.get_bit(90));

        let mut e = encoder(100);
        op_bl.encode(&mut e);
        assert_eq!(opcode(&e), 0xd6d);
        assert!(e.get_bit(91));
        assert_eq!(e.get_field(56..58), 1);
    }

    #[test]
    fn op_txq_queries_and_cbuf_bindless() {
        for (query, expected) in [
            (TexQuery::Dimension, 0_u64),
            (TexQuery::TextureType, 1),
            (TexQuery::SamplerPos, 2),
        ] {
            let op = OpTxq {
                dsts: [Dst::None, Dst::None],
                tex: TexRef::CBuf(TexCBufRef {
                    idx: 1,
                    offset: 0x10,
                }),
                src: gpr_src(7),
                query,
                nodep: false,
                channel_mask: ChannelMask::for_comps(4),
            };
            let mut e = encoder(70);
            op.encode(&mut e);
            assert_eq!(opcode(&e), 0xb6f);
            assert_eq!(e.get_field(62..64), expected);
        }

        let op_bl = OpTxq {
            dsts: [Dst::None, Dst::None],
            tex: TexRef::Bindless,
            src: gpr_src(3),
            query: TexQuery::Dimension,
            nodep: true,
            channel_mask: ChannelMask::for_comps(1),
        };
        let mut e = encoder(70);
        op_bl.encode(&mut e);
        assert_eq!(opcode(&e), 0x370);
        assert!(e.get_bit(59));
        assert!(e.get_bit(90));
    }

    #[test]
    fn helper_set_tex_dim_lod_channel_ndv_image() {
        let labels = FxHashMap::<Label, usize>::default();
        let mut e = SM70Encoder {
            sm: 70,
            ip: 0,
            labels: &labels,
            inst: [0_u32; 4],
        };
        e.set_tex_dim(10..13, TexDim::ArrayCube);
        assert_eq!(e.get_field(10..13), 7);

        let mut e = SM70Encoder {
            sm: 70,
            ip: 0,
            labels: &labels,
            inst: [0_u32; 4],
        };
        e.set_tex_lod_mode(20..23, TexLodMode::BiasClamp);
        assert_eq!(e.get_field(20..23), 5);

        let mut e = SM70Encoder {
            sm: 70,
            ip: 0,
            labels: &labels,
            inst: [0_u32; 4],
        };
        e.set_tex_channel_mask(30..34, ChannelMask::new(0b0110));
        assert_eq!(e.get_field(30..34), 6);

        let mut e = SM70Encoder {
            sm: 70,
            ip: 0,
            labels: &labels,
            inst: [0_u32; 4],
        };
        e.set_tex_ndv(40, TexDerivMode::Auto);
        assert!(!e.get_bit(40));
        e.set_tex_ndv(41, TexDerivMode::NonDivergent);
        assert!(e.get_bit(41));

        let mut e = SM70Encoder {
            sm: 70,
            ip: 0,
            labels: &labels,
            inst: [0_u32; 4],
        };
        e.set_image_dim(50..53, ImageDim::_2DArray);
        assert_eq!(e.get_field(50..53), 4);

        let mut e = SM70Encoder {
            sm: 70,
            ip: 0,
            labels: &labels,
            inst: [0_u32; 4],
        };
        e.set_image_channel_mask(60..64, ChannelMask::new(0x3));
        assert_eq!(e.get_field(60..64), 3);

        let mut e = SM70Encoder {
            sm: 100,
            ip: 0,
            labels: &labels,
            inst: [0_u32; 4],
        };
        e.set_tex_deriv_mode(10..12, TexDerivMode::NonDivergent);
        assert_eq!(e.get_field(10..12), 1);
    }

    #[test]
    #[should_panic(expected = "legacy bound")]
    fn op_tex_bound_panics() {
        let op = OpTex {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bound(0),
            srcs: [gpr_src(0), gpr_src(0)],
            dim: TexDim::_2D,
            lod_mode: TexLodMode::Auto,
            deriv_mode: TexDerivMode::Auto,
            z_cmpr: false,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask: ChannelMask::for_comps(4),
        };
        let mut e = encoder(70);
        op.encode(&mut e);
    }
}
