// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
//! Unit tests for SM70 memory instruction encoders.

use super::super::encoder::{SM70Encoder, SM70Op};
use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    AtomCmpSrc, AtomOp, AtomType, CBuf, CBufRef, CCtlOp, ChannelMask, Dst, ImageAccess, ImageDim,
    InterpFreq, InterpLoc, LdcMode, MemAccess, MemAddrType, MemEvictionPriority, MemOrder,
    MemScope, MemSpace, MemType, OffsetStride, OpAL2P, OpALd, OpASt, OpAtom, OpCCtl, OpIpa, OpLd,
    OpLdTram, OpLdc, OpMemBar, OpSt, OpSuAtom, OpSuLd, OpSuSt, RegFile, RegRef, Src, SrcMod,
    SrcRef, SrcSwizzle,
};

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn encoder(sm: u8) -> SM70Encoder<'static> {
    let labels: &'static FxHashMap<crate::codegen::ir::Label, usize> =
        Box::leak(Box::new(FxHashMap::default()));
    SM70Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 4],
    }
}

fn opc(e: &SM70Encoder<'_>) -> u64 {
    e.get_field(0..12)
}

fn access_shared_cta(ty: MemType) -> MemAccess {
    MemAccess {
        mem_type: ty,
        space: MemSpace::Shared,
        order: MemOrder::Strong(MemScope::CTA),
        eviction_priority: MemEvictionPriority::Normal,
    }
}

#[test]
fn op_suld_sust_surface_binary_formatted() {
    let mut e = encoder(80);
    OpSuLd {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        image_access: ImageAccess::Binary(MemType::B32),
        image_dim: ImageDim::_2D,
        mem_order: MemOrder::Weak,
        mem_eviction_priority: MemEvictionPriority::Normal,
        srcs: [gpr_src(10), gpr_src(11)],
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x99a);

    let mut e = encoder(80);
    OpSuLd {
        dsts: [Dst::None, Dst::None],
        image_access: ImageAccess::Formatted(ChannelMask::for_comps(4)),
        image_dim: ImageDim::_1D,
        mem_order: MemOrder::Weak,
        mem_eviction_priority: MemEvictionPriority::Normal,
        srcs: [gpr_src(5), gpr_src(6)],
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x998);

    let mut e = encoder(80);
    OpSuSt {
        image_access: ImageAccess::Binary(MemType::B64),
        image_dim: ImageDim::_3D,
        mem_order: MemOrder::Weak,
        mem_eviction_priority: MemEvictionPriority::Normal,
        srcs: [gpr_src(1), gpr_src(2), gpr_src(3)],
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x99e);
}

#[test]
fn op_suatom_paths() {
    let mut e = encoder(80);
    OpSuAtom {
        dsts: [Dst::None, Dst::None],
        image_dim: ImageDim::_1D,
        atom_op: AtomOp::Add,
        atom_type: AtomType::U32,
        mem_order: MemOrder::Weak,
        mem_eviction_priority: MemEvictionPriority::Normal,
        srcs: [gpr_src(4), gpr_src(5), gpr_src(6)],
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x3a0);

    let mut e = encoder(80);
    OpSuAtom {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 2, 1)), Dst::None],
        image_dim: ImageDim::_2D,
        atom_op: AtomOp::Min,
        atom_type: AtomType::F32,
        mem_order: MemOrder::Weak,
        mem_eviction_priority: MemEvictionPriority::Normal,
        srcs: [gpr_src(1), gpr_src(2), gpr_src(3)],
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x394);

    let mut e = encoder(80);
    OpSuAtom {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        image_dim: ImageDim::_2D,
        atom_op: AtomOp::CmpExch(AtomCmpSrc::Packed),
        atom_type: AtomType::U32,
        mem_order: MemOrder::Weak,
        mem_eviction_priority: MemEvictionPriority::Normal,
        srcs: [gpr_src(1), gpr_src(2), gpr_src(3)],
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x396);
}

#[test]
fn op_ld_global_local_shared() {
    let mut e = encoder(80);
    OpLd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        addr: gpr_src(2),
        offset: 0x1000,
        stride: OffsetStride::X1,
        access: MemAccess {
            mem_type: MemType::B32,
            space: MemSpace::Global(MemAddrType::A64),
            order: MemOrder::Constant,
            eviction_priority: MemEvictionPriority::Normal,
        },
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x381);

    let mut e = encoder(80);
    OpLd {
        dst: Dst::None,
        addr: gpr_src(0),
        offset: 0,
        stride: OffsetStride::X1,
        access: MemAccess {
            mem_type: MemType::B32,
            space: MemSpace::Local,
            order: MemOrder::Strong(MemScope::CTA),
            eviction_priority: MemEvictionPriority::Normal,
        },
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x983);

    let mut e = encoder(75);
    OpLd {
        dst: Dst::None,
        addr: gpr_src(1),
        offset: 0,
        stride: OffsetStride::X4,
        access: access_shared_cta(MemType::B32),
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x984);
    assert_eq!(e.get_field(78..80), 1, "stride x4");
}

