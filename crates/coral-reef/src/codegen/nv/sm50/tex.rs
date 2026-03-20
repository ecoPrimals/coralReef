// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)
//! SM50 texture instruction encoders.

use super::encoder::*;

fn legalize_tex_instr(op: &mut impl SrcsAsSlice, _b: &mut LegalizeBuilder) {
    // Texture instructions have one or two sources.  When they have two, the
    // second one is optional and we can set rZ instead.
    let srcs = op.srcs_as_mut_slice();
    assert!(matches!(&srcs[0].reference, SrcRef::SSA(_)));
    if srcs.len() > 1 {
        debug_assert!(srcs.len() == 2);
        assert!(matches!(&srcs[1].reference, SrcRef::SSA(_) | SrcRef::Zero));
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
                crate::codegen::ice!("SM50 doesn't have CBuf textures");
            }
            TexRef::Bindless => {
                e.set_opcode(0xdeb8);
                e.set_bit(36, self.offset_mode == TexOffsetMode::AddOffI);
                e.set_tex_lod_mode(37..39, self.lod_mode);
            }
        }

        e.set_dst(&self.dsts[0]);
        assert!(self.dsts[1].is_none());
        assert!(self.fault().is_none());
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
                crate::codegen::ice!("SM50 doesn't have CBuf textures");
            }
            TexRef::Bindless => {
                e.set_opcode(0xdd38);
            }
        }

        e.set_dst(&self.dsts[0]);
        assert!(self.dsts[1].is_none());
        assert!(self.fault().is_none());
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
                crate::codegen::ice!("SM50 doesn't have CBuf textures");
            }
            TexRef::Bindless => {
                e.set_opcode(0xdef8);
                e.set_field(36..38, offset_mode);
                e.set_field(38..40, self.comp);
            }
        }

        e.set_dst(&self.dsts[0]);
        assert!(self.dsts[1].is_none());
        assert!(self.fault().is_none());
        e.set_reg_src(8..16, &self.srcs[0]);
        e.set_reg_src(20..28, &self.srcs[1]);

        e.set_tex_dim(28..31, self.dim);
        e.set_tex_channel_mask(31..35, self.channel_mask);
        e.set_bit(35, false); // NDV: no derivative (gradient sampling disabled)
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
                crate::codegen::ice!("SM50 doesn't have CBuf textures");
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
                crate::codegen::ice!("SM50 doesn't have CBuf textures");
            }
            TexRef::Bindless => {
                e.set_opcode(0xde78);
            }
        }

        e.set_dst(&self.dsts[0]);
        assert!(self.dsts[1].is_none());
        assert!(self.fault().is_none());
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
                crate::codegen::ice!("SM50 doesn't have CBuf textures");
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

#[cfg(test)]
mod tests {
    use super::super::encoder::*;
    use bitview::BitViewable;
    use coral_reef_stubs::fxhash::FxHashMap;

    use crate::codegen::ir::{
        ChannelMask, Dst, MemEvictionPriority, RegFile, RegRef, Src, SrcMod, SrcSwizzle,
        TexDerivMode, TexDim, TexLodMode, TexOffsetMode, TexQuery, TexRef,
    };

    fn gpr_src(idx: u32) -> Src {
        Src {
            reference: RegRef::new(RegFile::GPR, idx, 1).into(),
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        }
    }

