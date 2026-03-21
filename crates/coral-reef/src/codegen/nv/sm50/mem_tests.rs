// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
//! Unit tests for SM50 memory instruction encoders.

use super::super::encoder::*;
use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    AtomCmpSrc, AtomOp, AtomType, CBuf, CBufRef, CCtlOp, ChannelMask, Dst, ImageAccess, ImageDim,
    InterpFreq, InterpLoc, LdcMode, MemAccess, MemAddrType, MemOrder, MemScope, MemSpace, MemType,
    OffsetStride, RegFile, RegRef, Src, SrcMod, SrcRef, SrcSwizzle,
};

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn ssa_pair(a: u32, b: u32) -> [Src; 2] {
    [
        Src {
            reference: RegRef::new(RegFile::GPR, a, 1).into(),
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        },
        Src {
            reference: RegRef::new(RegFile::GPR, b, 1).into(),
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        },
    ]
}

fn sm50_encoder() -> SM50Encoder<'static> {
    let sm: &'static ShaderModel50 = Box::leak(Box::new(ShaderModel50::new(52)));
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

fn access_gl(ty: MemType, at: MemAddrType) -> MemAccess {
    MemAccess {
        mem_type: ty,
        space: MemSpace::Global(at),
        order: MemOrder::Weak,
        eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
    }
}

#[test]
fn op_suld_binary_and_formatted() {
    let mut e = sm50_encoder();
    OpSuLd {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        image_access: ImageAccess::Binary(MemType::B64),
        image_dim: ImageDim::_2DArray,
        mem_order: MemOrder::Weak,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
        srcs: ssa_pair(10, 11),
    }
    .encode(&mut e);
    assert!(e.get_bit(52), "binary .B");
    assert_eq!(e.get_field(33..36), 4, "2DArray");

    let mut e = sm50_encoder();
    OpSuLd {
        dsts: [Dst::None, Dst::None],
        image_access: ImageAccess::Formatted(ChannelMask::for_comps(4)),
        image_dim: ImageDim::_1D,
        mem_order: MemOrder::Weak,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
        srcs: ssa_pair(10, 11),
    }
    .encode(&mut e);
    assert!(!e.get_bit(52), "formatted .P");
    assert_eq!(e.get_field(20..24), 0xf);
}

#[test]
fn op_sust_encodes() {
    let mut e = sm50_encoder();
    OpSuSt {
        image_access: ImageAccess::Binary(MemType::B32),
        image_dim: ImageDim::_3D,
        mem_order: MemOrder::Weak,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
        srcs: [
            Src {
                reference: RegRef::new(RegFile::GPR, 8, 1).into(),
                modifier: SrcMod::None,
                swizzle: SrcSwizzle::None,
            },
            Src {
                reference: RegRef::new(RegFile::GPR, 9, 1).into(),
                modifier: SrcMod::None,
                swizzle: SrcSwizzle::None,
            },
            gpr_src(10),
        ],
    }
    .encode(&mut e);
    assert_eq!(e.get_field(33..36), 5, "3D");
}

#[test]
fn op_suatom_rmw_and_cmpexch() {
    let mut e = sm50_encoder();
    OpSuAtom {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        image_dim: ImageDim::_2D,
        atom_op: AtomOp::Xor,
        atom_type: AtomType::I32,
        mem_order: MemOrder::Weak,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
        srcs: [
            Src {
                reference: RegRef::new(RegFile::GPR, 5, 1).into(),
                modifier: SrcMod::None,
                swizzle: SrcSwizzle::None,
            },
            Src {
                reference: RegRef::new(RegFile::GPR, 6, 1).into(),
                modifier: SrcMod::None,
                swizzle: SrcSwizzle::None,
            },
            gpr_src(7),
        ],
    }
    .encode(&mut e);
    assert_eq!(e.get_field(36..39), 1, "I32 atom type");
    assert!(e.get_bit(52), ".D");

    let mut e = sm50_encoder();
    OpSuAtom {
        dsts: [Dst::None, Dst::None],
        image_dim: ImageDim::_1D,
        atom_op: AtomOp::CmpExch(AtomCmpSrc::Packed),
        atom_type: AtomType::U32,
        mem_order: MemOrder::Weak,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
        srcs: [
            Src {
                reference: RegRef::new(RegFile::GPR, 1, 1).into(),
                modifier: SrcMod::None,
                swizzle: SrcSwizzle::None,
            },
            Src {
                reference: RegRef::new(RegFile::GPR, 2, 1).into(),
                modifier: SrcMod::None,
                swizzle: SrcSwizzle::None,
            },
            gpr_src(3),
        ],
    }
    .encode(&mut e);
    assert_eq!(e.get_field(36..39), 0, "U32 cmpexch atom type");
}

