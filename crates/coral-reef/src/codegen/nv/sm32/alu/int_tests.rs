// SPDX-License-Identifier: AGPL-3.0-or-later

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    CBuf, CBufRef, Dst, IntCmpOp, IntCmpType, IntType, Label, LogicOp2, OpBfe, OpFlo, OpIAdd2,
    OpIAdd2X, OpIMad, OpIMnMx, OpIMul, OpISetP, OpLop2, OpPopC, OpShf, OpShl, OpShr, PredSetOp,
    RegFile, RegRef, Src, SrcMod, SrcRef, SrcSwizzle,
};

use super::super::super::encoder::{SM32Encoder, SM32Op, ShaderModel32};

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

fn sm32_encoder() -> SM32Encoder<'static> {
    let sm: &'static ShaderModel32 = Box::leak(Box::new(ShaderModel32::new(32)));
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    SM32Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 2],
    }
}

fn opcode_sm32(e: &SM32Encoder<'_>) -> u64 {
    e.get_field(52..64)
}

#[test]
fn op_bfe_immreg_and_flags() {
    let mut e = sm32_encoder();
    let op = OpBfe {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3)],
        signed: true,
        reverse: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_sm32(&e), 0xe00);
    assert!(e.get_bit(51), "signed");
    assert!(e.get_bit(43), "reverse");
}

#[test]
fn op_flo_reg_and_cbuf() {
    let mut e = sm32_encoder();
    let op = OpFlo {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: gpr_src(4),
        signed: false,
        return_shift_amount: false,
    };
    op.encode(&mut e);
    assert_eq!(opcode_sm32(&e), 0xe18);
    assert_eq!(e.get_field(0..2), 2, "functional unit");

    let mut e = sm32_encoder();
    let op = OpFlo {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: cbuf_src(0, 0x20),
        signed: true,
        return_shift_amount: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_sm32(&e), 0x618);
}

#[test]
fn op_iadd2_src1_ineg() {
    let mut e = sm32_encoder();
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
    assert!(e.get_bit(51), "src1 ineg");
}

#[test]
fn op_iadd2_reg_and_imm32_wide() {
    let mut e = sm32_encoder();
    let op = OpIAdd2 {
        dsts: [
            Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
            Dst::Reg(RegRef::new(RegFile::Carry, 0, 1)),
        ],
        srcs: [gpr_src(2), gpr_src(3)],
    };
    op.encode(&mut e);
    assert_eq!(opcode_sm32(&e), 0xe08);
    assert!(e.get_bit(50), "carry_out");

    let mut e = sm32_encoder();
    let op = OpIAdd2 {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        srcs: [gpr_src(2), Src::new_imm_u32(0x0010_0000)],
    };
    op.encode(&mut e);
    assert_eq!(opcode_sm32(&e), 0x400);
    assert_eq!(e.get_field(0..2), 1, "imm32 unit");
}

#[test]
fn op_iadd2x_imm32() {
    let mut e = sm32_encoder();
    let op = OpIAdd2X {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        srcs: [
            gpr_src(2),
            Src::new_imm_u32(0x0030_0000),
            Src {
                reference: RegRef::new(RegFile::Carry, 0, 1).into(),
                modifier: SrcMod::None,
                swizzle: SrcSwizzle::None,
            },
        ],
    };
    op.encode(&mut e);
    assert_eq!(opcode_sm32(&e), 0x410);
    assert!(e.get_bit(56), ".X");
}

#[test]
fn op_imad_src1_imm20() {
    let mut e = sm32_encoder();
    let op = OpIMad {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), Src::new_imm_u32(0x20), gpr_src(5)],
        signed: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_sm32(&e), 0xa10);
}

#[test]
fn op_imad_signed_bits() {
    let mut e = sm32_encoder();
    let op = OpIMad {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        signed: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_sm32(&e), 0xd10);
    assert!(e.get_bit(56), "signed mul");
    assert!(e.get_bit(51), "signed mad");
}

#[test]
fn op_imul_reg_and_imm32_wide() {
    let mut e = sm32_encoder();
    let op = OpIMul {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3)],
        signed: [true, false],
        high: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_sm32(&e), 0xe1c);
    assert!(e.get_bit(42), "high");
    assert!(e.get_bit(43), "signed[0]");
    assert!(!e.get_bit(44), "signed[1]");

    let mut e = sm32_encoder();
    let op = OpIMul {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), Src::new_imm_u32(0x0100_0000)],
        signed: [false, true],
        high: false,
    };
    op.encode(&mut e);
    assert_eq!(opcode_sm32(&e), 0x2c0);
}

#[test]
fn op_imnmx_signed() {
    let mut e = sm32_encoder();
    let op = OpIMnMx {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        cmp_type: IntCmpType::I32,
        srcs: [gpr_src(2), gpr_src(3), Src::new_imm_bool(true)],
    };
    op.encode(&mut e);
    assert_eq!(opcode_sm32(&e), 0xe10);
    assert!(e.get_bit(51), "signed");
}

#[test]
fn op_isetp_signed_cmp_and_false_true() {
    let mut e = sm32_encoder();
    let op = OpISetP {
        dst: Dst::Reg(RegRef::new(RegFile::Pred, 2, 1)),
        set_op: PredSetOp::Xor,
        cmp_op: IntCmpOp::False,
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
    assert_eq!(e.get_field(52..55), 0, "false cmp");

    let mut e = sm32_encoder();
    let op = OpISetP {
        dst: Dst::Reg(RegRef::new(RegFile::Pred, 3, 1)),
        set_op: PredSetOp::Or,
        cmp_op: IntCmpOp::True,
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
    assert_eq!(e.get_field(52..55), 7, "true cmp");
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
        let mut e = sm32_encoder();
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
        assert_eq!(e.get_field(52..55), enc);
    }
}

#[test]
fn op_lop2_xor_and_imm() {
    let mut e = sm32_encoder();
    let op = OpLop2 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        op: LogicOp2::Xor,
        srcs: [gpr_src(2), gpr_src(3)],
    };
    op.encode(&mut e);
    assert_eq!(opcode_sm32(&e), 0xe20);
    assert_eq!(e.get_field(44..46), 2, "xor");

    let mut e = sm32_encoder();
    let op = OpLop2 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        op: LogicOp2::And,
        srcs: [gpr_src(2), Src::new_imm_u32(0x0200_0000)],
    };
    op.encode(&mut e);
    assert_eq!(opcode_sm32(&e), 0x200);
}

#[test]
fn op_popc_encode() {
    let mut e = sm32_encoder();
    let op = OpPopC {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: gpr_src(5),
    };
    op.encode(&mut e);
    assert_eq!(opcode_sm32(&e), 0xe04);
}

#[test]
fn op_shf_data_types() {
    for (data_type, ty_enc) in [(IntType::U32, 0_u64), (IntType::U64, 2), (IntType::I64, 3)] {
        let mut e = sm32_encoder();
        let op = OpShf {
            dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
            srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
            right: true,
            wrap: false,
            data_type,
            dst_high: true,
        };
        op.encode(&mut e);
        assert_eq!(opcode_sm32(&e), 0xe7c);
        assert_eq!(e.get_field(40..42), ty_enc);
    }
}

#[test]
fn op_shl_shr() {
    let mut e = sm32_encoder();
    let op = OpShl {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3)],
        wrap: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_sm32(&e), 0xe24);
    assert!(e.get_bit(42), "shl wrap");

    let mut e = sm32_encoder();
    let op = OpShr {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3)],
        wrap: false,
        signed: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode_sm32(&e), 0xe14);
    assert!(e.get_bit(51), "shr signed");
}