    fn encoder_sm50() -> SM50Encoder<'static> {
        let sm: &'static ShaderModel50 = Box::leak(Box::new(ShaderModel50::new(50)));
        let labels: &'static FxHashMap<crate::codegen::ir::Label, usize> =
            Box::leak(Box::new(FxHashMap::default()));
        SM50Encoder {
            sm,
            ip: 0,
            labels,
            inst: [0_u32; 2],
            sched: 0,
        }
    }

    fn sm50_opcode_clean(e: &SM50Encoder<'_>) -> u64 {
        e.get_field(48..64)
    }

    fn base_tex_fields() -> (ChannelMask, [Src; 2]) {
        (ChannelMask::for_comps(4), [gpr_src(1), gpr_src(2)])
    }

    #[test]
    fn helper_set_tex_dim_lod_channel_ndv_sm50() {
        let sm: &'static ShaderModel50 = Box::leak(Box::new(ShaderModel50::new(50)));
        let labels: &'static FxHashMap<crate::codegen::ir::Label, usize> =
            Box::leak(Box::new(FxHashMap::default()));
        let mut e = SM50Encoder {
            sm,
            ip: 0,
            labels,
            inst: [0_u32; 2],
            sched: 0,
        };
        e.set_tex_dim(10..13, TexDim::ArrayCube);
        assert_eq!(e.get_field(10..13), 7);
        e.set_tex_lod_mode(20..22, TexLodMode::Bias);
        assert_eq!(e.get_field(20..22), 2);
        e.set_tex_channel_mask(30..34, ChannelMask::new(0b1100));
        assert_eq!(e.get_field(30..34), 0xc);
        e.set_tex_ndv(40, TexDerivMode::Auto);
        assert!(!e.get_bit(40));
        e.set_tex_ndv(41, TexDerivMode::NonDivergent);
        assert!(e.get_bit(41));
    }

    #[test]
    #[should_panic(expected = "internal compiler error")]
    fn helper_set_tex_lod_unknown_panics() {
        let sm: &'static ShaderModel50 = Box::leak(Box::new(ShaderModel50::new(50)));
        let labels: &'static FxHashMap<crate::codegen::ir::Label, usize> =
            Box::leak(Box::new(FxHashMap::default()));
        let mut e = SM50Encoder {
            sm,
            ip: 0,
            labels,
            inst: [0_u32; 2],
            sched: 0,
        };
        e.set_tex_lod_mode(0..2, TexLodMode::Clamp);
    }

    #[test]
    #[should_panic(expected = "internal compiler error")]
    fn helper_set_tex_ndv_unsupported_deriv_panics() {
        let sm: &'static ShaderModel50 = Box::leak(Box::new(ShaderModel50::new(50)));
        let labels: &'static FxHashMap<crate::codegen::ir::Label, usize> =
            Box::leak(Box::new(FxHashMap::default()));
        let mut e = SM50Encoder {
            sm,
            ip: 0,
            labels,
            inst: [0_u32; 2],
            sched: 0,
        };
        e.set_tex_ndv(0, TexDerivMode::DerivXY);
    }

    #[test]
    fn op_tex_bound_bindless_and_lod_split() {
        let (channel_mask, srcs) = base_tex_fields();
        let op_b = OpTex {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bound(0x321),
            srcs,
            dim: TexDim::_2D,
            lod_mode: TexLodMode::Lod,
            deriv_mode: TexDerivMode::NonDivergent,
            z_cmpr: true,
            offset_mode: TexOffsetMode::AddOffI,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: true,
            channel_mask,
        };
        let mut e = encoder_sm50();
        op_b.encode(&mut e);
        assert_eq!(e.get_field(36..49), 0x321);
        assert!(e.get_bit(54));
        assert_eq!(e.get_field(55..57), 3, "TexLodMode::Lod");

        let (channel_mask, srcs) = base_tex_fields();
        let op_bl = OpTex {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bindless,
            srcs,
            dim: TexDim::_3D,
            lod_mode: TexLodMode::Zero,
            deriv_mode: TexDerivMode::Auto,
            z_cmpr: false,
            offset_mode: TexOffsetMode::AddOffI,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask,
        };
        let mut e = encoder_sm50();
        op_bl.encode(&mut e);
        assert_eq!(sm50_opcode_clean(&e), 0xdeb8);
        assert!(e.get_bit(36));
        assert_eq!(e.get_field(37..39), 1, "Zero lod");
    }

    #[test]
    fn op_tex_tex_dims_all() {
        let (channel_mask, _) = base_tex_fields();
        for (dim, bits) in [
            (TexDim::_1D, 0_u64),
            (TexDim::Array1D, 1),
            (TexDim::_2D, 2),
            (TexDim::Array2D, 3),
            (TexDim::_3D, 4),
            (TexDim::Cube, 6),
            (TexDim::ArrayCube, 7),
        ] {
            let srcs = [gpr_src(1), gpr_src(2)];
            let op = OpTex {
                dsts: [Dst::None, Dst::None, Dst::None],
                tex: TexRef::Bound(0),
                srcs,
                dim,
                lod_mode: TexLodMode::Auto,
                deriv_mode: TexDerivMode::Auto,
                z_cmpr: false,
                offset_mode: TexOffsetMode::None,
                mem_eviction_priority: MemEvictionPriority::Normal,
                nodep: false,
                channel_mask,
            };
            let mut e = encoder_sm50();
            op.encode(&mut e);
            assert_eq!(e.get_field(28..31), bits);
        }
    }

    #[test]
    #[should_panic(expected = "internal compiler error")]
    fn op_tex_cbuf_ice() {
        let (channel_mask, srcs) = base_tex_fields();
        let op = OpTex {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::CBuf(crate::codegen::ir::TexCBufRef { idx: 0, offset: 0 }),
            srcs,
            dim: TexDim::_2D,
            lod_mode: TexLodMode::Auto,
            deriv_mode: TexDerivMode::Auto,
            z_cmpr: false,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask,
        };
        let mut e = encoder_sm50();
        op.encode(&mut e);
    }

    #[test]
    #[should_panic(expected = "internal compiler error")]
    fn op_tex_unknown_lod_panics() {
        let (channel_mask, srcs) = base_tex_fields();
        let op = OpTex {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bound(0),
            srcs,
            dim: TexDim::_2D,
            lod_mode: TexLodMode::BiasClamp,
            deriv_mode: TexDerivMode::Auto,
            z_cmpr: false,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask,
        };
        let mut e = encoder_sm50();
        op.encode(&mut e);
    }

    #[test]
    fn op_tld_bound_bindless_lod_bit() {
        let (channel_mask, srcs) = base_tex_fields();
        let op_b = OpTld {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bound(0x77),
            srcs,
            dim: TexDim::Array2D,
            is_ms: true,
            lod_mode: TexLodMode::Lod,
            offset_mode: TexOffsetMode::AddOffI,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: true,
            channel_mask,
        };
        let mut e = encoder_sm50();
        op_b.encode(&mut e);
        assert_eq!(e.get_field(36..49), 0x77);
        assert!(e.get_bit(55));

        let (channel_mask, srcs) = base_tex_fields();
        let op_bl = OpTld {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bindless,
            srcs,
            dim: TexDim::_1D,
            is_ms: false,
            lod_mode: TexLodMode::Zero,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask,
        };
        let mut e = encoder_sm50();
        op_bl.encode(&mut e);
        assert_eq!(sm50_opcode_clean(&e), 0xdd38);
    }

    #[test]
    #[should_panic(expected = "internal compiler error")]
    fn op_tld_cbuf_ice() {
        let (channel_mask, srcs) = base_tex_fields();
        let op = OpTld {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::CBuf(crate::codegen::ir::TexCBufRef { idx: 0, offset: 0 }),
            srcs,
            dim: TexDim::_2D,
            is_ms: false,
            lod_mode: TexLodMode::Zero,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask,
        };
        let mut e = encoder_sm50();
        op.encode(&mut e);
    }

    #[test]
    fn op_tld4_bound_and_bindless_offset_and_comp() {
        let (channel_mask, srcs) = base_tex_fields();
        let op_b = OpTld4 {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bound(3),
            srcs,
            dim: TexDim::_2D,
            comp: 1,
            offset_mode: TexOffsetMode::PerPx,
            z_cmpr: false,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask,
        };
        let mut e = encoder_sm50();
        op_b.encode(&mut e);
        assert_eq!(e.get_field(54..56), 2, "PerPx");
        assert_eq!(e.get_field(56..58), 1, "comp");

        let (channel_mask, srcs) = base_tex_fields();
        let op_bl = OpTld4 {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bindless,
            srcs,
            dim: TexDim::Cube,
            comp: 2,
            offset_mode: TexOffsetMode::AddOffI,
            z_cmpr: false,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask,
        };
        let mut e = encoder_sm50();
        op_bl.encode(&mut e);
        assert_eq!(sm50_opcode_clean(&e), 0xdef8);
        assert_eq!(e.get_field(36..38), 1);
        assert_eq!(e.get_field(38..40), 2);
    }

    #[test]
    fn op_tmml_and_txd() {
        let (channel_mask, srcs) = base_tex_fields();
        let op_m = OpTmml {
            dsts: [Dst::None, Dst::None],
            tex: TexRef::Bindless,
            srcs,
            dim: TexDim::Array1D,
            deriv_mode: TexDerivMode::NonDivergent,
            nodep: false,
            channel_mask,
        };
        let mut e = encoder_sm50();
        op_m.encode(&mut e);
        assert_eq!(sm50_opcode_clean(&e), 0xdf60);

        let (channel_mask, srcs) = base_tex_fields();
        let op_x = OpTxd {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bound(0),
            srcs,
            dim: TexDim::ArrayCube,
            offset_mode: TexOffsetMode::AddOffI,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask,
        };
        let mut e = encoder_sm50();
        op_x.encode(&mut e);
        assert_eq!(e.get_field(36..49), 0);
        assert!(e.get_bit(35));
    }

    #[test]
    fn op_txq_queries() {
        let channel_mask = ChannelMask::for_comps(1);
        for (query, qbits) in [
            (TexQuery::Dimension, 1_u64),
            (TexQuery::TextureType, 2),
            (TexQuery::SamplerPos, 5),
        ] {
            let op = OpTxq {
                dsts: [Dst::None, Dst::None],
                tex: TexRef::Bound(0x42),
                src: gpr_src(3),
                query,
                nodep: false,
                channel_mask,
            };
            let mut e = encoder_sm50();
            op.encode(&mut e);
            assert_eq!(e.get_field(22..28), qbits);
            assert_eq!(e.get_field(36..49), 0x42);
        }

        let (channel_mask, _) = base_tex_fields();
        let op_bl = OpTxq {
            dsts: [Dst::None, Dst::None],
            tex: TexRef::Bindless,
            src: gpr_src(2),
            query: TexQuery::Dimension,
            nodep: false,
            channel_mask,
        };
        let mut e = encoder_sm50();
        op_bl.encode(&mut e);
        assert_eq!(sm50_opcode_clean(&e), 0xdf50);
    }
}
