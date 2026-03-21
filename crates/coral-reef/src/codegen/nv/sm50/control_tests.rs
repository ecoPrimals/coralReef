// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
//! Unit tests for SM50 control-flow and system encoders (`control.rs`).

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    Dst, Label, LabelAllocator, OpBar, OpBra, OpCS2R, OpExit, OpNop, OpOut, OpS2R, OpVote, OutType,
    RegFile, RegRef, Src, SrcMod, SrcSwizzle, VoteOp,
};

use super::encoder::{SM50Encoder, SM50Op, ShaderModel50};

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn pred_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::Pred, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn encoder_sm50(ip: usize, labels: &'static FxHashMap<Label, usize>) -> SM50Encoder<'static> {
    let sm: &'static ShaderModel50 = Box::leak(Box::new(ShaderModel50::new(50)));
    SM50Encoder {
        sm,
        ip,
        labels,
        inst: [0_u32; 2],
        sched: 0,
    }
}

fn sm50_opcode(e: &SM50Encoder<'_>) -> u64 {
    e.get_field(48..64)
}

#[test]
fn op_bra_rel_offset_encoding() {
    let mut map = FxHashMap::default();
    let mut alloc = LabelAllocator::new();
    let target = alloc.alloc();
    map.insert(target, 0x40);
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(map));
    let mut e = encoder_sm50(0, labels);
    OpBra {
        target,
        cond: Src::new_imm_bool(true),
    }
    .encode(&mut e);
    assert_eq!(sm50_opcode(&e), 0xe240);
    let rel: i64 = e.get_field(20..44) as i64;
    assert_eq!(rel, 0x38, "target 0x40 - ip 0 - 8");
}

#[test]
fn op_exit_encoding() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder_sm50(0, labels);
    OpExit {}.encode(&mut e);
    assert_eq!(sm50_opcode(&e), 0xe300);
    assert_eq!(e.get_field(0..4), 0xF_u64, "CC.T");
}

#[test]
fn op_bar_sync_and_reduction_fields() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder_sm50(0, labels);
    OpBar {}.encode(&mut e);
    assert_eq!(sm50_opcode(&e), 0xf0a8);
    assert_eq!(e.get_field(32..35), 0_u64, "SYNC mode");
    assert_eq!(e.get_field(35..37), 0_u64, "RED.POPC");
}

#[test]
fn op_cs2r_encoding() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder_sm50(0, labels);
    OpCS2R {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 4, 1)),
        idx: 0x2a,
    }
    .encode(&mut e);
    assert_eq!(sm50_opcode(&e), 0x50c8);
    assert_eq!(e.get_field(20..28), 0x2a_u64);
}

#[test]
fn op_s2r_encoding() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder_sm50(0, labels);
    OpS2R {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 1)),
        idx: 0x17,
    }
    .encode(&mut e);
    assert_eq!(sm50_opcode(&e), 0xf0c8);
    assert_eq!(e.get_field(20..28), 0x17_u64);
}

#[test]
fn op_vote_all_any_eq() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder_sm50(0, labels);
    OpVote {
        op: VoteOp::All,
        dsts: [
            Dst::Reg(RegRef::new(RegFile::GPR, 9, 1)),
            Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
        ],
        pred: pred_src(3),
    }
    .encode(&mut e);
    assert_eq!(sm50_opcode(&e), 0x50d8);
    assert_eq!(e.get_field(48..50), 0_u64, "vote.all");

    let mut e = encoder_sm50(0, labels);
    OpVote {
        op: VoteOp::Any,
        dsts: [
            Dst::Reg(RegRef::new(RegFile::GPR, 9, 1)),
            Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
        ],
        pred: Src::new_imm_bool(true),
    }
    .encode(&mut e);
    assert_eq!(e.get_field(48..50), 1_u64, "vote.any");

    let mut e = encoder_sm50(0, labels);
    OpVote {
        op: VoteOp::Eq,
        dsts: [
            Dst::Reg(RegRef::new(RegFile::GPR, 9, 1)),
            Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
        ],
        pred: pred_src(2),
    }
    .encode(&mut e);
    assert_eq!(e.get_field(48..50), 2_u64, "vote.eq");
}

#[test]
fn op_nop_encoding() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder_sm50(0, labels);
    OpNop { label: None }.encode(&mut e);
    assert_eq!(sm50_opcode(&e), 0x50b0);
    assert_eq!(e.get_field(8..12), 0xF_u64, "CC.T");
}

#[test]
fn op_out_stream_variants() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder_sm50(0, labels);
    OpOut {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 5, 1)),
        srcs: [gpr_src(1), gpr_src(2)],
        out_type: OutType::Emit,
    }
    .encode(&mut e);
    assert_eq!(sm50_opcode(&e), 0xfbe0);
    assert_eq!(e.get_field(39..41), 1_u64, "emit");

    let mut e = encoder_sm50(0, labels);
    OpOut {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 5, 1)),
        srcs: [gpr_src(1), Src::new_imm_u32(4)],
        out_type: OutType::Cut,
    }
    .encode(&mut e);
    assert_eq!(sm50_opcode(&e), 0xf6e0);
    assert_eq!(e.get_field(39..41), 2_u64, "cut");

    let mut e = encoder_sm50(0, labels);
    OpOut {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 5, 1)),
        srcs: [gpr_src(1), gpr_src(2)],
        out_type: OutType::EmitThenCut,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(39..41), 3_u64, "emit_then_cut");
}
