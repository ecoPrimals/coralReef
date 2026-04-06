// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unit tests for SM70 instruction latency tables.

use super::*;
use crate::codegen::ir::{
    AtomOp, AtomType, CCtlOp, ChannelMask, Dst, FRndMode, FloatCmpOp, FloatType, HmmaSize,
    ImageAccess, ImageDim, ImmaSize, IntCmpOp, IntCmpType, IntType, MemAccess, MemAddrType,
    MemEvictionPriority, MemOrder, MemScope, MemSpace, MemType, OffsetStride, Op, OpAtom, OpBMov,
    OpBRev, OpCCtl, OpCS2R, OpDAdd, OpDFma, OpDMnMx, OpDMul, OpDSetP, OpExit, OpF2F, OpFFma, OpFlo,
    OpHAdd2, OpHFma2, OpHMul2, OpHmma, OpIAdd2, OpIMad, OpIMad64, OpISetP, OpImma, OpLd, OpMov,
    OpNop, OpPopC, OpSuLd, OpSuSt, OpTex, OpTld, OpTld4, OpTmml, OpTranscendental, OpTxd, OpTxq,
    PredSetOp, RegFile, RegRef, Src, TexDerivMode, TexDim, TexLodMode, TexOffsetMode, TexQuery,
    TexRef, TranscendentalOp,
};

/// Same value as `DEFAULT_LATENCY` in `sm70_instr_latencies.rs` for unexpected register paths.
const EXPECT_DEFAULT_LATENCY: u32 = 15;

fn gpr_dst() -> Dst {
    Dst::Reg(RegRef::new(RegFile::GPR, 0, 1))
}

fn make_mov() -> Op {
    Op::Mov(Box::new(OpMov {
        dst: gpr_dst(),
        src: Src::ZERO,
        quad_lanes: 0xf,
    }))
}

fn make_ffma() -> Op {
    Op::FFma(Box::new(OpFFma {
        dst: gpr_dst(),
        srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }))
}

fn make_imad() -> Op {
    Op::IMad(Box::new(OpIMad {
        dst: gpr_dst(),
        srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
        signed: false,
    }))
}

fn make_ld() -> Op {
    Op::Ld(Box::new(OpLd {
        dst: gpr_dst(),
        addr: Src::ZERO,
        offset: 0,
        stride: OffsetStride::X1,
        access: MemAccess {
            mem_type: MemType::B32,
            space: MemSpace::Global(MemAddrType::A32),
            order: MemOrder::Strong(MemScope::CTA),
            eviction_priority: MemEvictionPriority::Normal,
        },
    }))
}

fn make_nop() -> Op {
    Op::Nop(OpNop { label: None })
}

fn make_exit() -> Op {
    Op::Exit(OpExit {})
}

fn gpr_pred_dst(idx: u32) -> Dst {
    Dst::Reg(RegRef::new(RegFile::Pred, idx, 1))
}

fn make_isetp() -> Op {
    Op::ISetP(Box::new(OpISetP {
        dst: gpr_pred_dst(0),
        set_op: PredSetOp::And,
        cmp_op: IntCmpOp::Eq,
        cmp_type: IntCmpType::U32,
        ex: false,
        srcs: [
            Src::ZERO,
            Src::ZERO,
            Src::new_imm_bool(false),
            Src::new_imm_bool(false),
        ],
    }))
}

fn make_dsetp() -> Op {
    Op::DSetP(Box::new(OpDSetP {
        dst: gpr_pred_dst(1),
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdEq,
        srcs: [Src::ZERO, Src::ZERO, Src::new_imm_bool(false)],
    }))
}

fn make_dfma() -> Op {
    Op::DFma(Box::new(OpDFma {
        dst: gpr_dst(),
        srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
        rnd_mode: FRndMode::NearestEven,
    }))
}

fn make_dadd() -> Op {
    Op::DAdd(Box::new(OpDAdd {
        dst: gpr_dst(),
        srcs: [Src::ZERO, Src::ZERO],
        rnd_mode: FRndMode::NearestEven,
    }))
}

fn make_dmul() -> Op {
    Op::DMul(Box::new(OpDMul {
        dst: gpr_dst(),
        srcs: [Src::ZERO, Src::ZERO],
        rnd_mode: FRndMode::NearestEven,
    }))
}

