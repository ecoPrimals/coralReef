// SPDX-License-Identifier: AGPL-3.0-or-later

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    Dst, FRndMode, FloatType, IntType, Label, OpF2F, OpF2I, OpI2F, OpI2I, RegFile, RegRef, Src,
    SrcMod, SrcSwizzle,
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
fn op_f2f_move_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpF2F {
        dst: gpr_dst(1),
        src: gpr_src(2),
        dst_type: FloatType::F32,
        src_type: FloatType::F32,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dst_high: false,
        integer_rnd: false,
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Move as u64);
    assert_eq!(opcode_byte(&e), 0x4);
}

#[test]
fn op_f2f_ftz_bit() {
    let mut e = sm20_encoder();
    OpF2F {
        dst: gpr_dst(1),
        src: gpr_src(2),
        dst_type: FloatType::F32,
        src_type: FloatType::F32,
        rnd_mode: FRndMode::NearestEven,
        ftz: true,
        dst_high: false,
        integer_rnd: false,
    }
    .encode(&mut e);
    assert!(e.get_bit(55), "FTZ should be bit 55");
}

#[test]
fn op_f2i_move_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpF2I {
        dst: gpr_dst(1),
        src: gpr_src(2),
        dst_type: IntType::I32,
        src_type: FloatType::F32,
        rnd_mode: FRndMode::Zero,
        ftz: false,
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Move as u64);
    assert_eq!(opcode_byte(&e), 0x5);
}

#[test]
fn op_f2i_signed_bit() {
    let mut e = sm20_encoder();
    OpF2I {
        dst: gpr_dst(1),
        src: gpr_src(2),
        dst_type: IntType::I32,
        src_type: FloatType::F32,
        rnd_mode: FRndMode::Zero,
        ftz: false,
    }
    .encode(&mut e);
    assert!(e.get_bit(7), "signed bit should be set for I32");
}

#[test]
fn op_f2i_unsigned() {
    let mut e = sm20_encoder();
    OpF2I {
        dst: gpr_dst(1),
        src: gpr_src(2),
        dst_type: IntType::U32,
        src_type: FloatType::F32,
        rnd_mode: FRndMode::Zero,
        ftz: false,
    }
    .encode(&mut e);
    assert!(!e.get_bit(7), "signed bit should be clear for U32");
}

#[test]
fn op_i2f_move_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpI2F {
        dst: gpr_dst(1),
        src: gpr_src(2),
        dst_type: FloatType::F32,
        src_type: IntType::I32,
        rnd_mode: FRndMode::NearestEven,
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Move as u64);
    assert_eq!(opcode_byte(&e), 0x6);
}

#[test]
fn op_i2f_signed_src_bit() {
    let mut e = sm20_encoder();
    OpI2F {
        dst: gpr_dst(1),
        src: gpr_src(2),
        dst_type: FloatType::F32,
        src_type: IntType::I32,
        rnd_mode: FRndMode::NearestEven,
    }
    .encode(&mut e);
    assert!(e.get_bit(9), "signed src bit should be set for I32");
}

#[test]
fn op_i2i_move_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpI2I {
        dst: gpr_dst(1),
        src: gpr_src(2),
        dst_type: IntType::I32,
        src_type: IntType::U16,
        saturate: false,
        abs: false,
        neg: false,
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Move as u64);
    assert_eq!(opcode_byte(&e), 0x7);
}

#[test]
fn op_i2i_saturate_bit() {
    let mut e = sm20_encoder();
    OpI2I {
        dst: gpr_dst(1),
        src: gpr_src(2),
        dst_type: IntType::I32,
        src_type: IntType::U16,
        saturate: true,
        abs: false,
        neg: false,
    }
    .encode(&mut e);
    assert!(e.get_bit(5), "saturate should be bit 5");
}

#[test]
fn op_i2i_abs_and_neg_bits() {
    let mut e = sm20_encoder();
    OpI2I {
        dst: gpr_dst(1),
        src: gpr_src(2),
        dst_type: IntType::I32,
        src_type: IntType::I32,
        saturate: false,
        abs: true,
        neg: true,
    }
    .encode(&mut e);
    assert!(e.get_bit(6), "abs should be bit 6");
    assert!(e.get_bit(8), "neg should be bit 8");
}
