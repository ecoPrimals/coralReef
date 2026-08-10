// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! Tests for memory operation encoding — Ld, St, Copy, MemBar.

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

fn global_access(mt: MemType) -> MemAccess {
    MemAccess {
        mem_type: mt,
        space: MemSpace::Global(MemAddrType::A64),
        order: MemOrder::Strong(MemScope::GPU),
        eviction_priority: MemEvictionPriority::Normal,
    }
}

#[test]
fn ld_dword_encodes_flat_load() {
    let op = OpLd {
        dst: gpr_dst(0),
        addr: gpr_src(2),
        offset: 0,
        stride: OffsetStride::X1,
        access: global_access(MemType::B32),
    };
    let words = enc_op(Op::Ld(Box::new(op))).expect("Ld B32");
    assert!(words.len() >= 2, "FLAT load + S_WAITCNT = at least 2 words");
}

#[test]
fn ld_byte_encodes_flat_load_ubyte() {
    let op = OpLd {
        dst: gpr_dst(0),
        addr: gpr_src(2),
        offset: 0,
        stride: OffsetStride::X1,
        access: global_access(MemType::U8),
    };
    let words = enc_op(Op::Ld(Box::new(op))).expect("Ld U8");
    assert!(!words.is_empty());
}

#[test]
fn ld_dwordx2_encodes() {
    let op = OpLd {
        dst: gpr_dst(0),
        addr: gpr_src(4),
        offset: 0,
        stride: OffsetStride::X1,
        access: global_access(MemType::B64),
    };
    let words = enc_op(Op::Ld(Box::new(op))).expect("Ld B64");
    assert!(!words.is_empty());
}

#[test]
fn st_dword_encodes_flat_store() {
    let op = OpSt {
        srcs: [gpr_src(2), gpr_src(4)],
        offset: 0,
        stride: OffsetStride::X1,
        access: global_access(MemType::B32),
    };
    let words = enc_op(Op::St(Box::new(op))).expect("St B32");
    assert!(!words.is_empty());
}

#[test]
fn copy_register_is_virtual() {
    let op = OpCopy {
        dst: gpr_dst(0),
        src: gpr_src(1),
    };
    let words = enc_op(Op::Copy(Box::new(op))).expect("Copy register");
    assert!(words.is_empty(), "reg-to-reg Copy is virtual on AMD");
}

#[test]
fn copy_cbuf_materializes_to_vop1() {
    let op = OpCopy {
        dst: gpr_dst(0),
        src: Src {
            reference: SrcRef::CBuf(CBufRef {
                buf: CBuf::Binding(0),
                offset: 0,
            }),
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        },
    };
    let words = enc_op(Op::Copy(Box::new(op))).expect("Copy CBuf");
    assert!(!words.is_empty(), "CBuf Copy should emit V_MOV_B32");
}

#[test]
fn membar_cta_scope() {
    let op = OpMemBar {
        scope: MemScope::CTA,
    };
    let labels = FxHashMap::default();
    let mut enc = AmdOpEncoder::new(&labels, 0, 254, 255, 10, 2, 0);
    let words = op.encode(&mut enc).expect("MemBar CTA");
    assert!(!words.is_empty());
}

#[test]
fn membar_gpu_scope() {
    let op = OpMemBar {
        scope: MemScope::GPU,
    };
    let labels = FxHashMap::default();
    let mut enc = AmdOpEncoder::new(&labels, 0, 254, 255, 10, 2, 0);
    let words = op.encode(&mut enc).expect("MemBar GPU");
    assert!(!words.is_empty());
}

#[test]
fn membar_system_scope() {
    let op = OpMemBar {
        scope: MemScope::System,
    };
    let labels = FxHashMap::default();
    let mut enc = AmdOpEncoder::new(&labels, 0, 254, 255, 10, 2, 0);
    let words = op.encode(&mut enc).expect("MemBar System");
    assert!(!words.is_empty());
}
