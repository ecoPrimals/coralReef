// SPDX-License-Identifier: AGPL-3.0-or-later

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    Dst, Label, OpMov, OpPSetP, OpPrmt, OpSel, OpShfl, PredSetOp, PrmtMode, RegFile, RegRef,
    ShflOp, Src, SrcMod, SrcSwizzle,
};

use super::super::super::encoder::{SM20Encoder, SM20Op, SM20Unit, ShaderModel20};

fn gpr(idx: u32) -> RegRef {
    RegRef::new(RegFile::GPR, idx, 1)
}

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: gpr(idx).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn gpr_dst(idx: u32) -> Dst {
    Dst::Reg(gpr(idx))
}

fn pred_dst(idx: u32) -> Dst {
    Dst::Reg(RegRef::new(RegFile::Pred, idx, 1))
}

fn pred_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::Pred, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn sm20_encoder() -> SM20Encoder<'static> {
    let sm: &'static ShaderModel20 = Box::leak(Box::new(ShaderModel20::new(20)));
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    SM20Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 2],
    }
}

fn unit(e: &SM20Encoder<'_>) -> u64 {
    e.get_field(0..3)
}

fn opcode_byte(e: &SM20Encoder<'_>) -> u64 {
    e.get_field(58..64)
}

#[test]
fn op_mov_reg_move_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpMov {
        dst: gpr_dst(1),
        src: gpr_src(2),
        quad_lanes: 0xf,
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Move as u64);
    assert_eq!(opcode_byte(&e), 0xa);
}

#[test]
fn op_mov_quad_lanes_field() {
    let mut e = sm20_encoder();
    OpMov {
        dst: gpr_dst(1),
        src: gpr_src(2),
        quad_lanes: 0b1010,
    }
    .encode(&mut e);
    let lanes: u64 = e.get_field(5..9);
    assert_eq!(lanes, 0b1010, "quad_lanes should be encoded in bits 5..9");
}

#[test]
fn op_prmt_move_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpPrmt {
        dst: gpr_dst(1),
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        mode: PrmtMode::Index,
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Move as u64);
    assert_eq!(opcode_byte(&e), 0x9);
}

#[test]
fn op_prmt_mode_field() {
    let mut e = sm20_encoder();
    OpPrmt {
        dst: gpr_dst(1),
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        mode: PrmtMode::Forward4Extract,
    }
    .encode(&mut e);
    let mode: u64 = e.get_field(5..8);
    assert_eq!(mode, 1, "Forward4Extract should be 1");
}

#[test]
fn op_sel_move_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpSel {
        dst: gpr_dst(1),
        srcs: [true.into(), gpr_src(2), gpr_src(3)],
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Move as u64);
    assert_eq!(opcode_byte(&e), 0x8);
}

#[test]
fn op_shfl_mem_unit() {
    let mut e = sm20_encoder();
    let op = OpShfl {
        dsts: [gpr_dst(1), Dst::None],
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        op: ShflOp::Idx,
    };
    op.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Mem as u64);
}

#[test]
fn op_shfl_op_field() {
    let mut e = sm20_encoder();
    let op = OpShfl {
        dsts: [gpr_dst(1), Dst::None],
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        op: ShflOp::Bfly,
    };
    op.encode(&mut e);
    let shfl_op: u64 = e.get_field(55..57);
    assert_eq!(shfl_op, 3, "Bfly should be 3");
}

#[test]
fn op_psetp_move_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpPSetP {
        dsts: [pred_dst(0), Dst::None],
        srcs: [pred_src(1), pred_src(2), true.into()],
        ops: [PredSetOp::And, PredSetOp::And],
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Move as u64);
    assert_eq!(opcode_byte(&e), 0x3);
}

#[test]
fn op_psetp_or_set_op() {
    let mut e = sm20_encoder();
    OpPSetP {
        dsts: [pred_dst(0), Dst::None],
        srcs: [pred_src(1), pred_src(2), true.into()],
        ops: [PredSetOp::Or, PredSetOp::And],
    }
    .encode(&mut e);
    let op0: u64 = e.get_field(30..32);
    assert_eq!(op0, 1, "PredSetOp::Or should be 1");
}
