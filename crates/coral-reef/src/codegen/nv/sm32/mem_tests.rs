// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
//! Unit tests for SM32 memory instruction encoders.

use super::super::encoder::*;
use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    AtomCmpSrc, AtomOp, AtomType, CBuf, CBufRef, Dst, InterpFreq, InterpLoc, LdcMode, MemAccess,
    MemAddrType, MemOrder, MemSpace, MemType, OffsetStride, RegFile, RegRef, Src, SrcMod, SrcRef,
    SrcSwizzle,
};

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn cbuf_src(idx: u8, off: u16) -> Src {
    Src {
        reference: SrcRef::CBuf(CBufRef {
            buf: CBuf::Binding(idx),
            offset: off,
        }),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn encoder_sm35() -> SM32Encoder<'static> {
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

fn fu(e: &SM32Encoder<'_>) -> u64 {
    e.get_field(0..2)
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
fn op_ld_global_a32_a64_mem_access_and_fu() {
    let mut e = encoder_sm35();
    OpLd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        addr: gpr_src(2),
        offset: 0x1234_5678,
        stride: OffsetStride::X1,
        access: access_gl(MemType::B32, MemAddrType::A32),
    }
    .encode(&mut e);
    assert_eq!(fu(&e), 0);
    assert_eq!(e.get_field(55..56), 0, "A32");
    assert_eq!(e.get_field(56..59), 4, "B32 mem type");

    let mut e = encoder_sm35();
    OpLd {
        dst: Dst::None,
        addr: gpr_src(0),
        offset: 0,
        stride: OffsetStride::X1,
        access: access_gl(MemType::B64, MemAddrType::A64),
    }
    .encode(&mut e);
    assert_eq!(e.get_field(55..56), 1, "A64");
    assert_eq!(e.get_field(56..59), 5, "B64");
}

#[test]
fn op_ld_local_and_shared_mem_access() {
    let mut e = encoder_sm35();
    OpLd {
        dst: Dst::None,
        addr: gpr_src(3),
        offset: 0xabc,
        stride: OffsetStride::X1,
        access: MemAccess {
            mem_type: MemType::B32,
            space: MemSpace::Local,
            order: MemOrder::Weak,
            eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
        },
    }
    .encode(&mut e);
    assert_eq!(fu(&e), 2);
    assert_eq!(e.get_field(50..54) & 1, 0, "local addr kind");

    let mut e = encoder_sm35();
    OpLd {
        dst: Dst::None,
        addr: gpr_src(1),
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
    assert_eq!(e.get_field(50..54) & 1, 0);
}

#[test]
fn op_ldc_indirect_const_mode_and_cb() {
    let mut e = encoder_sm35();
    OpLdc {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 1)),
        srcs: [cbuf_src(9, 0x20), gpr_src(4)],
        mode: LdcMode::IndexedLinear,
        mem_type: MemType::B32,
    }
    .encode(&mut e);
    assert_eq!(fu(&e), 2);
    assert_eq!(e.get_field(47..49), 1);
    assert_eq!(e.get_field(39..44), 9);
}

#[test]
fn op_ld_shared_lock_encodes() {
    let mut e = encoder_sm35();
    OpLdSharedLock {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        addr: gpr_src(2),
        offset: 0x1000,
        mem_type: MemType::B64,
    }
    .encode(&mut e);
    assert_eq!(fu(&e), 2);
    assert_eq!(e.get_field(23..47), 0x1000);
}

#[test]
fn op_st_global_and_shared_fields() {
    let mut e = encoder_sm35();
    OpSt {
        srcs: [gpr_src(1), gpr_src(2)],
        offset: 0x40,
        stride: OffsetStride::X1,
        access: access_gl(MemType::B32, MemAddrType::A32),
    }
    .encode(&mut e);
    assert_eq!(fu(&e), 0);
    assert_eq!(e.get_field(55..56), 0, "global A32");

    let mut e = encoder_sm35();
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
    assert_eq!(e.get_field(50..54) & 1, 0);
}

#[test]
fn op_st_scheck_unlock() {
    let mut e = encoder_sm35();
    OpStSCheckUnlock {
        locked: Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3)],
        offset: 0x200,
        mem_type: MemType::B32,
    }
    .encode(&mut e);
    assert_eq!(fu(&e), 2);
    assert_eq!(e.get_field(51..54), 4, "mem_type B32");
    assert_eq!(e.get_field(23..47), 0x200);
}

