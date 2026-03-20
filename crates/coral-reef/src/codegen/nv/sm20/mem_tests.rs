// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
//! Unit tests for SM20 memory instruction encoders.

use super::super::encoder::*;
use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    AtomCmpSrc, AtomOp, AtomType, CBuf, CBufRef, CCtlOp, ChannelMask, Dst, ImageAccess, InterpFreq,
    InterpLoc, LdCacheOp, MemAccess, MemAddrType, MemOrder, MemScope, MemSpace, MemType,
    OffsetStride, RegFile, RegRef, Src, SrcMod, SrcRef, SrcSwizzle, StCacheOp, SuGaOffsetMode,
};

fn gpr_src(idx: u32, comps: u8) -> Src {
    Src {
        reference: SrcRef::Reg(RegRef::new(RegFile::GPR, idx, comps)),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn gpr_src_1(idx: u32) -> Src {
    gpr_src(idx, 1)
}

fn pred_true_src() -> Src {
    Src {
        reference: SrcRef::True,
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn cbuf_src(idx: u8, offset: u16) -> Src {
    Src {
        reference: SrcRef::CBuf(CBufRef {
            buf: CBuf::Binding(idx),
            offset,
        }),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
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

fn mem_unit(e: &SM20Encoder<'_>) -> u64 {
    e.get_field(0..3)
}

fn mem_opc(e: &SM20Encoder<'_>) -> u64 {
    e.get_field(58..64)
}

fn global_access(mem_type: MemType, addr: MemAddrType) -> MemAccess {
    MemAccess {
        mem_type,
        space: MemSpace::Global(addr),
        order: MemOrder::Weak,
        eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
    }
}

#[test]
fn op_suldga_binary_encodes_opcode_and_fields() {
    let mut e = encoder_sm30();
    OpSuLdGa {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        mem_type: MemType::B32,
        offset_mode: SuGaOffsetMode::S32,
        cache_op: LdCacheOp::CacheGlobal,
        srcs: [gpr_src_1(2), gpr_src_1(3), pred_true_src()],
    }
    .encode(&mut e);
    assert_eq!(mem_unit(&e), SM20Unit::Mem as u64);
    assert_eq!(mem_opc(&e), 0x35);
    assert_eq!(e.get_field(5..8), 4, "mem_type B32");
    assert_eq!(e.get_field(8..10), 1, "cache global");
    assert_eq!(e.get_field(45..47), 1, "offset_mode S32");
    assert_eq!(e.get_field(14..20), 1);
    assert_eq!(e.get_field(20..26), 3);
}

#[test]
fn op_suldga_formatted_encodes_channel_mask() {
    let mut e = encoder_sm30();
    OpSuLdGa {
        dst: Dst::None,
        mem_type: MemType::B32,
        offset_mode: SuGaOffsetMode::U32,
        cache_op: LdCacheOp::CacheAll,
        srcs: [cbuf_src(5, 0x40), gpr_src_1(6), pred_true_src()],
    }
    .encode(&mut e);
    assert!(e.get_bit(53), "cbuf format");
    assert_eq!(e.get_field(26..40), 0x10, "cb.offset / 4");
    assert_eq!(e.get_field(40..45), 5, "cb idx");
}

#[test]
fn op_sustga_binary_and_formatted() {
    let mut e = encoder_sm30();
    OpSuStGa {
        image_access: ImageAccess::Binary(MemType::B64),
        offset_mode: SuGaOffsetMode::U8,
        cache_op: StCacheOp::WriteBack,
        srcs: [gpr_src_1(2), gpr_src_1(3), gpr_src_1(4), pred_true_src()],
    }
    .encode(&mut e);
    assert_eq!(mem_opc(&e), 0x37);
    assert_eq!(e.get_field(5..8), 5, "mem_type B64");
    assert_eq!(e.get_field(54..58), 0, "binary image_access");

    let mut e = encoder_sm30();
    OpSuStGa {
        image_access: ImageAccess::Formatted(ChannelMask::for_comps(4)),
        offset_mode: SuGaOffsetMode::U32,
        cache_op: StCacheOp::CacheGlobal,
        srcs: [gpr_src_1(1), gpr_src_1(2), gpr_src_1(3), pred_true_src()],
    }
    .encode(&mut e);
    assert_eq!(e.get_field(54..58), 0xf, "formatted mask bits");
}

#[test]
fn op_ld_global_local_shared_and_addr_type() {
    let mut e = encoder_sm30();
    OpLd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 1)),
        addr: gpr_src_1(3),
        offset: 0x1234_5678,
        stride: OffsetStride::X1,
        access: global_access(MemType::B32, MemAddrType::A32),
    }
    .encode(&mut e);
    assert_eq!(mem_opc(&e), 0x20);
    assert!(!e.get_bit(58), "A32");
    assert_eq!(e.get_field(26..58), 0x1234_5678_u64 & ((1 << 32) - 1));

    let mut e = encoder_sm30();
    OpLd {
        dst: Dst::None,
        addr: gpr_src_1(0),
        offset: 0xabc,
        stride: OffsetStride::X1,
        access: global_access(MemType::B64, MemAddrType::A64),
    }
    .encode(&mut e);
    assert!(e.get_bit(58), "A64");

    let mut e = encoder_sm30();
    OpLd {
        dst: Dst::None,
        addr: gpr_src_1(1),
        offset: 0x12345,
        stride: OffsetStride::X1,
        access: MemAccess {
            mem_type: MemType::B32,
            space: MemSpace::Local,
            order: MemOrder::Weak,
            eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
        },
    }
    .encode(&mut e);
    assert_eq!(mem_opc(&e), 0x30);
    assert!(!e.get_bit(56), "local");

    let mut e = encoder_sm30();
    OpLd {
        dst: Dst::None,
        addr: gpr_src_1(1),
        offset: 0xabcde,
        stride: OffsetStride::X1,
        access: MemAccess {
            mem_type: MemType::B32,
            space: MemSpace::Shared,
            order: MemOrder::Weak,
            eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
        },
    }
    .encode(&mut e);
    assert!(e.get_bit(56), "shared");
}

#[test]
fn op_ldc_modes_and_cb_fields() {
    for (mode, want) in [
        (crate::codegen::ir::LdcMode::Indexed, 0_u8),
        (crate::codegen::ir::LdcMode::IndexedLinear, 1_u8),
        (crate::codegen::ir::LdcMode::IndexedSegmented, 2_u8),
        (crate::codegen::ir::LdcMode::IndexedSegmentedLinear, 3_u8),
    ] {
        let mut e = encoder_sm30();
        OpLdc {
            dst: Dst::Reg(RegRef::new(RegFile::GPR, 4, 1)),
            srcs: [cbuf_src(7, 0x80), gpr_src_1(5)],
            mode,
            mem_type: MemType::B32,
        }
        .encode(&mut e);
        assert_eq!(mem_unit(&e), SM20Unit::Tex as u64);
        assert_eq!(mem_opc(&e), 0x05);
        assert_eq!(e.get_field(8..10), u64::from(want));
        assert_eq!(e.get_field(26..42), 0x80);
        assert_eq!(e.get_field(42..47), 7);
    }
}

#[test]
fn op_ld_shared_lock_encodes() {
    let mut e = encoder_sm30();
    OpLdSharedLock {
        dsts: [
            Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
            Dst::Reg(RegRef::new(RegFile::Pred, 2, 1)),
        ],
        addr: gpr_src_1(3),
        offset: 0x12340,
        mem_type: MemType::B64,
    }
    .encode(&mut e);
    assert_eq!(mem_opc(&e), 0x2a);
    assert_eq!(e.get_field(26..50), 0x12340);
}

#[test]
fn op_st_spaces_match_ld() {
    let mut e = encoder_sm30();
    OpSt {
        srcs: [gpr_src_1(2), gpr_src_1(3)],
        offset: 0x100,
        stride: OffsetStride::X1,
        access: global_access(MemType::B32, MemAddrType::A32),
    }
    .encode(&mut e);
    assert_eq!(mem_opc(&e), 0x24);

    let mut e = encoder_sm30();
    OpSt {
        srcs: [gpr_src_1(0), gpr_src_1(0)],
        offset: 0,
        stride: OffsetStride::X1,
        access: MemAccess {
            mem_type: MemType::B32,
            space: MemSpace::Shared,
            order: MemOrder::Weak,
            eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
        },
    }
    .encode(&mut e);
    assert_eq!(mem_opc(&e), 0x32);
    assert!(e.get_bit(56));
}

#[test]
fn op_st_scheck_unlock_encodes_pred_dst() {
    let mut e = encoder_sm30();
    OpStSCheckUnlock {
        locked: Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
        srcs: [gpr_src_1(2), gpr_src_1(3)],
        offset: 0x4000,
        mem_type: MemType::B32,
    }
    .encode(&mut e);
    assert_eq!(mem_opc(&e), 0x2e);
    assert_eq!(e.get_field(26..50), 0x4000);
}

#[test]
fn op_atom_global_rmw_ops_encode_subopcode() {
    let data = gpr_src_1(8);
    let addr = gpr_src_1(9);
    for (op, code) in [
        (AtomOp::Add, 0_u8),
        (AtomOp::Min, 1),
        (AtomOp::Max, 2),
        (AtomOp::Inc, 3),
        (AtomOp::Dec, 4),
        (AtomOp::And, 5),
        (AtomOp::Or, 6),
        (AtomOp::Xor, 7),
        (AtomOp::Exch, 8),
    ] {
        let mut e = encoder_sm30();
        OpAtom {
            dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
            srcs: [addr.clone(), Src::ZERO, data.clone()],
            atom_op: op,
            atom_type: AtomType::U32,
            addr_offset: 0x12345,
            addr_stride: OffsetStride::X1,
            mem_space: MemSpace::Global(MemAddrType::A64),
            mem_order: MemOrder::Constant,
            mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
        }
        .encode(&mut e);
        assert_eq!(e.get_field(5..9), u64::from(code));
    }
}

#[test]
fn op_atom_no_dst_encodes() {
    let mut e = encoder_sm30();
    OpAtom {
        dst: Dst::None,
        srcs: [gpr_src_1(4), Src::ZERO, gpr_src_1(5)],
        atom_op: AtomOp::Add,
        atom_type: AtomType::I32,
        addr_offset: 0xdead,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Global(MemAddrType::A64),
        mem_order: MemOrder::Constant,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(5..9), 0);
    assert_eq!(e.get_field(9..10), 0x7 & 1);
    assert_eq!(e.get_field(59..62), 0x7 >> 1);
}

#[test]
fn op_atom_cmpexch_packed_encodes_second_data_reg() {
    let mut e = encoder_sm30();
    OpAtom {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src_1(10), Src::ZERO, gpr_src(11, 2)],
        atom_op: AtomOp::CmpExch(AtomCmpSrc::Packed),
        atom_type: AtomType::F32,
        addr_offset: 0x1_ffff,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Global(MemAddrType::A64),
        mem_order: MemOrder::Constant,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(5..9), 9, "cmpexch subop");
    assert_eq!(e.get_field(49..55), 12, "second half of packed data");
}

#[test]
fn op_al2p_encodes() {
    let mut e = encoder_sm30();
    OpAL2P {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        offset: gpr_src_1(2),
        addr: 0x7ff,
        comps: 4,
        output: true,
    }
    .encode(&mut e);
    assert_eq!(mem_unit(&e), SM20Unit::Tex as u64);
    assert_eq!(mem_opc(&e), 0x03);
    assert_eq!(e.get_field(5..7), 2, "ilog2 comps");
    assert!(e.get_bit(9), "output");
    assert_eq!(e.get_field(32..43), 0x7ff);
}

#[test]
fn op_ald_non_patch_zero_offset_encodes() {
    let mut e = encoder_sm30();
    OpALd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src_1(3), Src::ZERO],
        addr: 0x200,
        comps: 3,
        patch: false,
        output: false,
        phys: false,
    }
    .encode(&mut e);
    assert_eq!(mem_opc(&e), 0x01);
    assert_eq!(e.get_field(5..7), 2, "comps-1");
    assert!(!e.get_bit(8), "patch");
    assert_eq!(e.get_field(32..42), 0x200);
}

