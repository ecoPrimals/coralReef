// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    CBuf, CBufRef, Dst, FloatCmpOp, Label, OpHAdd2, OpHFma2, OpHMnMx2, OpHMul2, OpHSet2, OpHSetP2,
    PredSetOp, RegFile, RegRef, Src, SrcMod, SrcRef, SrcSwizzle,
};
use crate::codegen::nv::sm70_encode::encoder::{SM70Encoder, SM70Op};

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: SrcRef::Reg(RegRef::new(RegFile::GPR, idx, 2)),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn gpr_src_xy(idx: u32) -> Src {
    Src {
        reference: SrcRef::Reg(RegRef::new(RegFile::GPR, idx, 2)),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::Xx,
    }
}

fn imm_src(u: u32) -> Src {
    Src::new_imm_u32(u)
}

fn cb_src(cb: CBufRef) -> Src {
    Src {
        reference: SrcRef::CBuf(cb),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn encoder(sm: u8) -> SM70Encoder<'static> {
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
fn op_hadd2_gpr_src1_and_imm_in_src2_slot() {
    let op_rr = OpHAdd2 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 10, 2)),
        srcs: [gpr_src(1), gpr_src(2)],
        saturate: true,
        ftz: true,
        f32: true,
    };
    let mut e = encoder(70);
    op_rr.encode(&mut e);
    assert_eq!(alu_opcode(&e), 0x030);
    assert!(e.get_bit(77), "sat");
    assert!(e.get_bit(78), "f32");
    assert!(e.get_bit(80), "ftz");
    assert!(!e.get_bit(85), "BF16_V2");

    let op_ri = OpHAdd2 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 10, 2)),
        srcs: [gpr_src(1), imm_src(0x3c00_0000)],
        saturate: false,
        ftz: false,
        f32: false,
    };
    let mut e = encoder(70);
    op_ri.encode(&mut e);
    assert_eq!(alu_opcode(&e), 0x030);
    assert_eq!(e.get_field(9..12), 2, "form: imm passed in src2 slot");
}

#[test]
fn op_hmul2_dnz_and_ftz_modes() {
    let op_dnz = OpHMul2 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 3, 2)),
        srcs: [gpr_src(4), gpr_src(5)],
        saturate: false,
        ftz: false,
        dnz: true,
    };
    let mut e = encoder(75);
    op_dnz.encode(&mut e);
    assert_eq!(alu_opcode(&e), 0x032);
    assert!(e.get_bit(76), "dnz");

    let op_ftz = OpHMul2 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 3, 2)),
        srcs: [gpr_src(4), gpr_src(5)],
        saturate: true,
        ftz: true,
        dnz: false,
    };
    let mut e = encoder(75);
    op_ftz.encode(&mut e);
    assert!(e.get_bit(77), "sat");
    assert!(e.get_bit(80), "ftz");
    assert!(!e.get_bit(78), "hmul2 f32 bit stays clear on SM70-SM75");
}

#[test]
fn op_hfma2_f32_dnz_roundtrip_bits() {
    let op = OpHFma2 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 6, 2)),
        srcs: [gpr_src(1), gpr_src(2), gpr_src(3)],
        saturate: true,
        ftz: true,
        dnz: true,
        f32: true,
    };
    let mut e = encoder(80);
    op.encode(&mut e);
    assert_eq!(alu_opcode(&e), 0x031);
    assert!(e.get_bit(76), "dnz");
    assert!(e.get_bit(77), "sat");
    assert!(e.get_bit(78), "f32");
    assert!(e.get_bit(80), "ftz");
}

#[test]
fn op_hset2_cmp_modes_and_ftz() {
    let op = OpHSet2 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 8, 2)),
        set_op: PredSetOp::Xor,
        cmp_op: FloatCmpOp::OrdGt,
        srcs: [gpr_src(1), gpr_src(2), Src::new_imm_bool(false)],
        ftz: true,
    };
    let mut e = encoder(70);
    op.encode(&mut e);
    assert_eq!(alu_opcode(&e), 0x033);
    assert_eq!(e.get_field(69..71), 2, "PredSetOp::Xor");
    assert_eq!(e.get_field(76..80), 0x04, "OrdGt");
    assert!(e.get_bit(71), "BF output");
    assert!(e.get_bit(80), "ftz");
}

#[test]
fn op_hset2_non_gpr_src1_uses_alternate_slot() {
    let op = OpHSet2 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 8, 2)),
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdEq,
        srcs: [
            gpr_src(1),
            cb_src(CBufRef {
                buf: CBuf::Binding(1),
                offset: 0,
            }),
            Src::new_imm_bool(false),
        ],
        ftz: false,
    };
    let mut e = encoder(70);
    op.encode(&mut e);
    assert_eq!(e.get_field(9..12), 3, "form: cbuf src1");
}

#[test]
fn op_hsetp2_pred_dsts_horizontal_and_cmp() {
    let op = OpHSetP2 {
        dsts: [
            Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
            Dst::Reg(RegRef::new(RegFile::Pred, 2, 1)),
        ],
        set_op: PredSetOp::Or,
        cmp_op: FloatCmpOp::OrdLe,
        srcs: [gpr_src(3), gpr_src(4), Src::new_imm_bool(true)],
        ftz: true,
        horizontal: true,
    };
    let mut e = encoder(73);
    op.encode(&mut e);
    assert_eq!(alu_opcode(&e), 0x034);
    assert_eq!(e.get_field(69..71), 1, "PredSetOp::Or");
    assert_eq!(e.get_field(76..80), 0x03, "OrdLe");
    assert!(e.get_bit(71), "horizontal H_AND");
    assert!(e.get_bit(80), "ftz");
}

#[test]
fn op_hsetp2_src1_zero_slot() {
    let op = OpHSetP2 {
        dsts: [
            Dst::Reg(RegRef::new(RegFile::Pred, 0, 1)),
            Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
        ],
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdNe,
        srcs: [gpr_src(2), Src::ZERO, Src::new_imm_bool(false)],
        ftz: false,
        horizontal: false,
    };
    let mut e = encoder(70);
    op.encode(&mut e);
    assert_eq!(alu_opcode(&e), 0x034);
}

#[test]
fn op_hmnmx2_requires_sm80_and_pred_min() {
    let op = OpHMnMx2 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 9, 2)),
        srcs: [gpr_src_xy(1), gpr_src(2), Src::new_imm_bool(true)],
        ftz: true,
    };
    let mut e = encoder(89);
    op.encode(&mut e);
    assert_eq!(alu_opcode(&e), 0x040);
    assert!(e.get_bit(80), "ftz");
}

#[test]
fn op_hadd2_swizzle_on_src0_encodes() {
    let op = OpHAdd2 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 2)),
        srcs: [gpr_src_xy(6), gpr_src(7)],
        saturate: false,
        ftz: false,
        f32: false,
    };
    let mut e = encoder(70);
    op.encode(&mut e);
    assert_eq!(e.get_field(74..76), 2, "SrcSwizzle::Xx");
}