#[test]
fn op_atom_non_cmpexch_types_and_addr_type() {
    let mut e = encoder_sm35();
    OpAtom {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), Src::ZERO, gpr_src(3)],
        atom_op: AtomOp::Max,
        atom_type: AtomType::F32,
        addr_offset: 0xabcde,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Global(MemAddrType::A64),
        mem_order: MemOrder::Constant,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(fu(&e), 2);
    assert_eq!(e.get_field(52..55), 3, "F32 data type");
    assert_eq!(e.get_field(51..52), 1, "A64");

    let mut e = encoder_sm35();
    OpAtom {
        dst: Dst::None,
        srcs: [gpr_src(0), Src::ZERO, gpr_src(1)],
        atom_op: AtomOp::Min,
        atom_type: AtomType::I32,
        addr_offset: 0,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Global(MemAddrType::A32),
        mem_order: MemOrder::Constant,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(51..52), 0, "A32");
}

#[test]
fn op_atom_cmpexch_packed_u64() {
    let mut e = encoder_sm35();
    OpAtom {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), Src::ZERO, gpr_src(3)],
        atom_op: AtomOp::CmpExch(AtomCmpSrc::Packed),
        atom_type: AtomType::U64,
        addr_offset: 0,
        addr_stride: OffsetStride::X1,
        mem_space: MemSpace::Global(MemAddrType::A32),
        mem_order: MemOrder::Constant,
        mem_eviction_priority: crate::codegen::ir::MemEvictionPriority::Normal,
    }
    .encode(&mut e);
    assert_eq!(fu(&e), 2);
    assert_eq!(e.get_field(52..53), 1, "U64");
    assert_eq!(e.get_field(53..55), 0, "packed layout");
}

#[test]
fn op_al2p_encodes() {
    let mut e = encoder_sm35();
    OpAL2P {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        offset: gpr_src(2),
        addr: 0x123,
        comps: 1,
        output: true,
    }
    .encode(&mut e);
    assert_eq!(fu(&e), 2);
    assert!(e.get_bit(35), "output");
}

#[test]
fn op_ald_encodes_patch_output() {
    let mut e = encoder_sm35();
    OpALd {
        dst: Dst::None,
        srcs: [gpr_src(1), Src::ZERO],
        addr: 0x400,
        comps: 4,
        patch: true,
        output: true,
        phys: false,
    }
    .encode(&mut e);
    assert_eq!(fu(&e), 2);
    assert!(e.get_bit(34));
    assert!(e.get_bit(35));
}

#[test]
fn op_ast_encodes() {
    let mut e = encoder_sm35();
    OpASt {
        srcs: [gpr_src(4), Src::ZERO, gpr_src(5)],
        addr: 0x200,
        comps: 2,
        patch: false,
        phys: false,
    }
    .encode(&mut e);
    assert_eq!(fu(&e), 2);
}

#[test]
fn op_ipa_loc_and_freq() {
    let mut e = encoder_sm35();
    OpIpa {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        addr: 0x80,
        freq: InterpFreq::State,
        loc: InterpLoc::Centroid,
        srcs: [Src::ZERO, gpr_src(2)],
    }
    .encode(&mut e);
    assert_eq!(e.get_field(51..53), 1);
    assert_eq!(e.get_field(53..55), 3);
}

#[test]
#[should_panic(expected = "internal compiler error")]
fn op_atom_local_ice() {
    let mut e = encoder_sm35();
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
fn op_atom_shared_ice() {
    let mut e = encoder_sm35();
    OpAtom {
        dst: Dst::None,
        srcs: [gpr_src(0), Src::ZERO, gpr_src(1)],
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