#[test]
fn op_ld_st_global_mem_access() {
    let mut e = sm50_encoder();
    OpLd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 1)),
        addr: gpr_src(3),
        offset: 0x1000,
        stride: OffsetStride::X1,
        access: access_gl(MemType::B32, MemAddrType::A64),
    }
    .encode(&mut e);
    assert_eq!(e.get_field(45..46), 1, "A64");
    assert_eq!(e.get_field(48..51), 4, "B32");

    let mut e = sm50_encoder();
    OpSt {
        srcs: [gpr_src(4), gpr_src(5)],
        offset: 0x20,
        stride: OffsetStride::X1,
        access: access_gl(MemType::B64, MemAddrType::A32),
    }
    .encode(&mut e);
    assert_eq!(e.get_field(45..46), 0, "A32");
    assert_eq!(e.get_field(48..51), 5, "B64");
}

#[test]
fn op_ld_st_local_shared_opcodes() {
    let mut e = sm50_encoder();
    OpLd {
        dst: Dst::None,
        addr: gpr_src(0),
        offset: 0,
        stride: OffsetStride::X1,
        access: MemAccess {
            mem_type: MemType::B32,
            space: MemSpace::Local,
            order: MemOrder::Weak,
            eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
        },
    }
    .encode(&mut e);
    assert_eq!(e.get_field(48..51), 4, "B32 local");

    let mut e = sm50_encoder();
    OpSt {
        srcs: [gpr_src(0), gpr_src(0)],
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
    assert_eq!(e.get_field(48..51), 4, "B32 shared st");
}

#[test]
fn op_ldc_segmented() {
    let mut e = sm50_encoder();
    OpLdc {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [
            Src {
                reference: SrcRef::CBuf(CBufRef {
                    buf: CBuf::Binding(5),
                    offset: 0x40,
                }),
                modifier: SrcMod::None,
                swizzle: SrcSwizzle::None,
            },
            gpr_src(2),
        ],
        mode: LdcMode::IndexedSegmented,
        mem_type: MemType::B64,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(44..46), 2);
    assert_eq!(e.get_field(48..51), 5, "B64 ldc");
}

#[test]
fn op_atom_global_add_and_cmpswap_subops() {
    let mut e = sm50_encoder();
    OpAtom {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), Src::ZERO, gpr_src(3)],
        atom_op: AtomOp::Add,
        atom_type: AtomType::U32,
        addr_offset: 0x10,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Global(MemAddrType::A64),
        mem_order: MemOrder::Constant,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(52..56), 0, "global add rmw");

    let mut e = sm50_encoder();
    OpAtom {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(4), Src::ZERO, gpr_src(5)],
        atom_op: AtomOp::CmpExch(AtomCmpSrc::Packed),
        atom_type: AtomType::U32,
        addr_offset: 0,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Global(MemAddrType::A64),
        mem_order: MemOrder::Constant,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(49..52), 0, "U32 cmpexch type");
    assert_eq!(e.get_field(50..52), 0, "packed layout");
}

#[test]
fn op_atom_global_paths() {
    let mut e = sm50_encoder();
    OpAtom {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), Src::ZERO, gpr_src(3)],
        atom_op: AtomOp::Or,
        atom_type: AtomType::U64,
        addr_offset: 0x12345,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Global(MemAddrType::A32),
        mem_order: MemOrder::Constant,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(49..52), 2, "U64");
    assert_eq!(e.get_field(48..49), 0, "A32 addr");

    let mut e = sm50_encoder();
    OpAtom {
        dst: Dst::None,
        srcs: [gpr_src(0), Src::ZERO, gpr_src(1)],
        atom_op: AtomOp::Min,
        atom_type: AtomType::I32,
        addr_offset: 0,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Global(MemAddrType::A64),
        mem_order: MemOrder::Constant,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(20..23), 1, "I32 type no-dst");

    let mut e = sm50_encoder();
    OpAtom {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(4), gpr_src(5), gpr_src(6)],
        atom_op: AtomOp::CmpExch(AtomCmpSrc::Packed),
        atom_type: AtomType::U64,
        addr_offset: 0,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Global(MemAddrType::A32),
        mem_order: MemOrder::Constant,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(49..50), 1, "U64 cmpexch");
}

