// SPDX-License-Identifier: AGPL-3.0-or-later

//! Encoder tests for SM70 FP64 ALU ops (`alu/float64.rs`).

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    Dst, FRndMode, FloatCmpOp, Label, OpDAdd, OpDFma, OpDMul, OpDSetP, PredSetOp, RegFile, RegRef,
    Src, SrcMod, SrcSwizzle,
};

use super::super::encoder::{SM70Encoder, SM70Op};

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn gpr_dst(idx: u32) -> Dst {
    RegRef::new(RegFile::GPR, idx, 1).into()
}

fn pred_dst(idx: u32) -> Dst {
    RegRef::new(RegFile::Pred, idx, 1).into()
}

fn encoder() -> SM70Encoder<'static> {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    SM70Encoder {
        sm: 70,
        ip: 0,
        labels,
        inst: [0_u32; 4],
    }
}

fn opcode(e: &SM70Encoder<'_>) -> u64 {
    e.get_field(0..9)
}

#[test]
fn dadd_opcode() {
    let op = OpDAdd {
        dst: gpr_dst(0),
        srcs: [gpr_src(2), gpr_src(4)],
        rnd_mode: FRndMode::NearestEven,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x029);
}

#[test]
fn dfma_opcode() {
    let op = OpDFma {
        dst: gpr_dst(0),
        srcs: [gpr_src(2), gpr_src(4), gpr_src(6)],
        rnd_mode: FRndMode::NearestEven,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x02b);
}

#[test]
fn dmul_opcode() {
    let op = OpDMul {
        dst: gpr_dst(0),
        srcs: [gpr_src(2), gpr_src(4)],
        rnd_mode: FRndMode::NearestEven,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x028);
}

#[test]
fn dsetp_opcode() {
    let op = OpDSetP {
        dst: pred_dst(0),
        srcs: [gpr_src(2), gpr_src(4), Src::new_imm_bool(true)],
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdLt,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x02a);
}

#[test]
fn dsetp_pred_dst_written() {
    let op = OpDSetP {
        dst: pred_dst(3),
        srcs: [gpr_src(2), gpr_src(4), Src::new_imm_bool(true)],
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdEq,
    };
    let mut e = encoder();
    op.encode(&mut e);
    let pred: u64 = e.get_field(81..84);
    assert_eq!(pred, 3);
}

#[test]
fn dsetp_dst1_cleared() {
    let op = OpDSetP {
        dst: pred_dst(0),
        srcs: [gpr_src(2), gpr_src(4), Src::new_imm_bool(true)],
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdLt,
    };
    let mut e = encoder();
    op.encode(&mut e);
    let dst1: u64 = e.get_field(84..87);
    assert_eq!(dst1, 0x7, "dst1 should be Dst::None (0x7)");
}
