// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
//! Unit tests for SM20 texture instruction encoders.

use super::*;
use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    ChannelMask, Dst, MemEvictionPriority, OutType, PixVal, RegFile, RegRef, Src, SrcMod, SrcRef,
    SrcSwizzle, TexCBufRef, TexDerivMode, TexDim, TexLodMode, TexOffsetMode, TexQuery, TexRef,
    VoteOp,
};

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn pred_true_src() -> Src {
    Src {
        reference: SrcRef::True,
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn encoder_sm20() -> SM20Encoder<'static> {
    let sm: &'static ShaderModel20 = Box::leak(Box::new(ShaderModel20::new(20)));
    let labels: &'static FxHashMap<crate::codegen::ir::Label, usize> =
        Box::leak(Box::new(FxHashMap::default()));
    SM20Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 2],
    }
}

fn encoder_sm30() -> SM20Encoder<'static> {
    let sm: &'static ShaderModel20 = Box::leak(Box::new(ShaderModel20::new(30)));
    let labels: &'static FxHashMap<crate::codegen::ir::Label, usize> =
        Box::leak(Box::new(FxHashMap::default()));
    SM20Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 2],
    }
}

fn sm20_unit(e: &SM20Encoder<'_>) -> u64 {
    e.get_field(0..3)
}

fn sm20_subopcode(e: &SM20Encoder<'_>) -> u64 {
    e.get_field(58..64)
}

fn base_tex_like() -> (TexDim, ChannelMask, [Src; 2]) {
    (
        TexDim::_2D,
        ChannelMask::for_comps(4),
        [gpr_src(1), gpr_src(2)],
    )
}

#[test]
fn helper_set_tex_dim_lod_channel_ndv_sm20() {
    let sm: &'static ShaderModel20 = Box::leak(Box::new(ShaderModel20::new(20)));
    let labels: &'static FxHashMap<crate::codegen::ir::Label, usize> =
        Box::leak(Box::new(FxHashMap::default()));
    let mut e = SM20Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 2],
    };
    e.set_tex_dim(10..13, TexDim::ArrayCube);
    assert_eq!(e.get_field(10..13), 7);
    e.set_tex_lod_mode(20..22, TexLodMode::Bias);
    assert_eq!(e.get_field(20..22), 2);
    e.set_tex_channel_mask(30..34, ChannelMask::new(0b1010));
    assert_eq!(e.get_field(30..34), 0xa);
    e.set_tex_ndv(40, TexDerivMode::Auto);
    assert!(!e.get_bit(40));
    e.set_tex_ndv(41, TexDerivMode::NonDivergent);
    assert!(e.get_bit(41));
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn helper_set_tex_ndv_unsupported_deriv_panics() {
    let sm: &'static ShaderModel20 = Box::leak(Box::new(ShaderModel20::new(20)));
    let labels: &'static FxHashMap<crate::codegen::ir::Label, usize> =
        Box::leak(Box::new(FxHashMap::default()));
    let mut e = SM20Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 2],
    };
    e.set_tex_ndv(0, TexDerivMode::ForceDivergent);
}

#[test]
fn op_tex_bound_encodes_tex_unit_subopcode_and_tex_idx() {
    let (dim, channel_mask, srcs) = base_tex_like();
    let op = OpTex {
        dsts: [Dst::None, Dst::None, Dst::None],
        tex: TexRef::Bound(0x2a),
        srcs,
        dim,
        lod_mode: TexLodMode::Auto,
        deriv_mode: TexDerivMode::NonDivergent,
        z_cmpr: true,
        offset_mode: TexOffsetMode::AddOffI,
        mem_eviction_priority: MemEvictionPriority::Normal,
        nodep: true,
        channel_mask,
    };
    let mut e = encoder_sm20();
    op.encode(&mut e);
    assert_eq!(sm20_unit(&e), SM20Unit::Tex as u64);
    assert_eq!(sm20_subopcode(&e), 0x20);
    assert_eq!(e.get_field(32..40), 0x2a);
    assert!(!e.get_bit(50), "bound: not bindless");
    assert_eq!(e.get_field(51..54), 2, "TexDim::_2D");
    assert_eq!(e.get_field(46..50), u64::from(channel_mask.to_bits()));
    assert!(e.get_bit(45), "ndv");
    assert!(e.get_bit(54), "offset AddOffI");
    assert!(e.get_bit(56), "z_cmpr");
    assert_eq!(e.get_field(57..59), 0, "TexLodMode::Auto");
    assert_eq!(e.get_field(20..26), 1);
    assert_eq!(e.get_field(26..32), 2);
}

#[test]
fn op_tex_lod_modes_auto_zero_bias_lod() {
    let (dim, channel_mask, _) = base_tex_like();
    for (lod, bits) in [
        (TexLodMode::Auto, 0_u64),
        (TexLodMode::Zero, 1),
        (TexLodMode::Bias, 2),
        (TexLodMode::Lod, 3),
    ] {
        let srcs = [gpr_src(1), gpr_src(2)];
        let op = OpTex {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bound(0),
            srcs,
            dim,
            lod_mode: lod,
            deriv_mode: TexDerivMode::Auto,
            z_cmpr: false,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask,
        };
        let mut e = encoder_sm20();
        op.encode(&mut e);
        assert_eq!(e.get_field(57..59), bits);
    }
}

