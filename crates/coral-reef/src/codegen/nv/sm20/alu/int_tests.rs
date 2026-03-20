// SPDX-License-Identifier: AGPL-3.0-only

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    Dst, IMadSpMode, IMadSpSrcType, IntCmpOp, IntCmpType, Label, LogicOp2, OpBfe, OpFlo, OpIAdd2,
    OpIAdd2X, OpIMad, OpIMadSp, OpIMnMx, OpIMul, OpISetP, OpLop2, OpPopC, OpShl, OpShr, OpSuBfm,
    OpSuClamp, OpSuEau, PredSetOp, RegFile, RegRef, Src, SrcMod, SrcSwizzle, SuClampMode,
    SuClampRound,
};

use super::super::super::encoder::{SM20Encoder, SM20Op, SM20Unit, ShaderModel20};

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
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
fn op_bfe_int_unit_and_subop() {
    let mut e = sm20_encoder();
    let op = OpBfe {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3)],
        signed: true,
        reverse: true,
    };
    op.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Int as u64);
    assert_eq!(opcode_byte(&e), 0x1c);
    assert!(e.get_bit(5), "signed");
    assert!(e.get_bit(8), "reverse");
}

#[test]
fn op_flo_form_b() {
    let mut e = sm20_encoder();
    let op = OpFlo {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: gpr_src(4),
        signed: false,
        return_shift_amount: true,
    };
    op.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Int as u64);
    assert_eq!(opcode_byte(&e), 0x1e);
    assert!(e.get_bit(6), "return_shift_amount");
}

#[test]
fn op_iadd2_reg_and_imm32_wide() {
    let mut e = sm20_encoder();
    let op = OpIAdd2 {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        srcs: [gpr_src(2), gpr_src(3)],
    };
    op.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Int as u64);
    assert_eq!(opcode_byte(&e), 0x12);
    assert!(!e.get_bit(48), "carry_out bit for reg form (none)");

    let mut e = sm20_encoder();
    let op = OpIAdd2 {
        dsts: [
            Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
            Dst::Reg(RegRef::new(RegFile::Carry, 0, 1)),
        ],
        srcs: [gpr_src(2), Src::new_imm_u32(0x0010_0000)],
    };
    op.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Imm32 as u64);
    assert_eq!(opcode_byte(&e), 0x03);
    assert!(e.get_bit(58), "carry_out imm path");
}

#[test]
fn op_iadd2x_carry_in() {
    let mut e = sm20_encoder();
    let op = OpIAdd2X {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        srcs: [
            gpr_src(2),
            gpr_src(3),
            Src {
                reference: RegRef::new(RegFile::Carry, 0, 1).into(),
                modifier: SrcMod::None,
                swizzle: SrcSwizzle::None,
            },
        ],
    };
    op.encode(&mut e);
    assert!(e.get_bit(6), "carry_in");
}

#[test]
fn op_imad_signed_neg_flags() {
    let mut e = sm20_encoder();
    let op = OpIMad {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        signed: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_byte(&e), 0x08);
    assert!(e.get_bit(5), "signed ab");
    assert!(e.get_bit(7), "signed c");
}

#[test]
fn op_imul_reg_and_imm32() {
    let mut e = sm20_encoder();
    let op = OpIMul {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3)],
        signed: [true, false],
        high: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_byte(&e), 0x14);
    assert!(e.get_bit(6), "high");

    let mut e = sm20_encoder();
    let op = OpIMul {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), Src::new_imm_u32(0x0020_0000)],
        signed: [false, false],
        high: false,
    };
    op.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Imm32 as u64);
    assert_eq!(opcode_byte(&e), 0x04);
}

#[test]
fn op_imnmx_cmp_type() {
    let mut e = sm20_encoder();
    let op = OpIMnMx {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        cmp_type: IntCmpType::I32,
        srcs: [gpr_src(2), gpr_src(3), Src::new_imm_bool(false)],
    };
    op.encode(&mut e);
    assert_eq!(e.get_field(5..6), 1, "signed min/max");
}

