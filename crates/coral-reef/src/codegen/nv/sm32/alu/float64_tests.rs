// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    CBuf, CBufRef, Dst, FRndMode, FloatCmpOp, OpDAdd, OpDFma, OpDMnMx, OpDMul, OpDSetP, PredSetOp,
    RegFile, RegRef, Src, SrcMod, SrcRef, SrcSwizzle,
};
use crate::codegen::nv::sm32::encoder::{SM32Encoder, SM32Op, ShaderModel32};

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: SrcRef::Reg(RegRef::new(RegFile::GPR, idx, 2)),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn gpr_src_fneg(idx: u32) -> Src {
    Src {
        reference: SrcRef::Reg(RegRef::new(RegFile::GPR, idx, 2)),
        modifier: SrcMod::FNeg,
        swizzle: SrcSwizzle::None,
    }
}

fn f64_imm_f20() -> u32 {
    0x3ff0_0000
}

fn cb_src(cb: CBufRef, modifier: SrcMod) -> Src {
    Src {
        reference: SrcRef::CBuf(cb),
        modifier,
        swizzle: SrcSwizzle::None,
    }
}

fn encoder_sm32() -> SM32Encoder<'static> {
    let sm: &'static ShaderModel32 = Box::leak(Box::new(ShaderModel32::new(32)));
    let labels: &'static FxHashMap<crate::codegen::ir::Label, usize> =
        Box::leak(Box::new(FxHashMap::default()));
    SM32Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 2],
    }
}

/// Full opcode nibble field after `encode_form_immreg` (includes `62..64` form selector bits).
fn sm32_opcode_form(e: &SM32Encoder<'_>) -> u64 {
    e.get_field(52..64)
}

#[test]
fn op_dadd_reg_reg_rounding_modes() {
    for (rnd, enc) in [
        (FRndMode::NearestEven, 0_u64),
        (FRndMode::NegInf, 1),
        (FRndMode::PosInf, 2),
        (FRndMode::Zero, 3),
    ] {
        let op = OpDAdd {
            dst: Dst::Reg(RegRef::new(RegFile::GPR, 4, 2)),
            srcs: [gpr_src(1), gpr_src(2)],
            rnd_mode: rnd,
        };
        let mut e = encoder_sm32();
        op.encode(&mut e);
        assert_eq!(sm32_opcode_form(&e), 0xe38, "dadd reg/reg opcode + form");
        assert_eq!(e.get_field(42..44), enc, "rounding field");
    }
}

#[test]
fn op_dadd_src1_imm_and_cbuf() {
    let op_imm = OpDAdd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 4, 2)),
        srcs: [gpr_src(1), Src::new_imm_u32(f64_imm_f20())],
        rnd_mode: FRndMode::NearestEven,
    };
    let mut e = encoder_sm32();
    op_imm.encode(&mut e);
    assert_eq!(sm32_opcode_form(&e), 0xc38);

    let op_cb = OpDAdd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 4, 2)),
        srcs: [
            gpr_src(1),
            cb_src(
                CBufRef {
                    buf: CBuf::Binding(2),
                    offset: 0x40,
                },
                SrcMod::None,
            ),
        ],
        rnd_mode: FRndMode::NearestEven,
    };
    let mut e = encoder_sm32();
    op_cb.encode(&mut e);
    assert_eq!(sm32_opcode_form(&e), 0x638);
}

#[test]
fn op_dmul_reg_imm_fneg_and_rounding() {
    let op = OpDMul {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 8, 2)),
        srcs: [gpr_src_fneg(1), gpr_src(2)],
        rnd_mode: FRndMode::PosInf,
    };
    let mut e = encoder_sm32();
    op.encode(&mut e);
    assert_eq!(sm32_opcode_form(&e), 0xe40);
    assert_eq!(e.get_field(42..44), 2, "rounding");
    assert!(e.get_bit(51), "shared fneg bit");
}

#[test]
fn op_dfma_reg_reg_reg_and_fneg_bits() {
    let op = OpDFma {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, 2)),
        srcs: [gpr_src_fneg(1), gpr_src(2), gpr_src(3)],
        rnd_mode: FRndMode::Zero,
    };
    let mut e = encoder_sm32();
    op.encode(&mut e);
    assert_eq!(sm32_opcode_form(&e), 0xdbe);
    assert_eq!(e.get_field(53..55), 3, "dfma rounding");
    assert!(e.get_bit(51), "fneg fmul (src0 XOR src1)");
    assert!(!e.get_bit(52), "fneg src2");
}