#[test]
fn op_tex_tex_dims_all_variants() {
    let (_, channel_mask, _) = base_tex_like();
    for (dim, expected) in [
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
            lod_mode: TexLodMode::Zero,
            deriv_mode: TexDerivMode::Auto,
            z_cmpr: false,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask,
        };
        let mut e = encoder_sm20();
        op.encode(&mut e);
        assert_eq!(e.get_field(51..54), expected);
    }
}

#[test]
fn op_tex_bindless_sm30_sets_ff_and_b_bit() {
    let (dim, channel_mask, srcs) = base_tex_like();
    let op = OpTex {
        dsts: [Dst::None, Dst::None, Dst::None],
        tex: TexRef::Bindless,
        srcs,
        dim,
        lod_mode: TexLodMode::Lod,
        deriv_mode: TexDerivMode::Auto,
        z_cmpr: false,
        offset_mode: TexOffsetMode::None,
        mem_eviction_priority: MemEvictionPriority::Normal,
        nodep: false,
        channel_mask,
    };
    let mut e = encoder_sm30();
    op.encode(&mut e);
    assert_eq!(e.get_field(32..40), 0xff);
    assert!(e.get_bit(50));
}

#[test]
#[should_panic(expected = "assertion failed")]
fn op_tex_bindless_sm20_panics() {
    let (dim, channel_mask, srcs) = base_tex_like();
    let op = OpTex {
        dsts: [Dst::None, Dst::None, Dst::None],
        tex: TexRef::Bindless,
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
    let mut e = encoder_sm20();
    op.encode(&mut e);
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn op_tex_cbuf_ice() {
    let (dim, channel_mask, srcs) = base_tex_like();
    let op = OpTex {
        dsts: [Dst::None, Dst::None, Dst::None],
        tex: TexRef::CBuf(TexCBufRef {
            idx: 1,
            offset: 0x10,
        }),
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
    let mut e = encoder_sm20();
    op.encode(&mut e);
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn op_tex_unknown_lod_panics() {
    let (dim, channel_mask, srcs) = base_tex_like();
    let op = OpTex {
        dsts: [Dst::None, Dst::None, Dst::None],
        tex: TexRef::Bound(0),
        srcs,
        dim,
        lod_mode: TexLodMode::Clamp,
        deriv_mode: TexDerivMode::Auto,
        z_cmpr: false,
        offset_mode: TexOffsetMode::None,
        mem_eviction_priority: MemEvictionPriority::Normal,
        nodep: false,
        channel_mask,
    };
    let mut e = encoder_sm20();
    op.encode(&mut e);
}

#[test]
fn op_tld_bound_zero_and_lod() {
    let (dim, channel_mask, _) = base_tex_like();
    for (lod, bit57) in [(TexLodMode::Zero, 0_u64), (TexLodMode::Lod, 1)] {
        let srcs = [gpr_src(1), gpr_src(2)];
        let op = OpTld {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bound(3),
            srcs,
            dim,
            is_ms: true,
            lod_mode: lod,
            offset_mode: TexOffsetMode::AddOffI,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: true,
            channel_mask,
        };
        let mut e = encoder_sm20();
        op.encode(&mut e);
        assert_eq!(sm20_subopcode(&e), 0x24);
        assert_eq!(e.get_field(57..58), bit57);
        assert!(e.get_bit(55), "is_ms");
    }
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn op_tld_invalid_lod_panics() {
    let (dim, channel_mask, srcs) = base_tex_like();
    let op = OpTld {
        dsts: [Dst::None, Dst::None, Dst::None],
        tex: TexRef::Bound(0),
        srcs,
        dim,
        is_ms: false,
        lod_mode: TexLodMode::Bias,
        offset_mode: TexOffsetMode::None,
        mem_eviction_priority: MemEvictionPriority::Normal,
        nodep: false,
        channel_mask,
    };
    let mut e = encoder_sm20();
    op.encode(&mut e);
}

#[test]
fn op_tld4_offset_modes_and_comp() {
    let (dim, channel_mask, _) = base_tex_like();
    for (offset_mode, bits_54_56) in [
        (TexOffsetMode::None, 0_u64),
        (TexOffsetMode::AddOffI, 1),
        (TexOffsetMode::PerPx, 2),
    ] {
        let srcs = [gpr_src(1), gpr_src(2)];
        let op = OpTld4 {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bound(1),
            srcs,
            dim,
            comp: 3,
            offset_mode,
            z_cmpr: true,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask,
        };
        let mut e = encoder_sm20();
        op.encode(&mut e);
        assert_eq!(sm20_subopcode(&e), 0x28);
        assert_eq!(e.get_field(5..7), 3, "comp");
        assert_eq!(e.get_field(54..56), bits_54_56);
        assert!(e.get_bit(56), "z_cmpr");
    }
}

#[test]
fn op_tmml_bound() {
    let (dim, channel_mask, srcs) = base_tex_like();
    let op = OpTmml {
        dsts: [Dst::None, Dst::None],
        tex: TexRef::Bound(5),
        srcs,
        dim,
        deriv_mode: TexDerivMode::NonDivergent,
        nodep: true,
        channel_mask,
    };
    let mut e = encoder_sm20();
    op.encode(&mut e);
    assert_eq!(sm20_subopcode(&e), 0x2c);
    assert!(e.get_bit(45), "ndv");
    assert!(e.get_bit(9), "nodep");
}

#[test]
fn op_txd_bound_offset_bit() {
    let (dim, channel_mask, srcs) = base_tex_like();
    let op = OpTxd {
        dsts: [Dst::None, Dst::None, Dst::None],
        tex: TexRef::Bound(7),
        srcs,
        dim,
        offset_mode: TexOffsetMode::AddOffI,
        mem_eviction_priority: MemEvictionPriority::Normal,
        nodep: false,
        channel_mask,
    };
    let mut e = encoder_sm20();
    op.encode(&mut e);
    assert_eq!(sm20_subopcode(&e), 0x38);
    assert!(e.get_bit(54));
}

#[test]
fn op_txq_queries_bound() {
    let channel_mask = ChannelMask::for_comps(2);
    for (query, qbits) in [
        (TexQuery::Dimension, 0_u64),
        (TexQuery::TextureType, 1),
        (TexQuery::SamplerPos, 2),
    ] {
        let op = OpTxq {
            dsts: [Dst::None, Dst::None],
            tex: TexRef::Bound(4),
            src: gpr_src(6),
            query,
            nodep: true,
            channel_mask,
        };
        let mut e = encoder_sm20();
        op.encode(&mut e);
        assert_eq!(sm20_subopcode(&e), 0x30);
        assert_eq!(e.get_field(54..57), qbits);
        assert_eq!(e.get_field(20..26), 6);
    }
}

#[test]
fn op_texdepbar_encodes() {
    let op = OpTexDepBar {
        textures_left: 0x2f,
    };
    let mut e = encoder_sm20();
    op.encode(&mut e);
    assert_eq!(sm20_subopcode(&e), 0x3c);
    assert_eq!(e.get_field(5..9), 0xf);
    assert_eq!(e.get_field(26..32), 0x2f);
}

#[test]
fn op_vild_encodes() {
    let op = OpViLd {
        dst: Dst::None,
        idx: gpr_src(8),
        off: -4,
    };
    let mut e = encoder_sm20();
    op.encode(&mut e);
    assert_eq!(sm20_subopcode(&e), 0x0);
    // `off` is i8; encoder stores it in the 16-bit field without full sign extension.
    assert_eq!(e.get_field(26..42), 252);
}

#[test]
fn op_pixld_covmask() {
    let op = OpPixLd {
        dst: Dst::None,
        val: PixVal::CovMask,
    };
    let mut e = encoder_sm20();
    op.encode(&mut e);
    assert_eq!(sm20_subopcode(&e), 0x4);
    assert_eq!(e.get_field(5..8), 1);
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn op_pixld_unsupported_val_panics() {
    let op = OpPixLd {
        dst: Dst::None,
        val: PixVal::MsCount,
    };
    let mut e = encoder_sm20();
    op.encode(&mut e);
}

#[test]
fn op_s2r_encodes() {
    let op = OpS2R {
        dst: Dst::None,
        idx: 0x17,
    };
    let mut e = encoder_sm20();
    op.encode(&mut e);
    assert_eq!(sm20_unit(&e), SM20Unit::Move as u64);
    assert_eq!(sm20_subopcode(&e), 0xb);
    assert_eq!(e.get_field(26..36), 0x17);
}

#[test]
fn op_vote_all_any_eq() {
    for (vote_op, bits) in [(VoteOp::All, 0_u64), (VoteOp::Any, 1), (VoteOp::Eq, 2)] {
        let op = OpVote {
            op: vote_op,
            dsts: [Dst::None, Dst::None],
            pred: pred_true_src(),
        };
        let mut e = encoder_sm20();
        op.encode(&mut e);
        assert_eq!(sm20_unit(&e), SM20Unit::Move as u64);
        assert_eq!(sm20_subopcode(&e), 0x12);
        assert_eq!(e.get_field(5..7), bits);
    }
}

#[test]
fn op_out_emit_encodes() {
    let op = OpOut {
        dst: Dst::None,
        srcs: [gpr_src(3), gpr_src(4)],
        out_type: OutType::Emit,
    };
    let mut e = encoder_sm20();
    op.encode(&mut e);
    assert_eq!(sm20_unit(&e), SM20Unit::Tex as u64);
    assert_eq!(sm20_subopcode(&e), 0x7);
    assert_eq!(e.get_field(5..7), 1, "emit");
}
