// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
//! Unit tests for SM32 texture and surface encoders.

use super::super::encoder::*;
use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    ChannelMask, Dst, IMadSpMode, ImageAccess, LdCacheOp, MemEvictionPriority, MemType, RegFile,
    RegRef, Src, SrcMod, SrcRef, SrcSwizzle, StCacheOp, SuClampMode, SuClampRound, SuGaOffsetMode,
    TexDerivMode, TexDim, TexLodMode, TexOffsetMode, TexQuery, TexRef,
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

fn fresh_encoder() -> SM32Encoder<'static> {
    let sm: &'static ShaderModel32 = Box::leak(Box::new(ShaderModel32::new(35)));
    let labels: &'static FxHashMap<crate::codegen::ir::Label, usize> =
        Box::leak(Box::new(FxHashMap::default()));
    SM32Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 2],
    }
}

fn encoder_sm35() -> SM32Encoder<'static> {
    fresh_encoder()
}

fn sm32_opcode_clean(e: &SM32Encoder<'_>) -> u64 {
    e.get_field(52..64)
}

fn sm32_fu(e: &SM32Encoder<'_>) -> u64 {
    e.get_field(0..2)
}

fn base_tex_op_fields() -> (ChannelMask, [Src; 2]) {
    (ChannelMask::for_comps(4), [gpr_src(1), gpr_src(2)])
}

