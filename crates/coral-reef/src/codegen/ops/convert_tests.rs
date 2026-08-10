// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! Tests for type conversion operation encoding — F2F, F2I, I2F, I2I.

use super::encode_amd_op;
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
fn f2f_f32_to_f64() {
    let op = OpF2F {
        dst: gpr_dst(0),
        src: gpr_src(1),
        src_type: FloatType::F32,
        dst_type: FloatType::F64,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dst_high: false,
        integer_rnd: false,
    };
    let words = enc_op(Op::F2F(Box::new(op))).expect("F2F f32→f64");
    assert!(!words.is_empty());
}

#[test]
fn f2f_f64_to_f32() {
    let op = OpF2F {
        dst: gpr_dst(0),
        src: gpr_src(2),
        src_type: FloatType::F64,
        dst_type: FloatType::F32,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dst_high: false,
        integer_rnd: false,
    };
    let words = enc_op(Op::F2F(Box::new(op))).expect("F2F f64→f32");
    assert!(!words.is_empty());
}

#[test]
fn f2i_f32_to_i32() {
    let op = OpF2I {
        dst: gpr_dst(0),
        src: gpr_src(1),
        src_type: FloatType::F32,
        dst_type: IntType::I32,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    };
    let words = enc_op(Op::F2I(Box::new(op))).expect("F2I f32→i32");
    assert!(!words.is_empty());
}

#[test]
fn i2f_i32_to_f32() {
    let op = OpI2F {
        dst: gpr_dst(0),
        src: gpr_src(1),
        src_type: IntType::I32,
        dst_type: FloatType::F32,
        rnd_mode: FRndMode::NearestEven,
    };
    let words = enc_op(Op::I2F(Box::new(op))).expect("I2F i32→f32");
    assert!(!words.is_empty());
}

#[test]
fn i2f_u32_to_f64() {
    let op = OpI2F {
        dst: gpr_dst(0),
        src: gpr_src(1),
        src_type: IntType::U32,
        dst_type: FloatType::F64,
        rnd_mode: FRndMode::NearestEven,
    };
    let words = enc_op(Op::I2F(Box::new(op))).expect("I2F u32→f64");
    assert!(!words.is_empty());
}

#[test]
fn i2f_i32_to_f64() {
    let op = OpI2F {
        dst: gpr_dst(0),
        src: gpr_src(1),
        src_type: IntType::I32,
        dst_type: FloatType::F64,
        rnd_mode: FRndMode::NearestEven,
    };
    let words = enc_op(Op::I2F(Box::new(op))).expect("I2F i32→f64");
    assert!(!words.is_empty());
}

#[test]
fn i2i_pass_through() {
    let op = OpI2I {
        dst: gpr_dst(0),
        src: gpr_src(1),
        src_type: IntType::I32,
        dst_type: IntType::U32,
        saturate: false,
        abs: false,
        neg: false,
    };
    let words = enc_op(Op::I2I(Box::new(op))).expect("I2I i32→u32");
    assert!(!words.is_empty());
}