fn make_dmnmx() -> Op {
    Op::DMnMx(Box::new(OpDMnMx {
        dst: gpr_dst(),
        srcs: [Src::ZERO, Src::ZERO, Src::new_imm_bool(false)],
    }))
}

fn make_hfma2() -> Op {
    Op::HFma2(Box::new(OpHFma2 {
        dst: gpr_dst(),
        srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
        saturate: false,
        ftz: false,
        dnz: false,
        f32: false,
    }))
}

fn make_hadd2() -> Op {
    Op::HAdd2(Box::new(OpHAdd2 {
        dst: gpr_dst(),
        srcs: [Src::ZERO, Src::ZERO],
        saturate: false,
        ftz: false,
        f32: false,
    }))
}

fn make_hmul2() -> Op {
    Op::HMul2(Box::new(OpHMul2 {
        dst: gpr_dst(),
        srcs: [Src::ZERO, Src::ZERO],
        saturate: false,
        ftz: false,
        dnz: false,
    }))
}

fn make_f2f() -> Op {
    Op::F2F(Box::new(OpF2F {
        dst: gpr_dst(),
        src: Src::ZERO,
        src_type: FloatType::F32,
        dst_type: FloatType::F32,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dst_high: false,
        integer_rnd: false,
    }))
}

fn make_popc() -> Op {
    Op::PopC(Box::new(OpPopC {
        dst: gpr_dst(),
        src: Src::ZERO,
    }))
}

fn make_flo() -> Op {
    Op::Flo(Box::new(OpFlo {
        dst: gpr_dst(),
        src: Src::ZERO,
        signed: false,
        return_shift_amount: false,
    }))
}

fn make_brev() -> Op {
    Op::BRev(Box::new(OpBRev {
        dst: gpr_dst(),
        src: Src::ZERO,
    }))
}

fn make_transcendental() -> Op {
    Op::Transcendental(Box::new(OpTranscendental {
        dst: gpr_dst(),
        op: TranscendentalOp::Sqrt,
        src: Src::ZERO,
    }))
}

fn make_tex() -> Op {
    Op::Tex(Box::new(OpTex {
        dsts: [gpr_dst(), Dst::None, Dst::None],
        tex: TexRef::Bound(0),
        srcs: [Src::ZERO, Src::new_imm_u32(1)],
        dim: TexDim::_2D,
        lod_mode: TexLodMode::Auto,
        deriv_mode: TexDerivMode::Auto,
        z_cmpr: false,
        offset_mode: TexOffsetMode::None,
        mem_eviction_priority: MemEvictionPriority::Normal,
        nodep: false,
        channel_mask: ChannelMask::for_comps(4),
    }))
}

fn make_tld() -> Op {
    Op::Tld(Box::new(OpTld {
        dsts: [gpr_dst(), Dst::None, Dst::None],
        tex: TexRef::Bound(0),
        srcs: [Src::ZERO, Src::ZERO],
        dim: TexDim::_2D,
        is_ms: false,
        lod_mode: TexLodMode::Auto,
        offset_mode: TexOffsetMode::None,
        mem_eviction_priority: MemEvictionPriority::Normal,
        nodep: false,
        channel_mask: ChannelMask::for_comps(4),
    }))
}

fn make_tld4() -> Op {
    Op::Tld4(Box::new(OpTld4 {
        dsts: [gpr_dst(), Dst::None, Dst::None],
        tex: TexRef::Bound(0),
        srcs: [Src::ZERO, Src::ZERO],
        dim: TexDim::_2D,
        comp: 0,
        offset_mode: TexOffsetMode::None,
        z_cmpr: false,
        mem_eviction_priority: MemEvictionPriority::Normal,
        nodep: false,
        channel_mask: ChannelMask::for_comps(4),
    }))
}

fn make_tmml() -> Op {
    Op::Tmml(Box::new(OpTmml {
        dsts: [gpr_dst(), Dst::None],
        tex: TexRef::Bound(0),
        srcs: [Src::ZERO, Src::ZERO],
        dim: TexDim::_2D,
        deriv_mode: TexDerivMode::Auto,
        nodep: false,
        channel_mask: ChannelMask::for_comps(4),
    }))
}

