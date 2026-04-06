// SPDX-License-Identifier: AGPL-3.0-or-later

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use super::super::super::encoder::{SM50Encoder, SM50Op, ShaderModel50};
use crate::codegen::ir::{
    CBuf, CBufRef, Dst, IntCmpOp, IntCmpType, IntType, Label, LogicOp2, OpBfe, OpFlo, OpIAdd2,
    OpIAdd2X, OpIMad, OpIMnMx, OpIMul, OpISetP, OpLop2, OpPopC, OpShf, OpShl, OpShr, PredSetOp,
    RegFile, RegRef, Src, SrcMod, SrcRef, SrcSwizzle,
};

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn cbuf_src(idx: u32, offset: u16) -> Src {
    Src {
        reference: SrcRef::CBuf(CBufRef {
            buf: CBuf::Binding(u8::try_from(idx).expect("test cbuf binding index")),
            offset,
        }),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn sm50_encoder() -> SM50Encoder<'static> {
    let sm: &'static ShaderModel50 = Box::leak(Box::new(ShaderModel50::new(50)));
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    SM50Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 2],
        sched: 0,
    }
}

/// Bits 48..64: primary opcode halfword; bit 48 is often overlaid by sign/pred sideband.
fn opcode_hi(e: &SM50Encoder<'_>) -> u64 {
    e.get_field(48..64)
}

#[test]
fn sm50_set_opcode_round_trips_in_bits_48_63() {
    let mut e = sm50_encoder();
    e.set_opcode(0x5c00);
    assert_eq!(e.get_field(48..64), 0x5c00);
}

#[test]
fn op_bfe_reg_imm_cbuf_and_flags() {
    let mut e = sm50_encoder();
    let op = OpBfe {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3)],
        signed: false,
        reverse: false,
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x5c00);
    assert!(!e.get_bit(48), "unsigned bfe");
    assert!(!e.get_bit(40), "reverse");

    let mut e = sm50_encoder();
    let op = OpBfe {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), cbuf_src(0, 0x10)],
        signed: true,
        reverse: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x4c01, "0x4c00 | sign bit 48");
    assert!(e.get_bit(48), "signed bfe");
    assert!(e.get_bit(40), "reverse");
}

#[test]
fn op_bfe_imm_range() {
    let mut e = sm50_encoder();
    let op = OpBfe {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), Src::new_imm_u32(0x00ff_00aa)],
        signed: false,
        reverse: false,
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x3800);
}

#[test]
fn op_flo_bnot_on_reg() {
    let mut e = sm50_encoder();
    let op = OpFlo {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: Src {
            reference: gpr_src(4).reference,
            modifier: SrcMod::BNot,
            swizzle: SrcSwizzle::None,
        },
        signed: false,
        return_shift_amount: false,
    };
    op.encode(&mut e);
    assert!(e.get_bit(40), "bnot");
}

#[test]
fn op_flo_src_modes() {
    let mut e = sm50_encoder();
    let op = OpFlo {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: gpr_src(4),
        signed: false,
        return_shift_amount: false,
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x5c30);
    assert!(!e.get_bit(48), "unsigned flo");
    assert!(!e.get_bit(41), "return_shift_amount");

    let mut e = sm50_encoder();
    let op = OpFlo {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: Src::new_imm_u32(0x42),
        signed: true,
        return_shift_amount: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x3831, "0x3830 | sign bit 48");
    assert!(e.get_bit(48), "signed flo");
    assert!(e.get_bit(41), "return_shift_amount");

    let mut e = sm50_encoder();
    let op = OpFlo {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: cbuf_src(1, 4),
        signed: false,
        return_shift_amount: false,
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x4c30);
}

#[test]
fn op_iadd2_src1_ineg_bits() {
    let mut e = sm50_encoder();
    let op = OpIAdd2 {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        srcs: [
            gpr_src(2),
            Src {
                reference: gpr_src(3).reference,
                modifier: SrcMod::INeg,
                swizzle: SrcSwizzle::None,
            },
        ],
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x5c11, "0x5c10 | src1 ineg bit 48");
    assert!(e.get_bit(48), "src1 ineg");
    assert!(!e.get_bit(49), "src0 unmodified");
}