#[test]
fn op_dfma_src1_imm_and_src2_reg() {
    let op = OpDFma {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, 2)),
        srcs: [gpr_src(1), Src::new_imm_u32(f64_imm_f20()), gpr_src(3)],
        rnd_mode: FRndMode::NearestEven,
    };
    let mut e = encoder_sm32();
    op.encode(&mut e);
    assert_eq!(sm32_opcode_form(&e), 0xb38);
}

#[test]
fn op_dfma_src1_cbuf() {
    let op = OpDFma {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, 2)),
        srcs: [
            gpr_src(1),
            cb_src(
                CBufRef {
                    buf: CBuf::Binding(1),
                    offset: 0,
                },
                SrcMod::None,
            ),
            gpr_src(3),
        ],
        rnd_mode: FRndMode::NearestEven,
    };
    let mut e = encoder_sm32();
    op.encode(&mut e);
    assert_eq!(sm32_opcode_form(&e), 0x5b8);
}

#[test]
fn op_dfma_src2_cbuf() {
    let op = OpDFma {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, 2)),
        srcs: [
            gpr_src(1),
            gpr_src(2),
            cb_src(
                CBufRef {
                    buf: CBuf::Binding(3),
                    offset: 0x20,
                },
                SrcMod::None,
            ),
        ],
        rnd_mode: FRndMode::NearestEven,
    };
    let mut e = encoder_sm32();
    op.encode(&mut e);
    assert_eq!(sm32_opcode_form(&e), 0x9b8);
}

#[test]
fn op_dmnmx_reg_imm_cbuf() {
    let op_reg = OpDMnMx {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 2)),
        srcs: [gpr_src(1), gpr_src(2), Src::new_imm_bool(true)],
    };
    let mut e = encoder_sm32();
    op_reg.encode(&mut e);
    assert_eq!(sm32_opcode_form(&e), 0xe28);

    let op_imm = OpDMnMx {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 2)),
        srcs: [
            gpr_src(1),
            Src::new_imm_u32(f64_imm_f20()),
            Src::new_imm_bool(false),
        ],
    };
    let mut e = encoder_sm32();
    op_imm.encode(&mut e);
    assert_eq!(sm32_opcode_form(&e), 0xc28);

    let op_cb = OpDMnMx {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 2)),
        srcs: [
            gpr_src(1),
            cb_src(
                CBufRef {
                    buf: CBuf::Binding(0),
                    offset: 0x10,
                },
                SrcMod::FAbs,
            ),
            Src::new_imm_bool(true),
        ],
    };
    let mut e = encoder_sm32();
    op_cb.encode(&mut e);
    assert_eq!(sm32_opcode_form(&e), 0x629);
    assert!(e.get_bit(52), "cbuf fabs on src1");
}

#[test]
fn op_dsetp_float_cmp_ops_and_set_op() {
    let base = |cmp: FloatCmpOp, expected_cmp: u64| {
        let op = OpDSetP {
            dst: Dst::Reg(RegRef::new(RegFile::Pred, 2, 1)),
            set_op: PredSetOp::And,
            cmp_op: cmp,
            srcs: [gpr_src(1), gpr_src(2), Src::new_imm_bool(false)],
        };
        let mut e = encoder_sm32();
        op.encode(&mut e);
        assert_eq!(e.get_field(51..55), expected_cmp, "float cmp encoding");
        assert_eq!(e.get_field(48..50), 0, "PredSetOp::And");
    };

    base(FloatCmpOp::OrdEq, 0x02);
    base(FloatCmpOp::OrdNe, 0x05);
    base(FloatCmpOp::OrdLt, 0x01);
    base(FloatCmpOp::OrdLe, 0x03);
    base(FloatCmpOp::OrdGt, 0x04);
    base(FloatCmpOp::OrdGe, 0x06);

    let op_or = OpDSetP {
        dst: Dst::Reg(RegRef::new(RegFile::Pred, 0, 1)),
        set_op: PredSetOp::Or,
        cmp_op: FloatCmpOp::UnordEq,
        srcs: [gpr_src(3), gpr_src(4), Src::new_imm_bool(true)],
    };
    let mut e = encoder_sm32();
    op_or.encode(&mut e);
    assert_eq!(e.get_field(48..50), 1, "PredSetOp::Or");
    assert_eq!(e.get_field(51..55), 0x0a, "UnordEq");
}

#[test]
fn op_dsetp_src1_imm() {
    let op = OpDSetP {
        dst: Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdEq,
        srcs: [
            gpr_src(5),
            Src::new_imm_u32(f64_imm_f20()),
            Src::new_imm_bool(false),
        ],
    };
    let mut e = encoder_sm32();
    op.encode(&mut e);
    assert_eq!(sm32_opcode_form(&e), 0xb41);
}