fn make_txd() -> Op {
    Op::Txd(Box::new(OpTxd {
        dsts: [gpr_dst(), Dst::None, Dst::None],
        tex: TexRef::Bound(0),
        srcs: [Src::ZERO, Src::ZERO],
        dim: TexDim::_2D,
        offset_mode: TexOffsetMode::None,
        mem_eviction_priority: MemEvictionPriority::Normal,
        nodep: false,
        channel_mask: ChannelMask::for_comps(4),
    }))
}

fn make_txq() -> Op {
    Op::Txq(Box::new(OpTxq {
        dsts: [gpr_dst(), Dst::None],
        tex: TexRef::Bound(0),
        src: Src::ZERO,
        query: TexQuery::Dimension,
        nodep: false,
        channel_mask: ChannelMask::for_comps(4),
    }))
}

fn make_atom() -> Op {
    Op::Atom(Box::new(OpAtom {
        dst: gpr_dst(),
        srcs: [Src::ZERO, Src::ZERO, Src::new_imm_u32(0)],
        atom_op: AtomOp::Add,
        atom_type: AtomType::U32,
        addr_offset: 0,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Global(MemAddrType::A32),
        mem_order: MemOrder::Constant,
        mem_eviction_priority: MemEvictionPriority::Normal,
    }))
}

fn make_suld() -> Op {
    Op::SuLd(Box::new(OpSuLd {
        dsts: [gpr_dst(), Dst::None],
        image_access: ImageAccess::Binary(MemType::B32),
        image_dim: ImageDim::_2D,
        mem_order: MemOrder::Constant,
        mem_eviction_priority: MemEvictionPriority::Normal,
        srcs: [Src::ZERO, Src::ZERO],
    }))
}

fn make_sust() -> Op {
    Op::SuSt(Box::new(OpSuSt {
        image_access: ImageAccess::Binary(MemType::B32),
        image_dim: ImageDim::_2D,
        mem_order: MemOrder::Constant,
        mem_eviction_priority: MemEvictionPriority::Normal,
        srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
    }))
}

fn make_cs2r(comps: u8) -> Op {
    Op::CS2R(Box::new(OpCS2R {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, comps)),
        idx: 0,
    }))
}

fn make_bmov_gpr() -> Op {
    Op::BMov(Box::new(OpBMov {
        dst: gpr_dst(),
        src: Src::ZERO,
        clear: false,
    }))
}

fn make_hmma() -> Op {
    Op::Hmma(Box::new(OpHmma {
        dst: gpr_dst(),
        mat_size: HmmaSize::M16N8K8,
        src_type: FloatType::F16,
        dst_type: FloatType::F32,
        srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
    }))
}

fn make_imma() -> Op {
    Op::Imma(Box::new(OpImma {
        dst: gpr_dst(),
        mat_size: ImmaSize::M16N8K16,
        src_types: [IntType::I8, IntType::I8],
        saturate: false,
        srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
    }))
}

fn make_imad64() -> Op {
    Op::IMad64(Box::new(OpIMad64 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, 2)),
        srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
        signed: false,
    }))
}

fn make_cctl() -> Op {
    Op::CCtl(Box::new(OpCCtl {
        op: CCtlOp::WB,
        mem_space: MemSpace::Global(MemAddrType::A32),
        addr: Src::ZERO,
        addr_offset: 0,
    }))
}

fn make_iadd2_carry_dst() -> Op {
    Op::IAdd2(Box::new(OpIAdd2 {
        dsts: [gpr_dst(), Dst::Reg(RegRef::new(RegFile::Carry, 0, 1))],
        srcs: [Src::ZERO, Src::ZERO],
    }))
}

#[test]
fn test_needs_scoreboards_decoupled_ops() {
    // Ld is decoupled (variable latency) - needs scoreboards
    assert!(SM70Latency::needs_scoreboards(&make_ld()));
}

#[test]
fn test_needs_scoreboards_coupled_ops() {
    // Mov, FFma, IMad are coupled - no scoreboards
    assert!(!SM70Latency::needs_scoreboards(&make_mov()));
    assert!(!SM70Latency::needs_scoreboards(&make_ffma()));
    assert!(!SM70Latency::needs_scoreboards(&make_imad()));
}

#[test]
fn test_raw_mov_to_mov() {
    let write = make_mov();
    let read = make_mov();
    assert_eq!(SM70Latency::raw(&write, 0, Some(&read), 0), 4);
}