#[test]
fn op_st_global_local_shared() {
    let mut e = encoder(80);
    OpSt {
        srcs: [gpr_src(1), gpr_src(2)],
        offset: 0x40,
        stride: OffsetStride::X1,
        access: MemAccess {
            mem_type: MemType::B64,
            space: MemSpace::Global(MemAddrType::A32),
            order: MemOrder::Weak,
            eviction_priority: MemEvictionPriority::Normal,
        },
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x386);

    let mut e = encoder(80);
    OpSt {
        srcs: [gpr_src(0), gpr_src(0)],
        offset: 0,
        stride: OffsetStride::X1,
        access: MemAccess {
            mem_type: MemType::B32,
            space: MemSpace::Local,
            order: MemOrder::Strong(MemScope::CTA),
            eviction_priority: MemEvictionPriority::Normal,
        },
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x387);

    let mut e = encoder(75);
    OpSt {
        srcs: [gpr_src(2), gpr_src(3)],
        offset: 0x80,
        stride: OffsetStride::X1,
        access: access_shared_cta(MemType::B32),
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x388);
}

#[test]
fn op_ldc_binding_non_uniform() {
    let mut e = encoder(80);
    OpLdc {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 3, 1)),
        srcs: [
            Src {
                reference: SrcRef::CBuf(CBufRef {
                    buf: CBuf::Binding(11),
                    offset: 0x20,
                }),
                modifier: SrcMod::None,
                swizzle: SrcSwizzle::None,
            },
            gpr_src(4),
        ],
        mode: LdcMode::IndexedLinear,
        mem_type: MemType::B32,
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0xb82);
    assert_eq!(e.get_field(78..80), 1, "ldc mode");
}

#[test]
fn op_atom_global_and_shared() {
    let mut e = encoder(80);
    OpAtom {
        dst: Dst::None,
        srcs: [gpr_src(1), Src::ZERO, gpr_src(2)],
        atom_op: AtomOp::And,
        atom_type: AtomType::U32,
        addr_offset: 0,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Global(MemAddrType::A32),
        mem_order: MemOrder::Weak,
        mem_eviction_priority: MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x98e);

    let mut e = encoder(90);
    OpAtom {
        dst: Dst::None,
        srcs: [gpr_src(1), Src::ZERO, gpr_src(2)],
        atom_op: AtomOp::Add,
        atom_type: AtomType::F32,
        addr_offset: 0,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Global(MemAddrType::A64),
        mem_order: MemOrder::Weak,
        mem_eviction_priority: MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x9a6);

    let mut e = encoder(80);
    OpAtom {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        atom_op: AtomOp::CmpExch(AtomCmpSrc::Separate),
        atom_type: AtomType::U32,
        addr_offset: 0,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Global(MemAddrType::A32),
        mem_order: MemOrder::Weak,
        mem_eviction_priority: MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x3a9);

    let mut e = encoder(75);
    OpAtom {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        atom_op: AtomOp::CmpExch(AtomCmpSrc::Separate),
        atom_type: AtomType::U32,
        addr_offset: 0x40,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Shared,
        mem_order: MemOrder::Strong(MemScope::CTA),
        mem_eviction_priority: MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x38d);

    let mut e = encoder(75);
    OpAtom {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), Src::ZERO, gpr_src(3)],
        atom_op: AtomOp::Xor,
        atom_type: AtomType::U32,
        addr_offset: 0,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Shared,
        mem_order: MemOrder::Strong(MemScope::CTA),
        mem_eviction_priority: MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x38c);
}

#[test]
fn op_al2p_ald_ast_ipa() {
    let mut e = encoder(80);
    OpAL2P {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        offset: gpr_src(2),
        addr: 0x100,
        comps: 1,
        output: true,
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x920);

    let mut e = encoder(80);
    OpALd {
        dst: Dst::None,
        srcs: [gpr_src(3), gpr_src(4)],
        addr: 0x200,
        comps: 2,
        patch: false,
        output: false,
        phys: false,
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x321);

    let mut e = encoder(80);
    OpASt {
        srcs: [gpr_src(5), gpr_src(6), gpr_src(7)],
        addr: 0x80,
        comps: 2,
        patch: true,
        phys: false,
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x322);

    let mut e = encoder(80);
    OpIpa {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        addr: 0x40,
        freq: InterpFreq::Constant,
        loc: InterpLoc::Centroid,
        srcs: [Src::ZERO, gpr_src(2)],
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x326);
}

#[test]
fn op_ldtram_encodes() {
    let mut e = encoder(80);
    OpLdTram {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        addr: 0x80,
        use_c: true,
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x3ad);
    assert!(e.get_bit(72), "use_c");
}

#[test]
fn op_cctl_and_membar() {
    let mut e = encoder(80);
    OpCCtl {
        mem_space: MemSpace::Global(MemAddrType::A32),
        op: CCtlOp::WB,
        addr: gpr_src(1),
        addr_offset: 0x1000,
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x98f);
    assert_eq!(e.get_field(87..91), 2);

    let mut e = encoder(80);
    OpMemBar {
        scope: MemScope::System,
    }
    .encode(&mut e);
    assert_eq!(opc(&e), 0x992);
    assert_eq!(e.get_field(76..79), 3);
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn op_atom_local_ice() {
    let mut e = encoder(80);
    OpAtom {
        dst: Dst::None,
        srcs: [gpr_src(0), Src::ZERO, gpr_src(1)],
        atom_op: AtomOp::Add,
        atom_type: AtomType::U32,
        addr_offset: 0,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Local,
        mem_order: MemOrder::Weak,
        mem_eviction_priority: MemEvictionPriority::Normal,
    }
    .encode(&mut e);
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn op_ipa_pass_mul_w_ice() {
    let mut e = encoder(80);
    OpIpa {
        dst: Dst::None,
        addr: 0x20,
        freq: InterpFreq::PassMulW,
        loc: InterpLoc::Default,
        srcs: [Src::ZERO, Src::ZERO],
    }
    .encode(&mut e);
}