#[test]
fn helper_set_tex_dim_lod_ndv_sm32() {
    let mut e = fresh_encoder();
    e.set_tex_dim(10..13, TexDim::ArrayCube);
    assert_eq!(e.get_field(10..13), 7);
    e.set_tex_lod_mode(20..23, TexLodMode::Bias);
    assert_eq!(e.get_field(20..23), 2);
    e.set_tex_ndv(40, TexDerivMode::Auto);
    assert!(!e.get_bit(40));
    e.set_tex_ndv(41, TexDerivMode::NonDivergent);
    assert!(e.get_bit(41));
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn helper_set_tex_lod_unknown_panics() {
    let mut e = fresh_encoder();
    e.set_tex_lod_mode(0..3, TexLodMode::Clamp);
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn helper_set_tex_ndv_unsupported_deriv_panics() {
    let mut e = fresh_encoder();
    e.set_tex_ndv(0, TexDerivMode::ForceDivergent);
}

#[test]
fn op_tex_bound_and_bindless() {
    let (channel_mask, srcs) = base_tex_op_fields();
    let op_b = OpTex {
        dsts: [Dst::None, Dst::None, Dst::None],
        tex: TexRef::Bound(0x123),
        srcs,
        dim: TexDim::_2D,
        lod_mode: TexLodMode::Auto,
        deriv_mode: TexDerivMode::NonDivergent,
        z_cmpr: true,
        offset_mode: TexOffsetMode::AddOffI,
        mem_eviction_priority: MemEvictionPriority::Normal,
        nodep: true,
        channel_mask,
    };
    let mut e = encoder_sm35();
    op_b.encode(&mut e);
    assert_eq!(sm32_fu(&e), 1);
    assert_eq!(e.get_field(47..60), 0x123);

    let (channel_mask, srcs) = base_tex_op_fields();
    let op_bl = OpTex {
        dsts: [Dst::None, Dst::None, Dst::None],
        tex: TexRef::Bindless,
        srcs,
        dim: TexDim::_3D,
        lod_mode: TexLodMode::Lod,
        deriv_mode: TexDerivMode::Auto,
        z_cmpr: false,
        offset_mode: TexOffsetMode::None,
        mem_eviction_priority: MemEvictionPriority::Normal,
        nodep: false,
        channel_mask,
    };
    let mut e = encoder_sm35();
    op_bl.encode(&mut e);
    assert_eq!(sm32_opcode_clean(&e), 0x7d8);
    assert_eq!(sm32_fu(&e), 2);
}

#[test]
fn op_tex_tex_dims_and_lod_modes() {
    let (channel_mask, _) = base_tex_op_fields();
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
            lod_mode: TexLodMode::Zero,
            deriv_mode: TexDerivMode::Auto,
            z_cmpr: false,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask,
        };
        let mut e = encoder_sm35();
        op.encode(&mut e);
        assert_eq!(e.get_field(38..41), bits);
    }

    let (channel_mask, _) = base_tex_op_fields();
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
            dim: TexDim::_2D,
            lod_mode: lod,
            deriv_mode: TexDerivMode::Auto,
            z_cmpr: false,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: false,
            channel_mask,
        };
        let mut e = encoder_sm35();
        op.encode(&mut e);
        assert_eq!(e.get_field(44..47), bits);
    }
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn op_tex_cbuf_ice() {
    let (channel_mask, srcs) = base_tex_op_fields();
    let op = OpTex {
        dsts: [Dst::None, Dst::None, Dst::None],
        tex: TexRef::CBuf(crate::codegen::ir::TexCBufRef { idx: 1, offset: 0 }),
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
    let mut e = encoder_sm35();
    op.encode(&mut e);
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn op_tex_unknown_lod_panics() {
    let (channel_mask, srcs) = base_tex_op_fields();
    let op = OpTex {
        dsts: [Dst::None, Dst::None, Dst::None],
        tex: TexRef::Bound(0),
        srcs,
        dim: TexDim::_2D,
        lod_mode: TexLodMode::Clamp,
        deriv_mode: TexDerivMode::Auto,
        z_cmpr: false,
        offset_mode: TexOffsetMode::None,
        mem_eviction_priority: MemEvictionPriority::Normal,
        nodep: false,
        channel_mask,
    };
    let mut e = encoder_sm35();
    op.encode(&mut e);
}

#[test]
fn op_tld_bound_bindless_and_lod_bit() {
    let (channel_mask, srcs) = base_tex_op_fields();
    let op_b = OpTld {
        dsts: [Dst::None, Dst::None, Dst::None],
        tex: TexRef::Bound(0x55),
        srcs,
        dim: TexDim::Array2D,
        is_ms: true,
        lod_mode: TexLodMode::Lod,
        offset_mode: TexOffsetMode::AddOffI,
        mem_eviction_priority: MemEvictionPriority::Normal,
        nodep: false,
        channel_mask,
    };
    let mut e = encoder_sm35();
    op_b.encode(&mut e);
    assert_eq!(sm32_fu(&e), 2);
    assert_eq!(e.get_field(47..60), 0x55);
    assert!(e.get_bit(44), "lod == Lod");

    let (channel_mask, srcs) = base_tex_op_fields();
    let op_z = OpTld {
        dsts: [Dst::None, Dst::None, Dst::None],
        tex: TexRef::Bound(0),
        srcs,
        dim: TexDim::_2D,
        is_ms: false,
        lod_mode: TexLodMode::Zero,
        offset_mode: TexOffsetMode::None,
        mem_eviction_priority: MemEvictionPriority::Normal,
        nodep: false,
        channel_mask,
    };
    let mut e = encoder_sm35();
    op_z.encode(&mut e);
    assert!(!e.get_bit(44), "lod == Zero");

    let (channel_mask, srcs) = base_tex_op_fields();
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
    let mut e = encoder_sm35();
    op_bl.encode(&mut e);
    assert_eq!(sm32_opcode_clean(&e), 0x780);
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn op_tld_cbuf_ice() {
    let (channel_mask, srcs) = base_tex_op_fields();
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
    let mut e = encoder_sm35();
    op.encode(&mut e);
}

#[test]
fn op_tld4_bound_offset_modes_and_comp() {
    let (channel_mask, _) = base_tex_op_fields();
    for (offset_mode, bits) in [
        (TexOffsetMode::None, 0_u64),
        (TexOffsetMode::AddOffI, 1),
        (TexOffsetMode::PerPx, 2),
    ] {
        let srcs = [gpr_src(1), gpr_src(2)];
        let op = OpTld4 {
            dsts: [Dst::None, Dst::None, Dst::None],
            tex: TexRef::Bound(2),
            srcs,
            dim: TexDim::Cube,
            comp: 2,
            offset_mode,
            z_cmpr: true,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: true,
            channel_mask,
        };
        let mut e = encoder_sm35();
        op.encode(&mut e);
        assert_eq!(sm32_fu(&e), 1);
        assert_eq!(e.get_field(47..60), 2);
        assert_eq!(e.get_field(43..45), bits);
        assert_eq!(e.get_field(45..47), 2, "comp");
    }
}

#[test]
fn op_tmml_and_txd() {
    let (channel_mask, srcs) = base_tex_op_fields();
    let op_m = OpTmml {
        dsts: [Dst::None, Dst::None],
        tex: TexRef::Bindless,
        srcs,
        dim: TexDim::Array1D,
        deriv_mode: TexDerivMode::NonDivergent,
        nodep: false,
        channel_mask,
    };
    let mut e = encoder_sm35();
    op_m.encode(&mut e);
    assert_eq!(sm32_opcode_clean(&e), 0x7e8);

    let (channel_mask, srcs) = base_tex_op_fields();
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
    let mut e = encoder_sm35();
    op_x.encode(&mut e);
    assert_eq!(e.get_field(47..60), 128);
    assert!(e.get_bit(54));
}

#[test]
fn op_txq_queries() {
    let channel_mask = ChannelMask::for_comps(2);
    for (query, qbits) in [
        (TexQuery::Dimension, 1_u64),
        (TexQuery::TextureType, 2),
        (TexQuery::SamplerPos, 5),
    ] {
        let op = OpTxq {
            dsts: [Dst::None, Dst::None],
            tex: TexRef::Bound(0xabc),
            src: gpr_src(4),
            query,
            nodep: true,
            channel_mask,
        };
        let mut e = encoder_sm35();
        op.encode(&mut e);
        assert_eq!(sm32_fu(&e), 2);
        assert_eq!(e.get_field(25..31), qbits);
        assert_eq!(e.get_field(41..54), 0xabc);
    }
}

#[test]
fn su_surface_op_family_encodes() {
    let mut e = encoder_sm35();
    OpSuClamp {
        dsts: [Dst::None, Dst::None],
        mode: SuClampMode::StoredInDescriptor,
        round: SuClampRound::R1,
        is_s32: false,
        is_2d: true,
        srcs: [gpr_src(1), gpr_src(2)],
        imm: 0,
    }
    .encode(&mut e);
    assert_eq!(sm32_fu(&e), 2);
    assert_eq!(e.get_field(42..48), 0);
    assert!(!e.get_bit(51));
    assert_eq!(e.get_field(52..56), 0);
    assert!(e.get_bit(56));

    let mut e = encoder_sm35();
    OpSuBfm {
        dsts: [Dst::None, Dst::None],
        srcs: [gpr_src(1), gpr_src(2), gpr_src(3)],
        is_3d: false,
    }
    .encode(&mut e);
    assert_eq!(sm32_fu(&e), 2);
    assert!(!e.get_bit(50));

    let mut e = encoder_sm35();
    OpSuEau {
        dst: Dst::None,
        srcs: [gpr_src(1), gpr_src(2), gpr_src(3)],
    }
    .encode(&mut e);
    assert_eq!(sm32_fu(&e), 2);
    assert_eq!(e.get_field(10..18), 1);

    let mut e = encoder_sm35();
    OpIMadSp {
        dst: Dst::None,
        srcs: [gpr_src(1), gpr_src(2), gpr_src(3)],
        mode: IMadSpMode::FromSrc1,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(54..56), 3);

    let mut e = encoder_sm35();
    OpSuLdGa {
        dst: Dst::None,
        mem_type: MemType::B32,
        offset_mode: SuGaOffsetMode::U32,
        cache_op: LdCacheOp::CacheAll,
        srcs: [gpr_src(5), gpr_src(6), pred_true_src()],
    }
    .encode(&mut e);
    assert_eq!(sm32_opcode_clean(&e), 0x798);
    assert_eq!(sm32_fu(&e), 2);

    let mut e = encoder_sm35();
    OpSuStGa {
        image_access: ImageAccess::Binary(MemType::B32),
        offset_mode: SuGaOffsetMode::U32,
        cache_op: StCacheOp::WriteBack,
        srcs: [gpr_src(5), gpr_src(6), gpr_src(7), pred_true_src()],
    }
    .encode(&mut e);
    assert_eq!(sm32_fu(&e), 2);
    assert_eq!(e.get_field(10..18), 6);
}