#[test]
fn test_raw_ffma_to_mov() {
    let write = make_ffma();
    let read = make_mov();
    assert_eq!(SM70Latency::raw(&write, 0, Some(&read), 0), 5);
}

#[test]
fn test_raw_imad_to_ffma() {
    let write = make_imad();
    let read = make_ffma();
    assert_eq!(SM70Latency::raw(&write, 0, Some(&read), 0), 4);
}

#[test]
fn test_raw_ld_to_mov() {
    // Decoupled writer, CoupledAlu reader — falls through to default 1cy in RAW table
    assert_eq!(SM70Latency::raw(&make_ld(), 0, Some(&make_mov()), 0), 1);
}

#[test]
fn test_raw_none_uses_fp64_reader_category() {
    // Reader category forced to RedirectedFP64; CoupledFMA writer -> 6cy
    assert_eq!(SM70Latency::raw(&make_ffma(), 0, None, 0), 6);
}

#[test]
fn test_raw_fp64_to_alu() {
    assert_eq!(SM70Latency::raw(&make_dfma(), 0, Some(&make_mov()), 0), 9);
}

#[test]
fn test_raw_alu_to_fp64() {
    assert_eq!(SM70Latency::raw(&make_mov(), 0, Some(&make_dfma()), 0), 6);
}

#[test]
fn test_raw_fp16_to_fma() {
    assert_eq!(SM70Latency::raw(&make_hfma2(), 0, Some(&make_ffma()), 0), 8);
}

#[test]
fn test_raw_imad_to_alu() {
    assert_eq!(SM70Latency::raw(&make_imad(), 0, Some(&make_mov()), 0), 5);
}

#[test]
fn test_raw_decoupled_to_fma() {
    assert_eq!(SM70Latency::raw(&make_ld(), 0, Some(&make_ffma()), 0), 1);
}

#[test]
fn test_raw_fp64_to_fp64() {
    assert_eq!(SM70Latency::raw(&make_dfma(), 0, Some(&make_dadd()), 0), 8);
}

#[test]
fn test_war_mov_to_dfma() {
    assert_eq!(SM70Latency::war(&make_mov(), 0, &make_dfma(), 0), 2);
}

#[test]
fn test_waw_dfma_dfma() {
    assert_eq!(SM70Latency::waw(&make_dfma(), 0, &make_dmul(), 0, false), 1);
}

#[test]
fn test_pred_raw_isetp_to_isetp() {
    let write = make_isetp();
    let read = make_isetp();
    assert_eq!(SM70Latency::raw(&write, 0, Some(&read), 0), 4);
}

#[test]
fn test_pred_raw_isetp_none_reader() {
    let write = make_isetp();
    assert_eq!(SM70Latency::raw(&write, 0, None, 0), 12);
}

#[test]
fn test_pred_war_isetp_to_dsetp() {
    assert_eq!(SM70Latency::war(&make_isetp(), 0, &make_dsetp(), 0), 2);
}

#[test]
fn test_pred_waw_isetp_isetp() {
    let a = make_isetp();
    let b = make_isetp();
    assert_eq!(SM70Latency::waw(&a, 0, &b, 0, false), 1);
}

#[test]
fn test_op_category_redirected_fp64_variants() {
    for op in [
        make_dfma(),
        make_dadd(),
        make_dmul(),
        make_dmnmx(),
        make_dsetp(),
    ] {
        assert!(SM70Latency::needs_scoreboards(&op));
        assert_eq!(SM70Latency::raw(&op, 0, Some(&make_mov()), 0), 9);
    }
}

#[test]
fn test_op_category_redirected_fp16_variants() {
    for op in [make_hfma2(), make_hadd2(), make_hmul2()] {
        assert!(!SM70Latency::needs_scoreboards(&op));
        assert_eq!(SM70Latency::raw(&op, 0, Some(&make_ffma()), 0), 8);
    }
}

#[test]
fn test_op_category_decoupled_conversions_and_bitops() {
    for op in [
        make_f2f(),
        make_popc(),
        make_flo(),
        make_brev(),
        make_transcendental(),
    ] {
        assert!(SM70Latency::needs_scoreboards(&op));
        assert_eq!(SM70Latency::raw(&op, 0, Some(&make_mov()), 0), 1);
    }
}