#[test]
fn op_atom_shared() {
    let mut e = sm50_encoder();
    OpAtom {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), Src::ZERO, gpr_src(3)],
        atom_op: AtomOp::Exch,
        atom_type: AtomType::U64,
        addr_offset: 0x100,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Shared,
        mem_order: MemOrder::Strong(MemScope::CTA),
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(28..30), 2, "U64 shared");

    let mut e = sm50_encoder();
    OpAtom {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(1), Src::ZERO, gpr_src(2)],
        atom_op: AtomOp::CmpExch(AtomCmpSrc::Packed),
        atom_type: AtomType::U32,
        addr_offset: 0x40,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Shared,
        mem_order: MemOrder::Strong(MemScope::CTA),
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(52..56), 4, "shared cmpexch subop");
}

#[test]
fn op_al2p_ald_ast_ipa() {
    let mut e = sm50_encoder();
    OpAL2P {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        offset: gpr_src(2),
        addr: 0x200,
        comps: 1,
        output: false,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(20..31), 0x200);

    let mut e = sm50_encoder();
    OpALd {
        dst: Dst::None,
        srcs: [gpr_src(3), Src::ZERO],
        addr: 0x100,
        comps: 2,
        patch: true,
        output: true,
        phys: false,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(47..49), 1, "comps-1");

    let mut e = sm50_encoder();
    OpASt {
        srcs: [gpr_src(4), Src::ZERO, gpr_src(5)],
        addr: 0x80,
        comps: 2,
        patch: false,
        phys: false,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(47..49), 1);

    let mut e = sm50_encoder();
    OpIpa {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        addr: 0x40,
        freq: InterpFreq::Constant,
        loc: InterpLoc::Offset,
        srcs: [Src::ZERO, gpr_src(2)],
    }
    .encode(&mut e);
    assert_eq!(e.get_field(52..54), 2);
    assert_eq!(e.get_field(54..56), 2);
}

#[test]
fn op_ldc_linear_mode_and_membar_system() {
    let mut e = sm50_encoder();
    OpLdc {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [
            Src {
                reference: SrcRef::CBuf(CBufRef {
                    buf: CBuf::Binding(1),
                    offset: 0,
                }),
                modifier: SrcMod::None,
                swizzle: SrcSwizzle::None,
            },
            gpr_src(2),
        ],
        mode: LdcMode::IndexedLinear,
        mem_type: MemType::B32,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(44..46), 1, "indexed linear");

    let mut e = sm50_encoder();
    OpMemBar {
        scope: MemScope::System,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(8..10), 2, "system scope");
}

#[test]
fn op_cctl_and_membar() {
    let mut e = sm50_encoder();
    OpCCtl {
        mem_space: MemSpace::Global(MemAddrType::A32),
        op: CCtlOp::PF2,
        addr: gpr_src(0),
        addr_offset: 0x100,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(0..4), 3, "PF2");

    let mut e = sm50_encoder();
    OpCCtl {
        mem_space: MemSpace::Shared,
        op: CCtlOp::IVAll,
        addr: gpr_src(1),
        addr_offset: 0x200,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(0..4), 6, "IVAll");

    let mut e = sm50_encoder();
    OpMemBar {
        scope: MemScope::GPU,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(48..64), 0xef98);
    assert_eq!(e.get_field(8..10), 1);
}

#[test]
fn op_cctl_global_a64_and_qry1() {
    let mut e = sm50_encoder();
    OpCCtl {
        mem_space: MemSpace::Global(MemAddrType::A64),
        op: CCtlOp::Qry1,
        addr: gpr_src(3),
        addr_offset: 0x40,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(0..4), 0, "Qry1");
    assert_eq!(e.get_field(52..53), 1, "A64");
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn op_atom_local_ice() {
    let mut e = sm50_encoder();
    OpAtom {
        dst: Dst::None,
        srcs: [gpr_src(0), Src::ZERO, gpr_src(1)],
        atom_op: AtomOp::Add,
        atom_type: AtomType::U32,
        addr_offset: 0,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Local,
        mem_order: MemOrder::Constant,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
    }
    .encode(&mut e);
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn op_cctl_local_ice() {
    let mut e = sm50_encoder();
    OpCCtl {
        mem_space: MemSpace::Local,
        op: CCtlOp::PF1,
        addr: gpr_src(0),
        addr_offset: 0,
    }
    .encode(&mut e);
}
