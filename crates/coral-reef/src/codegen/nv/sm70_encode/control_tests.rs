// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
//! Unit tests for SM70 control-flow and system encoders (`control.rs`).

use super::super::encoder::{SM70Encoder, SM70Op};
use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    Dst, Label, LabelAllocator, MatchOp, MemScope, OpBClear, OpBMov, OpBSSy, OpBSync, OpBar, OpBra,
    OpBreak, OpExit, OpKill, OpMatch, OpMemBar, OpNop, OpOut, OpOutFinal, OpPixLd, OpS2R, OpVote,
    OpWarpSync, OutType, PixVal, Pred, PredRef, RegFile, RegRef, Src, SrcMod, SrcSwizzle, VoteOp,
};

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn pred_reg(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::Pred, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn upred_reg(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::UPred, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn encoder(sm: u8, ip: usize, labels: &'static FxHashMap<Label, usize>) -> SM70Encoder<'static> {
    SM70Encoder {
        sm,
        ip,
        labels,
        inst: [0_u32; 4],
    }
}

fn opcode(e: &SM70Encoder<'_>) -> u64 {
    e.get_field(0..12)
}

#[test]
fn op_exit_opcode_and_always_pred() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder(70, 0, labels);
    OpExit {}.encode(&mut e);
    assert_eq!(opcode(&e), 0x94d);
    assert_eq!(e.get_field(87..90), 7, "always-true predicate");
    assert!(!e.get_bit(84));
    assert!(!e.get_bit(85));
}

#[test]
fn op_kill_opcode() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder(70, 0, labels);
    OpKill {}.encode(&mut e);
    assert_eq!(opcode(&e), 0x95b);
}

#[test]
fn op_bar_sync_opcode_and_mode() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder(70, 0, labels);
    OpBar {}.encode(&mut e);
    assert_eq!(opcode(&e), 0x31d);
    assert_eq!(e.get_field(77..79), 0, "SYNC");
    assert_eq!(e.get_field(74..76), 0, "reduction op");
}

#[test]
fn op_nop_opcode() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder(70, 0, labels);
    OpNop { label: None }.encode(&mut e);
    assert_eq!(opcode(&e), 0x918);
}

#[test]
fn op_warpsync_mask_encodes_as_alu_src() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder(70, 0, labels);
    OpWarpSync { mask: 0xabcd_dcba }.encode(&mut e);
    assert_eq!(e.get_field(0..9), 0x148);
    assert_eq!(e.get_field(32..64), 0xabcd_dcba);
}

#[test]
fn op_break_barrier_and_pred_src() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let bar = RegRef::new(RegFile::Bar, 3, 1);
    let mut e = encoder(70, 0, labels);
    OpBreak {
        bar_out: Dst::Reg(bar),
        srcs: [
            Src {
                reference: bar.into(),
                modifier: SrcMod::None,
                swizzle: SrcSwizzle::None,
            },
            pred_reg(2),
        ],
    }
    .encode(&mut e);
    assert_eq!(opcode(&e), 0x942);
    assert_eq!(e.get_field(16..20), 3, "barrier dst");
    assert_eq!(e.get_field(87..90), 2, "pred reg index");
}

#[test]
fn op_bra_rel_offset_sm70_and_pred_branch_form() {
    let mut map = FxHashMap::default();
    let mut alloc = LabelAllocator::new();
    let target = alloc.alloc();
    map.insert(target, 0x40);
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(map));
    let mut e = encoder(70, 0, labels);
    OpBra {
        target,
        cond: Src::new_imm_bool(true),
    }
    .encode(&mut e);
    assert_eq!(opcode(&e), 0x947);
    assert!(!e.get_bit(32), "not .U");
    let rel: i64 = e.get_field(34..82) as i64;
    assert_eq!(rel, 0x3c, "target 0x40 - ip 0 - 4");
}

#[test]
fn op_bra_upred_sm80_uses_uniform_branch_opcode() {
    let mut map = FxHashMap::default();
    let mut alloc = LabelAllocator::new();
    let target = alloc.alloc();
    map.insert(target, 8);
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(map));
    let mut e = encoder(80, 0, labels);
    OpBra {
        target,
        cond: upred_reg(4),
    }
    .encode(&mut e);
    assert_eq!(opcode(&e), 0x547);
    assert!(e.get_bit(32), ".U");
    assert_eq!(e.get_field(24..27), 4);
}