#[test]
fn test_op_category_decoupled_tex_family() {
    for op in [
        make_tex(),
        make_tld(),
        make_tld4(),
        make_tmml(),
        make_txd(),
        make_txq(),
    ] {
        assert!(SM70Latency::needs_scoreboards(&op));
        assert_eq!(SM70Latency::raw(&op, 0, Some(&make_mov()), 0), 1);
    }
}

#[test]
fn test_op_category_decoupled_atom_and_surface() {
    for op in [make_atom(), make_suld()] {
        assert!(SM70Latency::needs_scoreboards(&op));
        assert_eq!(SM70Latency::raw(&op, 0, Some(&make_mov()), 0), 1);
    }
    assert!(SM70Latency::needs_scoreboards(&make_sust()));
}

#[test]
fn test_cs2r_component_categories() {
    assert_eq!(SM70Latency::raw(&make_cs2r(1), 0, Some(&make_mov()), 0), 4);
    assert_eq!(SM70Latency::raw(&make_cs2r(2), 0, Some(&make_mov()), 0), 6);
    assert!(!SM70Latency::needs_scoreboards(&make_cs2r(1)));
    assert!(!SM70Latency::needs_scoreboards(&make_cs2r(2)));
}

#[test]
fn test_bmov_gpr_category() {
    let op = make_bmov_gpr();
    assert!(!SM70Latency::needs_scoreboards(&op));
    assert_eq!(SM70Latency::war(&make_mov(), 0, &op, 0), 9);
}

#[test]
fn test_unhandled_ops_default_to_decoupled() {
    for op in [make_hmma(), make_imma()] {
        assert!(SM70Latency::needs_scoreboards(&op));
    }
}

#[test]
fn test_needs_scoreboards_all_major_categories() {
    assert!(SM70Latency::needs_scoreboards(&make_ld()));
    assert!(SM70Latency::needs_scoreboards(&make_dfma()));
    assert!(!SM70Latency::needs_scoreboards(&make_mov()));
    assert!(!SM70Latency::needs_scoreboards(&make_ffma()));
    assert!(!SM70Latency::needs_scoreboards(&make_hfma2()));
}

#[test]
fn test_needs_scoreboards_control_flow_ops() {
    // Nop is CoupledDisp - no scoreboards
    assert!(!SM70Latency::needs_scoreboards(&make_nop()));
    // Exit is Decoupled - needs scoreboards
    assert!(SM70Latency::needs_scoreboards(&make_exit()));
}

#[test]
fn test_needs_scoreboards_cctl_decoupled_other() {
    assert!(!SM70Latency::needs_scoreboards(&make_cctl()));
}

#[test]
fn test_raw_imad64_reader_indices() {
    let w = make_imad64();
    // Writer dst is IMADWideUpper; IMad64 readers: src0/src1 → IMADWideAB, src2 → IMADWideLower
    assert_eq!(SM70Latency::raw(&w, 0, Some(&make_imad64()), 0), 6);
    assert_eq!(SM70Latency::raw(&w, 0, Some(&make_imad64()), 1), 6);
    assert_eq!(SM70Latency::raw(&w, 0, Some(&make_imad64()), 2), 2);
}

#[test]
fn test_raw_carry_destination_falls_through_default() {
    let op = make_iadd2_carry_dst();
    assert_eq!(
        SM70Latency::raw(&op, 1, Some(&make_mov()), 0),
        EXPECT_DEFAULT_LATENCY
    );
}

#[test]
fn test_war_carry_write_destination_default() {
    assert_eq!(
        SM70Latency::war(&make_mov(), 0, &make_iadd2_carry_dst(), 1),
        EXPECT_DEFAULT_LATENCY
    );
}

#[test]
fn test_waw_carry_destination_default() {
    let a = make_iadd2_carry_dst();
    let b = make_iadd2_carry_dst();
    assert_eq!(
        SM70Latency::waw(&a, 1, &b, 1, false),
        EXPECT_DEFAULT_LATENCY
    );
}

#[test]
fn test_pred_waw_dsetp_to_dsetp() {
    let a = make_dsetp();
    let b = make_dsetp();
    assert_eq!(SM70Latency::waw(&a, 0, &b, 0, false), 1);
}

#[test]
fn test_pred_war_dsetp_to_isetp() {
    assert_eq!(SM70Latency::war(&make_dsetp(), 0, &make_isetp(), 0), 1);
}
