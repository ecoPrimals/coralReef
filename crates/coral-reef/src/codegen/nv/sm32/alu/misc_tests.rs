// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    CBuf, CBufRef, Dst, OpMov, OpPSetP, OpPrmt, OpSel, OpShfl, PredSetOp, PrmtMode, RegFile,
    RegRef, ShflOp, Src, SrcMod, SrcRef, SrcSwizzle,
};
use crate::codegen::nv::sm32::encoder::{SM32Encoder, SM32Op, ShaderModel32};

fn gpr(idx: u32) -> Src {
    Src {
        reference: SrcRef::Reg(RegRef::new(RegFile::GPR, idx, 1)),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn pred(idx: u32) -> Src {
    Src {
        reference: SrcRef::Reg(RegRef::new(RegFile::Pred, idx, 1)),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn encoder_sm32() -> SM32Encoder<'static> {
    let sm: &'static ShaderModel32 = Box::leak(Box::new(ShaderModel32::new(32)));
    let labels: &'static FxHashMap<crate::codegen::ir::Label, usize> =
        Box::leak(Box::new(FxHashMap::default()));
    SM32Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 2],
    }
}

fn op_hi(e: &SM32Encoder<'_>) -> u64 {
    e.get_field(52..64)
}

#[test]
fn op_mov_src_forms() {
    let mut e = encoder_sm32();
    OpMov {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: gpr(2),
        quad_lanes: 0xf,
    }
    .encode(&mut e);
    assert_eq!(op_hi(&e), 0xe4c);

    let mut e = encoder_sm32();
    OpMov {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: Src::new_imm_u32(0x4040_0000),
        quad_lanes: 0xf,
    }
    .encode(&mut e);
    assert_eq!(op_hi(&e), 0x742);

    let mut e = encoder_sm32();
    OpMov {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: Src {
            reference: SrcRef::CBuf(CBufRef {
                buf: CBuf::Binding(1),
                offset: 0x10,
            }),
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        },
        quad_lanes: 0xf,
    }
    .encode(&mut e);
    assert_eq!(op_hi(&e), 0x64c);
}

#[test]
fn op_prmt_modes_encode_distinct_fields() {
    for (mode, expected) in [
        (PrmtMode::Index, 0_u64),
        (PrmtMode::Forward4Extract, 1),
        (PrmtMode::Backward4Extract, 2),
        (PrmtMode::Replicate8, 3),
        (PrmtMode::EdgeClampLeft, 4),
        (PrmtMode::EdgeClampRight, 5),
        (PrmtMode::Replicate16, 6),
    ] {
        let mut e = encoder_sm32();
        OpPrmt {
            dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, 1)),
            srcs: [gpr(1), gpr(2), Src::new_imm_u32(0x3210)],
            mode,
        }
        .encode(&mut e);
        assert_eq!(e.get_field(51..54), expected, "prmt mode field");
    }
}

#[test]
fn op_sel_encoding() {
    let mut e = encoder_sm32();
    OpSel {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 3, 1)),
        srcs: [pred(1), gpr(4), gpr(5)],
    }
    .encode(&mut e);
    assert_eq!(op_hi(&e), 0xe50);
}

#[test]
fn op_shfl_ops() {
    for (shfl, enc) in [
        (ShflOp::Idx, 0_u64),
        (ShflOp::Up, 1),
        (ShflOp::Down, 2),
        (ShflOp::Bfly, 3),
    ] {
        let mut e = encoder_sm32();
        OpShfl {
            dsts: [
                Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
                Dst::Reg(RegRef::new(RegFile::Pred, 2, 1)),
            ],
            srcs: [gpr(3), gpr(4), gpr(5)],
            op: shfl,
        }
        .encode(&mut e);
        assert_eq!(e.get_field(33..35), enc, "shfl op field");
    }
}

#[test]
fn op_psetp_pred_ops() {
    let mut e = encoder_sm32();
    OpPSetP {
        dsts: [
            Dst::Reg(RegRef::new(RegFile::Pred, 0, 1)),
            Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
        ],
        ops: [PredSetOp::And, PredSetOp::Or],
        srcs: [pred(2), pred(3), pred(4)],
    }
    .encode(&mut e);
    assert_eq!(op_hi(&e), 0x848);
    assert_eq!(e.get_field(27..29), 0_u64);
    assert_eq!(e.get_field(48..50), 1_u64);
}