#[test]
fn op_bra_sm100_splits_rel_offset() {
    let mut map = FxHashMap::default();
    let mut alloc = LabelAllocator::new();
    let target = alloc.alloc();
    map.insert(target, 0x100);
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(map));
    let mut e = encoder(100, 0, labels);
    OpBra {
        target,
        cond: Src::new_imm_bool(true),
    }
    .encode(&mut e);
    assert_eq!(opcode(&e), 0x947);
    assert_eq!(e.get_field(16..24), 0xfc);
    assert_eq!(e.get_field(34..82), 0);
}

#[test]
fn op_bssy_rel_offset_and_barrier() {
    let mut map = FxHashMap::default();
    let mut alloc = LabelAllocator::new();
    let target = alloc.alloc();
    map.insert(target, 0x80);
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(map));
    let bar = RegRef::new(RegFile::Bar, 1, 1);
    let mut e = encoder(70, 0, labels);
    OpBSSy {
        bar_out: Dst::Reg(bar),
        srcs: [
            Src {
                reference: bar.into(),
                modifier: SrcMod::None,
                swizzle: SrcSwizzle::None,
            },
            Src::new_imm_bool(true),
        ],
        target,
    }
    .encode(&mut e);
    assert_eq!(opcode(&e), 0x945);
    assert_eq!(e.get_field(16..20), 1);
    let rel: i64 = e.get_field(34..64) as i64;
    assert_eq!(rel, 0x7c);
}

#[test]
fn op_bsync_barrier_and_cond() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let bar = RegRef::new(RegFile::Bar, 2, 1);
    let mut e = encoder(70, 0, labels);
    OpBSync {
        srcs: [
            Src {
                reference: bar.into(),
                modifier: SrcMod::None,
                swizzle: SrcSwizzle::None,
            },
            pred_reg(5),
        ],
    }
    .encode(&mut e);
    assert_eq!(opcode(&e), 0x941);
    assert_eq!(e.get_field(16..20), 2);
    assert_eq!(e.get_field(87..90), 5);
}

#[test]
fn op_vote_warp_and_uniform_ballot() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder(70, 0, labels);
    OpVote {
        op: VoteOp::Eq,
        dsts: [
            Dst::Reg(RegRef::new(RegFile::GPR, 9, 1)),
            Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
        ],
        pred: pred_reg(3),
    }
    .encode(&mut e);
    assert_eq!(opcode(&e), 0x806);
    assert_eq!(e.get_field(72..74), 2, "vote.eq");
    assert_eq!(e.get_field(81..84), 1);

    let mut e = encoder(73, 0, labels);
    OpVote {
        op: VoteOp::Any,
        dsts: [Dst::Reg(RegRef::new(RegFile::UGPR, 11, 1)), Dst::None],
        pred: Src::new_imm_bool(true),
    }
    .encode(&mut e);
    assert_eq!(opcode(&e), 0x886);
    assert_eq!(e.get_field(72..74), 1, "vote.any");
}

#[test]
fn op_match_any_sets_pred_bit() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder(70, 0, labels);
    OpMatch {
        dsts: [Dst::None, Dst::Reg(RegRef::new(RegFile::GPR, 4, 1))],
        src: gpr_src(6),
        op: MatchOp::Any,
        u64: true,
    }
    .encode(&mut e);
    assert_eq!(opcode(&e), 0x3a1);
    assert!(e.get_bit(79), "MatchOp::Any");
    assert!(e.get_bit(73), ".u64");
}

#[test]
fn op_bclear_sets_clear_bit() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder(70, 0, labels);
    OpBClear {
        dst: Dst::Reg(RegRef::new(RegFile::Bar, 0, 1)),
    }
    .encode(&mut e);
    assert_eq!(opcode(&e), 0x355);
    assert!(e.get_bit(84), ".CLEAR");
}

