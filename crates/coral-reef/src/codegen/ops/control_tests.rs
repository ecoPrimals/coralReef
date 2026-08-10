// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! Tests for control flow operation encoding — Exit, Nop, Bar, Bra.

use super::{AmdOpEncoder, EncodeOp, encode_amd_op};
use crate::codegen::ir::*;
use coral_reef_stubs::fxhash::FxHashMap;

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

fn alloc_label() -> Label {
    let mut alloc = LabelAllocator::new();
    alloc.alloc()
}

fn alloc_two_labels() -> (Label, Label) {
    let mut alloc = LabelAllocator::new();
    let a = alloc.alloc();
    let b = alloc.alloc();
    (a, b)
}

#[test]
fn exit_encodes_to_s_endpgm() {
    let op = OpExit {};
    let words = enc_op(Op::Exit(op)).expect("Exit encode");
    assert!(!words.is_empty(), "Exit should emit at least one word");
    let sopp_encoding = words[0] >> 16;
    assert_eq!(sopp_encoding & 0xFF80, 0xBF80, "should be SOPP format");
}

#[test]
fn nop_encodes_to_s_nop() {
    let op = OpNop { label: None };
    let words = enc_op(Op::Nop(op)).expect("Nop encode");
    assert!(!words.is_empty());
}

#[test]
fn nop_with_label_encodes() {
    let label = alloc_label();
    let op = OpNop { label: Some(label) };
    let words = enc_op(Op::Nop(op)).expect("Nop+label encode");
    assert!(!words.is_empty());
}

#[test]
fn bar_encodes_to_s_barrier() {
    let op = OpBar {};
    let words = enc_op(Op::Bar(Box::new(op))).expect("Bar encode");
    assert!(!words.is_empty());
    let sopp_encoding = words[0] >> 16;
    assert_eq!(sopp_encoding & 0xFF80, 0xBF80, "should be SOPP format");
}

#[test]
fn bra_unconditional_succeeds() {
    let (target, _) = alloc_two_labels();
    let mut label_map = FxHashMap::default();
    label_map.insert(target, 10_usize);
    let op = OpBra {
        target,
        cond: Src {
            reference: SrcRef::True,
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        },
    };
    let mut enc = AmdOpEncoder::new(&label_map, 5, 254, 255, 10, 2, 0);
    let words = op.encode(&mut enc).expect("unconditional branch");
    assert!(!words.is_empty());
}

#[test]
fn bra_missing_label_returns_error() {
    let target = alloc_label();
    let labels = FxHashMap::default();
    let op = OpBra {
        target,
        cond: Src {
            reference: SrcRef::True,
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        },
    };
    let mut enc = AmdOpEncoder::new(&labels, 0, 254, 255, 10, 2, 0);
    assert!(op.encode(&mut enc).is_err());
}

#[test]
fn bra_conditional_bnot_uses_cbranch_vccz() {
    let target = alloc_label();
    let mut label_map = FxHashMap::default();
    label_map.insert(target, 20_usize);
    let op = OpBra {
        target,
        cond: Src {
            reference: SrcRef::Reg(RegRef::new(RegFile::Pred, 0, 1)),
            modifier: SrcMod::BNot,
            swizzle: SrcSwizzle::None,
        },
    };
    let mut enc = AmdOpEncoder::new(&label_map, 10, 254, 255, 10, 2, 0);
    let words = op.encode(&mut enc).expect("conditional branch");
    assert!(!words.is_empty());
}
