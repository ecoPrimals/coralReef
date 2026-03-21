// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals

//! Encoder tests for SM70 conversion ALU ops (`alu/conv.rs`).

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    Dst, FRndMode, FloatType, IntType, Label, OpF2F, OpF2I, OpFRnd, OpI2F, RegFile, RegRef, Src,
    SrcMod, SrcSwizzle,
};

use super::super::encoder::{SM70Encoder, SM70Op};

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn sm70_enc(sm: u8) -> SM70Encoder<'static> {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    SM70Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 4],
    }
}

fn alu_opcode(e: &SM70Encoder<'_>) -> u64 {
    e.get_field(0..9)
}

#[test]
fn op_f2f_f32_rounding_modes_and_ftz() {
    for (rnd, want) in [
        (FRndMode::NearestEven, 0_u64),
        (FRndMode::NegInf, 1),
        (FRndMode::PosInf, 2),
        (FRndMode::Zero, 3),
    ] {
        let mut e = sm70_enc(70);
        OpF2F {
            dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
            src: gpr_src(2),
            src_type: FloatType::F32,
            dst_type: FloatType::F32,
            rnd_mode: rnd,
            ftz: true,
            dst_high: false,
            integer_rnd: false,
        }
        .encode(&mut e);
        assert_eq!(alu_opcode(&e), 0x104);
        assert_eq!(e.get_field(78..80), want);
        assert!(e.get_bit(80), "ftz");
        assert_eq!(e.get_field(75..77), 2, "dst f32");
        assert_eq!(e.get_field(84..86), 2, "src f32");
    }
}

#[test]
fn op_f2f_f64_form_opcode() {
    let mut e = sm70_enc(70);
    OpF2F {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 2)),
        src: gpr_src(2),
        src_type: FloatType::F64,
        dst_type: FloatType::F64,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dst_high: false,
        integer_rnd: false,
    }
    .encode(&mut e);
    assert_eq!(alu_opcode(&e), 0x110);
    assert_eq!(e.get_field(75..77), 3);
    assert_eq!(e.get_field(84..86), 3);
}

#[test]
fn op_f2i_signed_dst_and_f32_src() {
    let mut e = sm70_enc(70);
    OpF2I {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 3, 1)),
        src: gpr_src(4),
        src_type: FloatType::F32,
        dst_type: IntType::I32,
        rnd_mode: FRndMode::Zero,
        ftz: false,
    }
    .encode(&mut e);
    assert_eq!(alu_opcode(&e), 0x105);
    assert!(e.get_bit(72), "signed integer dst");
    assert_eq!(e.get_field(75..77), 2, "i32 dst");
    assert_eq!(e.get_field(78..80), 3, "rounding zero");
    assert_eq!(e.get_field(84..86), 2, "f32 src");
}

#[test]
fn op_f2i_u32_dst() {
    let mut e = sm70_enc(70);
    OpF2I {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: gpr_src(2),
        src_type: FloatType::F32,
        dst_type: IntType::U32,
        rnd_mode: FRndMode::NearestEven,
        ftz: true,
    }
    .encode(&mut e);
    assert!(!e.get_bit(72), "unsigned dst");
    assert!(e.get_bit(80), "ftz");
}

#[test]
fn op_i2f_signed_src_f32_dst() {
    let mut e = sm70_enc(70);
    OpI2F {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 5, 1)),
        src: gpr_src(6),
        dst_type: FloatType::F32,
        src_type: IntType::I32,
        rnd_mode: FRndMode::PosInf,
    }
    .encode(&mut e);
    assert_eq!(alu_opcode(&e), 0x106);
    assert!(e.get_bit(74), "signed src");
    assert_eq!(e.get_field(75..77), 2);
    assert_eq!(e.get_field(78..80), 2);
    assert_eq!(e.get_field(84..86), 2);
}

#[test]
fn op_i2f_unsigned_src() {
    let mut e = sm70_enc(70);
    OpI2F {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: gpr_src(2),
        dst_type: FloatType::F32,
        src_type: IntType::U32,
        rnd_mode: FRndMode::NearestEven,
    }
    .encode(&mut e);
    assert!(!e.get_bit(74));
}

#[test]
fn op_frnd_f32() {
    let mut e = sm70_enc(70);
    OpFRnd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: gpr_src(3),
        dst_type: FloatType::F32,
        src_type: FloatType::F32,
        rnd_mode: FRndMode::NegInf,
        ftz: true,
    }
    .encode(&mut e);
    assert_eq!(alu_opcode(&e), 0x107);
    assert_eq!(e.get_field(78..80), 1);
    assert!(e.get_bit(80), "ftz");
}
