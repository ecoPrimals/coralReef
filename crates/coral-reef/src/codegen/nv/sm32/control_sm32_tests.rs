// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
//! Unit tests for SM32 control-flow encoders (`control.rs`).

use super::super::encoder::*;
use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    CCtlOp, Dst, LabelAllocator, MemAddrType, MemScope, MemSpace, OpBar, OpBra, OpBrk, OpCCtl,
    OpCont, OpExit, OpKill, OpMemBar, OpNop, OpOut, OpPBk, OpPCnt, OpPixLd, OpS2R, OpSSy, OpSync,
    OpTexDepBar, OpViLd, OpVote, OutType, PixVal, RegFile, RegRef, Src, SrcMod, SrcSwizzle, VoteOp,
};

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn fresh_encoder() -> SM32Encoder<'static> {
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

fn encoder_with_labels(
    labels: &'static FxHashMap<crate::codegen::ir::Label, usize>,
) -> SM32Encoder<'static> {
    let sm: &'static ShaderModel32 = Box::leak(Box::new(ShaderModel32::new(35)));
    SM32Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 2],
    }
}

fn sm32_opcode(e: &SM32Encoder<'_>) -> u64 {
    e.get_field(52..64)
}

fn sm32_fu(e: &SM32Encoder<'_>) -> u64 {
    e.get_field(0..2)
}

#[test]
fn op_bra_rel_offset_and_unit() {
    let mut map = FxHashMap::default();
    let mut alloc = LabelAllocator::new();
    let target = alloc.alloc();
    map.insert(target, 0x40);
    let labels: &'static FxHashMap<_, _> = Box::leak(Box::new(map));
    let mut e = encoder_with_labels(labels);
    OpBra {
        target,
        cond: Src::new_imm_bool(true),
    }
    .encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x120);
    assert_eq!(sm32_fu(&e), 0);
    assert_eq!(e.get_field(2..6), 0xf);
    let rel: i64 = e.get_field(23..47) as i32 as i64;
    assert_eq!(rel, 0x38);
}

#[test]
fn op_ssy_and_pbk_encode_relative_target() {
    let mut map = FxHashMap::default();
    let mut alloc = LabelAllocator::new();
    let t1 = alloc.alloc();
    let t2 = alloc.alloc();
    map.insert(t1, 0x100);
    map.insert(t2, 0x200);
    let labels: &'static FxHashMap<_, _> = Box::leak(Box::new(map));

    let mut e = encoder_with_labels(labels);
    OpSSy { target: t1 }.encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x148);
    assert_eq!(e.get_field(2..8), 0xf);

    let mut e = encoder_with_labels(labels);
    OpPBk { target: t2 }.encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x150);
}

#[test]
fn op_brk_cont_exit_pcnt_flags() {
    let mut alloc = LabelAllocator::new();
    let mut e = fresh_encoder();
    OpBrk {
        target: alloc.alloc(),
    }
    .encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x1a0);
    assert_eq!(e.get_field(2..8), 0xf);

    let mut e = fresh_encoder();
    OpCont {
        target: alloc.alloc(),
    }
    .encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x1a8);

    let mut e = fresh_encoder();
    OpExit {}.encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x180);

    let mut map = FxHashMap::default();
    let target = alloc.alloc();
    map.insert(target, 0x80);
    let labels: &'static FxHashMap<_, _> = Box::leak(Box::new(map));
    let mut e = encoder_with_labels(labels);
    OpPCnt { target }.encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x158);
}

#[test]
fn op_bar_sync_fields() {
    let mut e = fresh_encoder();
    OpBar {}.encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x854);
    assert_eq!(sm32_fu(&e), 2);
    assert_eq!(e.get_field(35..38), 0);
    assert_eq!(e.get_field(38..40), 0);
}

#[test]
fn op_vote_ballot_and_mode() {
    let mut e = fresh_encoder();
    OpVote {
        op: VoteOp::All,
        dsts: [
            Dst::Reg(RegRef::new(RegFile::GPR, 6, 1)),
            Dst::Reg(RegRef::new(RegFile::Pred, 2, 1)),
        ],
        pred: Src::new_imm_bool(true),
    }
    .encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x86c);
    assert_eq!(e.get_field(51..53), 0);
}

#[test]
fn op_kill_opcode() {
    let mut e = fresh_encoder();
    OpKill {}.encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x198);
}

#[test]
fn op_membar_scopes() {
    for (scope, code) in [
        (MemScope::CTA, 0_u8),
        (MemScope::GPU, 1_u8),
        (MemScope::System, 2_u8),
    ] {
        let mut e = fresh_encoder();
        OpMemBar { scope }.encode(&mut e);
        assert_eq!(sm32_opcode(&e), 0x7cc);
        assert_eq!(e.get_field(10..12), u64::from(code));
    }
}

#[test]
fn op_cctl_global_a32_and_shared() {
    let mut e = fresh_encoder();
    OpCCtl {
        op: CCtlOp::WB,
        mem_space: MemSpace::Global(MemAddrType::A32),
        addr: gpr_src(3),
        addr_offset: 0x20,
    }
    .encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x7b0);
    assert_eq!(e.get_field(25..55), 8);
    assert_eq!(e.get_field(55..56), 0);

    let mut e = fresh_encoder();
    OpCCtl {
        op: CCtlOp::WB,
        mem_space: MemSpace::Shared,
        addr: gpr_src(3),
        addr_offset: 0x40,
    }
    .encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x7c0);
    assert_eq!(e.get_field(25..47), 16);
}

#[test]
fn op_sync_n_sets_sync_bit() {
    let mut e = fresh_encoder();
    OpSync {
        target: LabelAllocator::new().alloc(),
    }
    .encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x858);
    assert!(e.get_bit(22));
}

#[test]
fn op_nop_unit() {
    let mut e = fresh_encoder();
    OpNop { label: None }.encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x858);
    assert_eq!(e.get_field(10..14), 0xf);
}

#[test]
fn op_texdepbar_queue_depth() {
    let mut e = fresh_encoder();
    OpTexDepBar {
        textures_left: 0x2a,
    }
    .encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x770);
    assert_eq!(e.get_field(23..29), 0x2a);
}

#[test]
fn op_s2r_and_vild() {
    let mut e = fresh_encoder();
    OpS2R {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 5, 1)),
        idx: 0x17,
    }
    .encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x864);
    assert_eq!(e.get_field(23..31), 0x17);

    let mut e = fresh_encoder();
    OpViLd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 5, 1)),
        idx: gpr_src(1),
        off: 0x0c,
    }
    .encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x7f8);
}

#[test]
fn op_pix_ld_cov_mask() {
    let mut e = fresh_encoder();
    OpPixLd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 1)),
        val: PixVal::CovMask,
    }
    .encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0x7f4);
    assert_eq!(e.get_field(34..37), 1);
}

#[test]
fn op_out_emit_rrr_form() {
    let mut e = fresh_encoder();
    OpOut {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 8, 1)),
        srcs: [gpr_src(1), gpr_src(2)],
        out_type: OutType::Emit,
    }
    .encode(&mut e);
    assert_eq!(sm32_opcode(&e), 0xdf0);
    assert_eq!(e.get_field(42..44), 1);
}