#[test]
fn op_ald_phys_encodes() {
    let mut e = encoder_sm30();
    OpALd {
        dst: Dst::None,
        srcs: [gpr_src_1(4), gpr_src_1(5)],
        addr: 0x100,
        comps: 2,
        patch: false,
        output: false,
        phys: true,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(32..42), 0x100);
}

#[test]
fn op_ast_encodes() {
    let mut e = encoder_sm30();
    OpASt {
        srcs: [gpr_src_1(6), Src::ZERO, gpr_src_1(7)],
        addr: 0x300,
        comps: 2,
        patch: true,
        phys: false,
    }
    .encode(&mut e);
    assert_eq!(mem_opc(&e), 0x02);
    assert!(e.get_bit(8), "patch");
    assert_eq!(e.get_field(32..42), 0x300);
}

#[test]
fn op_ipa_freq_and_loc() {
    for (freq, fc) in [
        (InterpFreq::Pass, 0_u8),
        (InterpFreq::PassMulW, 1),
        (InterpFreq::Constant, 2),
        (InterpFreq::State, 3),
    ] {
        for (loc, lc) in [
            (InterpLoc::Default, 0_u8),
            (InterpLoc::Centroid, 1),
            (InterpLoc::Offset, 2),
        ] {
            let mut e = encoder_sm30();
            OpIpa {
                dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
                addr: 0x40,
                freq,
                loc,
                srcs: [Src::ZERO, Src::ZERO],
            }
            .encode(&mut e);
            assert_eq!(e.get_field(6..8), u64::from(fc));
            assert_eq!(e.get_field(8..10), u64::from(lc));
        }
    }
}