#[test]
fn op_iadd2_reg_reg_and_carry_carry_out() {
    let mut e = sm50_encoder();
    let op = OpIAdd2 {
        dsts: [
            Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
            Dst::Reg(RegRef::new(RegFile::Carry, 0, 1)),
        ],
        srcs: [gpr_src(2), gpr_src(3)],
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x5c10);
    assert!(e.get_bit(47), "carry_out");
    assert!(!e.get_bit(43), ".X");

    let mut e = sm50_encoder();
    let op = OpIAdd2 {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        srcs: [gpr_src(2), gpr_src(3)],
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x5c10);
    assert!(!e.get_bit(47), "no carry_out");
}

#[test]
fn op_iadd2_imm32_wide_path() {
    let mut e = sm50_encoder();
    let op = OpIAdd2 {
        dsts: [
            Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
            Dst::Reg(RegRef::new(RegFile::Carry, 0, 1)),
        ],
        srcs: [gpr_src(2), Src::new_imm_u32(0x0010_0000)],
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x1c10, "0x1c00 | carry bit 52");
    assert!(e.get_bit(52), "carry_out");
    assert!(!e.get_bit(53), ".X");
}

#[test]
fn op_iadd2_imm20_path() {
    let mut e = sm50_encoder();
    let op = OpIAdd2 {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        srcs: [gpr_src(2), Src::new_imm_u32(0x7_ffff)],
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x3810);
}

#[test]
fn op_iadd2x_carry_in_and_imm32() {
    let mut e = sm50_encoder();
    let op = OpIAdd2X {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        srcs: [
            gpr_src(2),
            Src::new_imm_u32(0x0020_0000),
            Src {
                reference: RegRef::new(RegFile::Carry, 0, 1).into(),
                modifier: SrcMod::None,
                swizzle: SrcSwizzle::None,
            },
        ],
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x1c20, "0x1c00 | .X bit 53");
    assert!(e.get_bit(53), ".X");
}

#[test]
fn op_imad_reg_reg_reg() {
    let mut e = sm50_encoder();
    let op = OpIMad {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        signed: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x5a21, "0x5a00 | sign bits 48 and 53");
    assert!(e.get_bit(48), "src0 signed");
    assert!(e.get_bit(53), "src1 signed");
}

#[test]
fn op_imad_src1_imm_and_src2_reg() {
    let mut e = sm50_encoder();
    let op = OpIMad {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), Src::new_imm_u32(0x10), gpr_src(5)],
        signed: false,
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x3400);
}

#[test]
fn op_imad_ineg_flags() {
    let mut e = sm50_encoder();
    let op = OpIMad {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [
            gpr_src(2),
            Src {
                reference: gpr_src(3).reference,
                modifier: SrcMod::INeg,
                swizzle: SrcSwizzle::None,
            },
            Src {
                reference: gpr_src(4).reference,
                modifier: SrcMod::INeg,
                swizzle: SrcSwizzle::None,
            },
        ],
        signed: false,
    };
    op.encode(&mut e);
    assert!(e.get_bit(51), "ineg imul (src1 neg)");
    assert!(e.get_bit(52), "ineg src2");
}

#[test]
fn op_imad_src2_cbuf() {
    let mut e = sm50_encoder();
    let op = OpIMad {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3), cbuf_src(0, 0x20)],
        signed: false,
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x5200);
}

#[test]
fn op_imul_imm20_path_and_cbuf() {
    let mut e = sm50_encoder();
    let op = OpIMul {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), Src::new_imm_u32(0x3_ffff)],
        signed: [true, true],
        high: false,
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x3838);

    let mut e = sm50_encoder();
    let op = OpIMul {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), cbuf_src(3, 8)],
        signed: [false, false],
        high: false,
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x4c38);
}

#[test]
fn op_imul_reg_reg_and_imm32_wide() {
    let mut e = sm50_encoder();
    let op = OpIMul {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3)],
        signed: [true, false],
        high: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x5c38);
    assert!(e.get_bit(39), "high");
    assert!(e.get_bit(40), "signed[0]");
    assert!(!e.get_bit(41), "signed[1]");

    let mut e = sm50_encoder();
    let op = OpIMul {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), Src::new_imm_u32(0x00ff_0000)],
        signed: [false, true],
        high: false,
    };
    op.encode(&mut e);
    assert_eq!(
        opcode_hi(&e),
        0x1f80,
        "imm32 overlays bits 20..51 including 48..51"
    );
    assert!(!e.get_bit(54), "signed[0]");
    assert!(e.get_bit(55), "signed[1]");
}