#[test]
fn op_bmov_to_bar_opcode() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let bar = RegRef::new(RegFile::Bar, 2, 1);
    let mut e = encoder(70, 0, labels);
    OpBMov {
        dst: Dst::Reg(bar),
        src: gpr_src(7),
        clear: false,
    }
    .encode(&mut e);
    assert_eq!(opcode(&e), 0x356);
    assert_eq!(e.get_field(24..28), 2);
    assert_eq!(e.get_field(32..40), 7);
}

#[test]
fn instruction_pred_applies_after_encode() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder(70, 0, labels);
    OpNop { label: None }.encode(&mut e);
    e.set_pred(&Pred {
        predicate: PredRef::Reg(RegRef::new(RegFile::Pred, 3, 1)),
        inverted: false,
    });
    assert_eq!(e.get_field(12..15), 3);
    assert!(!e.get_bit(15));
}

#[test]
fn op_pix_ld_ms_count_encoding() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder(70, 0, labels);
    OpPixLd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        val: PixVal::MsCount,
    }
    .encode(&mut e);
    assert_eq!(opcode(&e), 0x925);
    assert_eq!(e.get_field(78..81), 0);
}

#[test]
fn op_pix_ld_all_encodable_variants() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    for (val, enc) in [
        (PixVal::MsCount, 0_u64),
        (PixVal::CovMask, 1),
        (PixVal::CentroidOffset, 2),
        (PixVal::MyIndex, 3),
        (PixVal::InnerCoverage, 4),
    ] {
        let mut e = encoder(70, 0, labels);
        OpPixLd {
            dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 1)),
            val,
        }
        .encode(&mut e);
        assert_eq!(e.get_field(78..81), enc);
    }
}

#[test]
fn op_out_types_and_streams() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    for (out_ty, enc) in [
        (OutType::Emit, 1_u64),
        (OutType::Cut, 2),
        (OutType::EmitThenCut, 3),
    ] {
        let mut e = encoder(70, 0, labels);
        OpOut {
            dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
            srcs: [gpr_src(4), gpr_src(5)],
            out_type: out_ty,
        }
        .encode(&mut e);
        assert_eq!(e.get_field(78..80), enc);
        assert_eq!(e.get_field(0..9), 0x124);
    }

    let mut e = encoder(70, 0, labels);
    OpOut {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(9), Src::new_imm_u32(3)],
        out_type: OutType::Emit,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(78..80), 1);
}

#[test]
fn op_out_final_encodes() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder(70, 0, labels);
    OpOutFinal { handle: gpr_src(6) }.encode(&mut e);
    assert_eq!(e.get_field(0..9), 0x124);
}

#[test]
fn op_membar_scopes_sm70() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    for (scope, enc) in [
        (MemScope::CTA, 0_u64),
        (MemScope::GPU, 2),
        (MemScope::System, 3),
    ] {
        let mut e = encoder(70, 0, labels);
        OpMemBar { scope }.encode(&mut e);
        assert_eq!(e.get_field(76..79), enc);
        assert_eq!(opcode(&e), 0x992);
    }
}

#[test]
fn op_s2r_uniform_uses_udst_opcode() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder(73, 0, labels);
    OpS2R {
        dst: Dst::Reg(RegRef::new(RegFile::UGPR, 5, 1)),
        idx: 0x2a,
    }
    .encode(&mut e);
    assert_eq!(opcode(&e), 0x9c3);
    assert_eq!(e.get_field(72..80), 0x2a);
}

#[test]
fn op_s2r_warp_uses_vdst_opcode() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder(70, 0, labels);
    OpS2R {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 12, 1)),
        idx: 0x11,
    }
    .encode(&mut e);
    assert_eq!(opcode(&e), 0x919);
    assert_eq!(e.get_field(72..80), 0x11);
}

#[test]
fn op_bmov_to_gpr_uses_bar_src_encoding_path() {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    let mut e = encoder(70, 0, labels);
    let bar_in = RegRef::new(RegFile::Bar, 4, 1);
    OpBMov {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 3, 1)),
        src: Src {
            reference: bar_in.into(),
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        },
        clear: true,
    }
    .encode(&mut e);
    assert_eq!(opcode(&e), 0x355);
    assert_eq!(e.get_field(24..28), 4, "bar src slot carries barrier id");
    assert!(e.get_bit(84), ".CLEAR");
}
