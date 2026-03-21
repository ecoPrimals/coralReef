// SPDX-License-Identifier: AGPL-3.0-only

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    Dst, IntCmpOp, IntCmpType, IntType, Label, LogicOp3, OpBMsk, OpBRev, OpFlo, OpIAbs, OpIAdd3,
    OpIAdd3X, OpIDp4, OpIMad, OpIMad64, OpIMnMx, OpISetP, OpLea, OpLeaX, OpLop3, OpPopC, OpShf,
    PredSetOp, RegFile, RegRef, Src, SrcMod, SrcRef, SrcSwizzle,
};

use super::super::super::encoder::{SM70Encoder, SM70Op};
use super::{fold_lop_src, src_as_lop_imm};

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn pred_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::Pred, idx, 1).into(),
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

fn opcode(e: &SM70Encoder<'_>) -> u64 {
    e.get_field(0..9)
}

#[test]
fn test_src_as_lop_imm_zero() {
    assert_eq!(src_as_lop_imm(&Src::ZERO), Some(false));
}

#[test]
fn test_src_as_lop_imm_true() {
    assert_eq!(src_as_lop_imm(&Src::new_imm_bool(true)), Some(true));
}

#[test]
fn test_src_as_lop_imm_false() {
    assert_eq!(src_as_lop_imm(&Src::new_imm_bool(false)), Some(false));
}

#[test]
fn test_src_as_lop_imm_imm32_zero() {
    let src = Src::new_imm_u32(0);
    assert_eq!(src_as_lop_imm(&src), Some(false));
}

#[test]
fn test_src_as_lop_imm_imm32_all_ones() {
    let src = Src::new_imm_u32(!0);
    assert_eq!(src_as_lop_imm(&src), Some(true));
}

#[test]
fn test_src_as_lop_imm_imm32_other_returns_none() {
    let src = Src::new_imm_u32(42);
    assert_eq!(src_as_lop_imm(&src), None);
}

#[test]
fn test_src_as_lop_imm_reg_returns_none() {
    let reg = RegRef::new(RegFile::GPR, 0, 1);
    let src = Src {
        reference: reg.into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    };
    assert_eq!(src_as_lop_imm(&src), None);
}

#[test]
fn test_src_as_lop_imm_bnot_inverts() {
    let src = Src {
        reference: SrcRef::Zero,
        modifier: SrcMod::BNot,
        swizzle: SrcSwizzle::None,
    };
    assert_eq!(src_as_lop_imm(&src), Some(true));
}

#[test]
fn test_fold_lop_src_with_imm() {
    let src = Src::new_imm_u32(!0);
    let mut x: u8 = 0;
    fold_lop_src(&src, &mut x);
    assert_eq!(x, !0);
}

#[test]
fn op_bmsk_alu_and_wrap() {
    let mut e = encoder(70);
    let op = OpBMsk {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3)],
        wrap: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x01b);
    assert!(e.get_bit(75), "wrap");
}

#[test]
fn op_brev_alu() {
    let mut e = encoder(70);
    let op = OpBRev {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: gpr_src(4),
    };
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x101);
}

#[test]
fn op_flo_signed_and_bnot() {
    let mut e = encoder(70);
    let op = OpFlo {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: gpr_src(4).bnot(),
        signed: true,
        return_shift_amount: false,
    };
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x100);
    assert_eq!(e.get_field(73..74), 1, "signed");
    assert_eq!(e.get_field(63..64), 1, "bnot");
}

#[test]
fn op_iabs() {
    let mut e = encoder(70);
    let op = OpIAbs {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: gpr_src(5),
    };
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x013);
}

#[test]
fn op_iadd3_overflow_preds() {
    let mut e = encoder(70);
    let op = OpIAdd3 {
        dsts: [
            Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
            Dst::Reg(RegRef::new(RegFile::Pred, 2, 1)),
            Dst::Reg(RegRef::new(RegFile::Pred, 3, 1)),
        ],
        srcs: [gpr_src(4), gpr_src(5), gpr_src(6)],
    };
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x010);
    assert_eq!(e.get_field(81..84), 2);
    assert_eq!(e.get_field(84..87), 3);
}

#[test]
fn op_iadd3x_sets_x_bit() {
    let mut e = encoder(70);
    let op = OpIAdd3X {
        dsts: [
            Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
            Dst::None,
            Dst::None,
        ],
        srcs: [gpr_src(4), gpr_src(5), gpr_src(6), pred_src(0), pred_src(1)],
    };
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x010);
    assert!(e.get_bit(74), ".X");
}