#[test]
fn op_isetp_cmp_ops() {
    for (cmp_op, enc) in [
        (IntCmpOp::Eq, 2_u64),
        (IntCmpOp::Ne, 5),
        (IntCmpOp::Lt, 1),
        (IntCmpOp::Le, 3),
        (IntCmpOp::Gt, 4),
        (IntCmpOp::Ge, 6),
    ] {
        let mut e = sm20_encoder();
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
        assert_eq!(opcode_byte(&e), 0x06);
        assert_eq!(e.get_field(55..58), enc);
    }
}

#[test]
fn op_lop2_xor_and_imm() {
    let mut e = sm20_encoder();
    let op = OpLop2 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        op: LogicOp2::Xor,
        srcs: [gpr_src(2), gpr_src(3)],
    };
    op.encode(&mut e);
    assert_eq!(e.get_field(6..8), 2, "xor");

    let mut e = sm20_encoder();
    let op = OpLop2 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        op: LogicOp2::Or,
        srcs: [gpr_src(2), Src::new_imm_u32(0x0100_0000)],
    };
    op.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Imm32 as u64);
    assert_eq!(opcode_byte(&e), 0x0e);
}

#[test]
fn op_popc_uses_move_unit() {
    let mut e = sm20_encoder();
    let op = OpPopC {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: gpr_src(5),
    };
    op.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Move as u64);
    assert_eq!(opcode_byte(&e), 0x15);
}

#[test]
fn op_shl_shr() {
    let mut e = sm20_encoder();
    let op = OpShl {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3)],
        wrap: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_byte(&e), 0x18);
    assert!(e.get_bit(9), "shl wrap");

    let mut e = sm20_encoder();
    let op = OpShr {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3)],
        wrap: false,
        signed: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_byte(&e), 0x16);
    assert!(e.get_bit(5), "shr signed");
}

#[test]
fn op_su_clamp_encodes_mode_bits() {
    let mut e = sm20_encoder();
    let op = OpSuClamp {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        srcs: [gpr_src(2), gpr_src(3)],
        mode: SuClampMode::PitchLinear,
        round: SuClampRound::R4,
        is_s32: true,
        is_2d: true,
        imm: 0x15,
    };
    op.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Move as u64);
    assert_eq!(opcode_byte(&e), 0x16);
    assert_eq!(e.get_field(5..9), 7, "pitch linear + R4");
    assert!(e.get_bit(9), "is_s32");
    assert!(e.get_bit(48), "is_2d");
}

#[test]
fn op_su_bfm_and_su_eau() {
    let mut e = sm20_encoder();
    let op = OpSuBfm {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        is_3d: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_byte(&e), 0x17);
    assert!(e.get_bit(48), "is_3d");

    let mut e = sm20_encoder();
    let op = OpSuEau {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(1), gpr_src(2), gpr_src(3)],
    };
    op.encode(&mut e);
    assert_eq!(opcode_byte(&e), 0x18);
}

#[test]
fn op_imad_sp_from_src1() {
    let mut e = sm20_encoder();
    let op = OpIMadSp {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        mode: IMadSpMode::FromSrc1,
    };
    op.encode(&mut e);
    assert_eq!(opcode_byte(&e), 0x00);
    assert_eq!(e.get_field(55..57), 3);
}

#[test]
fn op_imad_sp_explicit_sign_combo() {
    let mut e = sm20_encoder();
    let op = OpIMadSp {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        mode: IMadSpMode::Explicit([IMadSpSrcType::S32, IMadSpSrcType::U24, IMadSpSrcType::S32]),
    };
    op.encode(&mut e);
    assert_eq!(opcode_byte(&e), 0x00);
    assert!(!e.get_bit(5), "src1 U24 is unsigned");
    assert!(e.get_bit(7), "src0 S32 sign");
}
