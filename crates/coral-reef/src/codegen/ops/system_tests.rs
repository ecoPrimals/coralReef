// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! Tests for system register and move operation encoding — Mov, S2R, CS2R.

use super::{AmdOpEncoder, EncodeOp, encode_amd_op};
use crate::codegen::ir::*;
use coral_reef_stubs::fxhash::FxHashMap;

fn gpr_dst(i: u32) -> Dst {
    Dst::Reg(RegRef::new(RegFile::GPR, i, 1))
}

fn gpr_src(i: u32) -> Src {
    Src {
        reference: SrcRef::Reg(RegRef::new(RegFile::GPR, i, 1)),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn pred_true() -> Pred {
    Pred {
        predicate: PredRef::None,
        inverted: false,
    }
}

fn enc_op(op: Op) -> Result<Vec<u32>, crate::CompileError> {
    let labels = FxHashMap::default();
    encode_amd_op(&op, &pred_true(), &labels, 0, 254, 255, 10, 2, 0)
}

#[test]
fn mov_register_to_register() {
    let op = OpMov {
        dst: gpr_dst(0),
        src: gpr_src(1),
        quad_lanes: 0,
    };
    let words = enc_op(Op::Mov(Box::new(op))).expect("Mov encode");
    assert!(!words.is_empty());
}

#[test]
fn mov_immediate_materializes() {
    let op = OpMov {
        dst: gpr_dst(5),
        src: Src::new_imm_u32(0xDEAD_BEEF),
        quad_lanes: 0,
    };
    let words = enc_op(Op::Mov(Box::new(op))).expect("Mov imm encode");
    assert!(words.len() >= 2, "literal immediate should add extra word");
}

#[test]
fn s2r_tid_x_encodes() {
    let op = OpS2R {
        dst: gpr_dst(0),
        idx: 0x21,
    };
    let labels = FxHashMap::default();
    let mut enc = AmdOpEncoder::new(&labels, 0, 254, 255, 10, 2, 0);
    let words = op.encode(&mut enc).expect("S2R TID_X");
    assert!(!words.is_empty());
}

#[test]
fn s2r_ctaid_x_encodes() {
    let op = OpS2R {
        dst: gpr_dst(0),
        idx: 0x25,
    };
    let labels = FxHashMap::default();
    let mut enc = AmdOpEncoder::new(&labels, 0, 254, 255, 10, 2, 0);
    let words = op.encode(&mut enc).expect("S2R CTAID_X");
    assert!(!words.is_empty());
}

#[test]
fn s2r_unknown_sys_reg_returns_error() {
    let op = OpS2R {
        dst: gpr_dst(0),
        idx: 0xFE,
    };
    let labels = FxHashMap::default();
    let mut enc = AmdOpEncoder::new(&labels, 0, 254, 255, 10, 2, 0);
    assert!(op.encode(&mut enc).is_err());
}

#[test]
fn cs2r_tid_y_encodes() {
    let op = OpCS2R {
        dst: gpr_dst(0),
        idx: 0x22,
    };
    let labels = FxHashMap::default();
    let mut enc = AmdOpEncoder::new(&labels, 0, 254, 255, 10, 2, 0);
    let words = op.encode(&mut enc).expect("CS2R TID_Y");
    assert!(!words.is_empty());
}

#[test]
fn s2r_ntid_x_uses_user_sgpr_offset() {
    let op = OpS2R {
        dst: gpr_dst(0),
        idx: 0x29,
    };
    let labels = FxHashMap::default();
    let mut enc = AmdOpEncoder::new(&labels, 0, 254, 255, 10, 10, 0);
    let words = op.encode(&mut enc).expect("S2R NTID_X");
    assert!(!words.is_empty());
}

#[test]
fn s2r_laneid_encodes() {
    let op = OpS2R {
        dst: gpr_dst(0),
        idx: 0x00,
    };
    let labels = FxHashMap::default();
    let mut enc = AmdOpEncoder::new(&labels, 0, 254, 255, 10, 2, 0);
    let words = op.encode(&mut enc).expect("S2R LANEID");
    assert!(!words.is_empty());
}