#[test]
fn op_idp4_src_types() {
    let mut e = encoder(70);
    let op = OpIDp4 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src_types: [IntType::U8, IntType::I8],
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
    };
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x026);
    assert!(!e.get_bit(73), "src0 unsigned byte");
    assert!(e.get_bit(74), "src1 signed byte");
}

#[test]
fn op_imad_and_imad64_signed() {
    let mut e = encoder(70);
    let op = OpIMad {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        signed: true,
    };
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x024);
    assert!(e.get_bit(73), "imad signed");

    let mut e = encoder(70);
    let op = OpIMad64 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        signed: false,
    };
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x025);
    assert!(!e.get_bit(73), "imad64 unsigned");
}

#[test]
fn op_imnmx_sm70_and_sm120() {
    let op = OpIMnMx {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        cmp_type: IntCmpType::I32,
        srcs: [gpr_src(2), gpr_src(3), Src::new_imm_bool(true)],
    };
    let mut e70 = encoder(70);
    op.encode(&mut e70);
    assert_eq!(opcode(&e70), 0x017);
    assert!(e70.get_bit(73), "signed min/max");

    let mut e120 = encoder(120);
    op.encode(&mut e120);
    assert!(!e120.get_bit(74), "64-bit clear on sm120");
}

#[test]
fn op_isetp_cmp_ops_and_ex() {
    let mut e = encoder(70);
    let op = OpISetP {
        dst: Dst::Reg(RegRef::new(RegFile::Pred, 0, 1)),
        set_op: PredSetOp::Xor,
        cmp_op: IntCmpOp::Lt,
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
    assert_eq!(opcode(&e), 0x00c);
    assert_eq!(e.get_field(76..79), 1, "lt");
    assert!(!e.get_bit(72), "ex off");

    let mut e = encoder(70);
    let op = OpISetP {
        dst: Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
        set_op: PredSetOp::And,
        cmp_op: IntCmpOp::Eq,
        cmp_type: IntCmpType::I32,
        ex: true,
        srcs: [
            gpr_src(2),
            gpr_src(3),
            Src::new_imm_bool(false),
            pred_src(2),
        ],
    };
    op.encode(&mut e);
    assert!(e.get_bit(72), "ex on");
}

#[test]
fn op_lea_low_and_high() {
    let mut e = encoder(70);
    let op = OpLea {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        srcs: [gpr_src(2), gpr_src(3), Src::ZERO],
        shift: 3,
        dst_high: false,
        intermediate_mod: SrcMod::None,
    };
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x011);
    assert_eq!(e.get_field(75..80), 3, "shift");
    assert!(!e.get_bit(80), "dst_high");

    let mut e = encoder(70);
    let op = OpLea {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        shift: 4,
        dst_high: true,
        intermediate_mod: SrcMod::INeg,
    };
    op.encode(&mut e);
    assert!(e.get_bit(72), "ineg on shifted temp");
    assert!(e.get_bit(80), "dst_high");
}

#[test]
fn op_leax_carry() {
    let mut e = encoder(70);
    let op = OpLeaX {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4), pred_src(1)],
        shift: 2,
        dst_high: true,
        intermediate_mod: SrcMod::BNot,
    };
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x011);
    assert!(e.get_bit(74), ".X lea");
}

#[test]
fn op_lop3_lut() {
    let mut e = encoder(70);
    let op = OpLop3 {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        op: LogicOp3::new_lut(&|x, y, z| x ^ y ^ z),
        srcs: [gpr_src(1), gpr_src(2), gpr_src(3)],
    };
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x012);
    assert_eq!(e.get_field(81..84), 7, "pred mask");
}

#[test]
fn op_popc_bnot() {
    let mut e = encoder(70);
    let op = OpPopC {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: gpr_src(6).bnot(),
    };
    op.encode(&mut e);
    assert_eq!(opcode(&e), 0x109);
    assert_eq!(e.get_field(63..64), 1, "bnot");
}

#[test]
fn op_shf_int_types_and_direction() {
    for (data_type, enc) in [
        (IntType::I64, 0_u64),
        (IntType::U64, 1),
        (IntType::I32, 2),
        (IntType::U32, 3),
    ] {
        let mut e = encoder(70);
        let op = OpShf {
            dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
            srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
            right: true,
            wrap: true,
            data_type,
            dst_high: false,
        };
        op.encode(&mut e);
        assert_eq!(opcode(&e), 0x019);
        assert_eq!(e.get_field(73..75), enc);
    }
}