#[test]
fn op_cctl_global_a32_a64_and_shared() {
    let mut e = encoder_sm30();
    OpCCtl {
        mem_space: MemSpace::Global(MemAddrType::A32),
        op: CCtlOp::Qry1,
        addr: gpr_src_1(2),
        addr_offset: 0x100,
    }
    .encode(&mut e);
    assert_eq!(mem_opc(&e), 0x26);
    assert_eq!(e.get_field(5..10), 0);
    assert_eq!(e.get_field(28..58), 0x40, "offset / 4");

    let mut e = encoder_sm30();
    OpCCtl {
        mem_space: MemSpace::Global(MemAddrType::A64),
        op: CCtlOp::WB,
        addr: gpr_src_1(0),
        addr_offset: 0x20,
    }
    .encode(&mut e);
    assert_eq!(mem_opc(&e), 0x27);

    let mut e = encoder_sm30();
    OpCCtl {
        mem_space: MemSpace::Shared,
        op: CCtlOp::IV,
        addr: gpr_src_1(1),
        addr_offset: 0x400,
    }
    .encode(&mut e);
    assert_eq!(mem_opc(&e), 0x34);
    assert_eq!(e.get_field(28..50), 0x100);
}

#[test]
fn op_membar_scopes() {
    for (scope, bits) in [
        (MemScope::CTA, 0_u8),
        (MemScope::GPU, 1),
        (MemScope::System, 2),
    ] {
        let mut e = encoder_sm30();
        OpMemBar { scope }.encode(&mut e);
        assert_eq!(mem_opc(&e), 0x38);
        assert_eq!(e.get_field(5..7), u64::from(bits));
    }
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn op_ldc_requires_cbuf() {
    let mut e = encoder_sm30();
    OpLdc {
        dst: Dst::None,
        srcs: [gpr_src_1(0), Src::ZERO],
        mode: crate::codegen::ir::LdcMode::Indexed,
        mem_type: MemType::B32,
    }
    .encode(&mut e);
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn op_cctl_local_ice() {
    let mut e = encoder_sm30();
    OpCCtl {
        mem_space: MemSpace::Local,
        op: CCtlOp::PF1,
        addr: gpr_src_1(0),
        addr_offset: 0,
    }
    .encode(&mut e);
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn op_cctl_ivallp_unsupported_sm20() {
    let mut e = encoder_sm30();
    OpCCtl {
        mem_space: MemSpace::Global(MemAddrType::A32),
        op: CCtlOp::IVAllP,
        addr: gpr_src_1(0),
        addr_offset: 0x40,
    }
    .encode(&mut e);
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn op_atom_shared_mem_space_ice() {
    let mut e = encoder_sm30();
    OpAtom {
        dst: Dst::None,
        srcs: [gpr_src_1(0), Src::ZERO, gpr_src_1(1)],
        atom_op: AtomOp::Add,
        atom_type: AtomType::U32,
        addr_offset: 0,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Shared,
        mem_order: MemOrder::Constant,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
    }
    .encode(&mut e);
}