#[test]
fn op_imnmx_signed_cmp_and_pred() {
    let mut e = sm50_encoder();
    let op = OpIMnMx {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        cmp_type: IntCmpType::I32,
        srcs: [gpr_src(2), gpr_src(3), Src::new_imm_bool(true)],
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x5c21, "0x5c20 | signed compare bit 48");
    assert!(e.get_bit(48), "signed compare");
}

#[test]
fn op_isetp_int_cmp_false_true() {
    for (cmp_op, enc) in [(IntCmpOp::False, 0_u64), (IntCmpOp::True, 7)] {
        let mut e = sm50_encoder();
        let op = OpISetP {
            dst: Dst::Reg(RegRef::new(RegFile::Pred, 0, 1)),
            set_op: PredSetOp::Xor,
            cmp_op,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [
                gpr_src(2),
                gpr_src(3),
                Src::new_imm_bool(false),
                Src::new_imm_bool(false),
            ],
        };
        op.encode(&mut e);
        assert_eq!(e.get_field(49..52), enc, "{cmp_op:?}");
    }
}

#[test]
fn op_isetp_cmp_ops_and_types() {
    for (cmp_op, enc) in [
        (IntCmpOp::Eq, 2_u64),
        (IntCmpOp::Ne, 5),
        (IntCmpOp::Lt, 1),
        (IntCmpOp::Le, 3),
        (IntCmpOp::Gt, 4),
        (IntCmpOp::Ge, 6),
    ] {
        let mut e = sm50_encoder();
        let op = OpISetP {
            dst: Dst::Reg(RegRef::new(RegFile::Pred, 0, 1)),
            set_op: PredSetOp::And,
            cmp_op,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [
                gpr_src(2),
                gpr_src(3),
                Src::new_imm_bool(false),
                Src::new_imm_bool(false),
            ],
        };
        op.encode(&mut e);
        assert_eq!(e.get_field(49..52), enc);
        assert_eq!(e.get_field(48..49), 0_u64, "unsigned cmp");
    }

    let mut e = sm50_encoder();
    let op = OpISetP {
        dst: Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
        set_op: PredSetOp::Or,
        cmp_op: IntCmpOp::Eq,
        cmp_type: IntCmpType::I32,
        ex: false,
        srcs: [
            gpr_src(2),
            gpr_src(3),
            Src::new_imm_bool(false),
            Src::new_imm_bool(false),
        ],
    };
    op.encode(&mut e);
    assert_eq!(e.get_field(48..49), 1, "signed cmp");
}

#[test]
fn op_lop2_reg_and_imm32() {
    let mut e = sm50_encoder();
    let op = OpLop2 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        op: LogicOp2::Xor,
        srcs: [gpr_src(2), gpr_src(3)],
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x5c47, "0x5c40 | pred dst nibble");
    assert_eq!(e.get_field(41..43), 2, "xor");

    let mut e = sm50_encoder();
    let op = OpLop2 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        op: LogicOp2::And,
        srcs: [gpr_src(2), Src::new_imm_u32(0x0100_0000)],
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x0400);
}

#[test]
fn op_popc_modes() {
    let mut e = sm50_encoder();
    let op = OpPopC {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: gpr_src(4),
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x5c08);

    let mut e = sm50_encoder();
    let op = OpPopC {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: Src::new_imm_u32(0x1234),
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x3808);
}

#[test]
fn op_shf_data_types_int_u32_u64_i64() {
    for (data_type, ty_enc) in [(IntType::U32, 0_u64), (IntType::U64, 2), (IntType::I64, 3)] {
        let mut e = sm50_encoder();
        let op = OpShf {
            dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
            srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
            right: true,
            wrap: false,
            data_type,
            dst_high: true,
        };
        op.encode(&mut e);
        assert_eq!(opcode_hi(&e), 0x5cf9, "0x5cf8 | dst_high bit 48");
        assert_eq!(e.get_field(37..39), ty_enc);
    }
}

#[test]
fn op_shl_and_op_shr_signed() {
    let mut e = sm50_encoder();
    let op = OpShl {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3)],
        wrap: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x5c48);
    assert!(e.get_bit(39), "shl wrap");

    let mut e = sm50_encoder();
    let op = OpShr {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3)],
        wrap: false,
        signed: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x5c29, "0x5c28 | signed bit 48");
    assert!(e.get_bit(48), "shr signed");
}
