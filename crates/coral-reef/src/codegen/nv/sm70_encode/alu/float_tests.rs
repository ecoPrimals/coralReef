// SPDX-License-Identifier: AGPL-3.0-or-later

//! Encoder tests for SM70 FP32 ALU ops (`alu/float.rs`).

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    Dst, FRndMode, FSwzAddOp, FloatCmpOp, Label, OpFAdd, OpFFma, OpFMnMx, OpFMul, OpFSet, OpFSetP,
    OpFSwzAdd, PredSetOp, RegFile, RegRef, Src, SrcMod, SrcSwizzle, TexDerivMode,
};

use super::super::encoder::{SM70Encoder, SM70Op};

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn gpr_dst(idx: u32) -> Dst {
    RegRef::new(RegFile::GPR, idx, 1).into()
}

fn pred_dst(idx: u32) -> Dst {
    RegRef::new(RegFile::Pred, idx, 1).into()
}

fn encoder() -> SM70Encoder<'static> {
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    SM70Encoder {
        sm: 70,
        ip: 0,
        labels,
        inst: [0_u32; 4],
    }
}

fn opcode(e: &SM70Encoder<'_>) -> u64 {
    e.get_field(0..9)
}

#[test]
fn fadd_opcode() {
    let op = OpFAdd {
        dst: gpr_dst(0),
        srcs: [gpr_src(1), gpr_src(2)],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x021);
}

#[test]
fn fadd_saturate_bit() {
    let op = OpFAdd {
        dst: gpr_dst(0),
        srcs: [gpr_src(1), gpr_src(2)],
        saturate: true,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert!(e.get_bit(77));
}

#[test]
fn fadd_ftz_bit() {
    let op = OpFAdd {
        dst: gpr_dst(0),
        srcs: [gpr_src(1), gpr_src(2)],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: true,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert!(e.get_bit(80));
}

#[test]
fn ffma_opcode() {
    let op = OpFFma {
        dst: gpr_dst(0),
        srcs: [gpr_src(1), gpr_src(2), gpr_src(3)],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x023);
}

#[test]
fn ffma_dnz_bit() {
    let op = OpFFma {
        dst: gpr_dst(0),
        srcs: [gpr_src(1), gpr_src(2), gpr_src(3)],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: true,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert!(e.get_bit(76));
}

#[test]
fn ffma_saturate_bit() {
    let op = OpFFma {
        dst: gpr_dst(0),
        srcs: [gpr_src(1), gpr_src(2), gpr_src(3)],
        saturate: true,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert!(e.get_bit(77));
}

#[test]
fn fmnmx_opcode() {
    let op = OpFMnMx {
        dst: gpr_dst(0),
        srcs: [gpr_src(1), gpr_src(2), Src::new_imm_bool(true)],
        ftz: false,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x009);
}

#[test]
fn fmnmx_ftz_bit() {
    let op = OpFMnMx {
        dst: gpr_dst(0),
        srcs: [gpr_src(1), gpr_src(2), Src::new_imm_bool(true)],
        ftz: true,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert!(e.get_bit(80));
}

#[test]
fn fmul_opcode() {
    let op = OpFMul {
        dst: gpr_dst(0),
        srcs: [gpr_src(1), gpr_src(2)],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x020);
}

#[test]
fn fmul_pdiv_field() {
    let op = OpFMul {
        dst: gpr_dst(0),
        srcs: [gpr_src(1), gpr_src(2)],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    };
    let mut e = encoder();
    op.encode(&mut e);
    let pdiv: u64 = e.get_field(84..87);
    assert_eq!(pdiv, 0x4);
}

#[test]
fn fset_opcode() {
    let op = OpFSet {
        dst: gpr_dst(0),
        srcs: [gpr_src(1), gpr_src(2)],
        cmp_op: FloatCmpOp::OrdLt,
        ftz: false,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x00a);
}

#[test]
fn fset_ftz_bit() {
    let op = OpFSet {
        dst: gpr_dst(0),
        srcs: [gpr_src(1), gpr_src(2)],
        cmp_op: FloatCmpOp::OrdLt,
        ftz: true,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert!(e.get_bit(80));
}

#[test]
fn fsetp_opcode() {
    let op = OpFSetP {
        dst: pred_dst(0),
        srcs: [gpr_src(1), gpr_src(2), Src::new_imm_bool(true)],
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdEq,
        ftz: false,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x00b);
}

#[test]
fn fsetp_ftz_bit() {
    let op = OpFSetP {
        dst: pred_dst(0),
        srcs: [gpr_src(1), gpr_src(2), Src::new_imm_bool(true)],
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdEq,
        ftz: true,
    };
    let mut e = encoder();
    op.encode(&mut e);
    assert!(e.get_bit(80));
}

#[test]
fn fsetp_pred_dst_written() {
    let op = OpFSetP {
        dst: pred_dst(3),
        srcs: [gpr_src(1), gpr_src(2), Src::new_imm_bool(true)],
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdEq,
        ftz: false,
    };
    let mut e = encoder();
    op.encode(&mut e);
    let pred: u64 = e.get_field(81..84);
    assert_eq!(pred, 3);
}

#[test]
fn fswzadd_opcode() {
    let op = OpFSwzAdd {
        dst: gpr_dst(0),
        srcs: [gpr_src(1), gpr_src(2)],
        ops: [
            FSwzAddOp::Add,
            FSwzAddOp::Add,
            FSwzAddOp::Add,
            FSwzAddOp::Add,
        ],
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        deriv_mode: TexDerivMode::Auto,
    };
    let mut e = encoder();
    op.encode(&mut e);
    let opc: u64 = e.get_field(0..12);
    assert_eq!(opc, 0x822);
}

#[test]
fn fswzadd_subleft_encoding() {
    let op = OpFSwzAdd {
        dst: gpr_dst(0),
        srcs: [gpr_src(1), gpr_src(2)],
        ops: [
            FSwzAddOp::SubLeft,
            FSwzAddOp::SubRight,
            FSwzAddOp::MoveLeft,
            FSwzAddOp::Add,
        ],
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        deriv_mode: TexDerivMode::Auto,
    };
    let mut e = encoder();
    op.encode(&mut e);
    // SubLeft=1(bits 6-7), SubRight=2(bits 4-5), MoveLeft=3(bits 2-3), Add=0(bits 0-1) → 0b01_10_11_00 = 0x6C
    let subop: u64 = e.get_field(32..40);
    assert_eq!(subop, 0x6C);
}
