// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    CBuf, CBufRef, Dst, FRndMode, FloatCmpOp, OpDAdd, OpDFma, OpDMnMx, OpDMul, OpDSetP, PredSetOp,
    RegFile, RegRef, Src, SrcMod, SrcRef, SrcSwizzle,
};
use crate::codegen::nv::sm50::encoder::{SM50Encoder, SM50Op, ShaderModel50};

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
    // `set_src_imm_f20` requires the low 12 bits zero; use a nontrivial upper pattern.
    0x3ff0_0000
}

fn cb_src(cb: CBufRef, modifier: SrcMod) -> Src {
    Src {
        reference: SrcRef::CBuf(cb),
        modifier,
        swizzle: SrcSwizzle::None,
    }
}

fn encoder_sm50() -> SM50Encoder<'static> {
    let sm: &'static ShaderModel50 = Box::leak(Box::new(ShaderModel50::new(50)));
    let labels: &'static FxHashMap<crate::codegen::ir::Label, usize> =
        Box::leak(Box::new(FxHashMap::default()));
    SM50Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 2],
        sched: 0,
    }
}

/// Bits `52..64` hold the stable upper opcode nibble after `set_opcode`; bits `48..52` are often
/// reused by abs/neg/cmp/fneg fields on double and predicate ops.
fn sm50_opcode_hi12(e: &SM50Encoder<'_>) -> u64 {
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
        let mut e = encoder_sm50();
        op.encode(&mut e);
        assert_eq!(sm50_opcode_hi12(&e), 0x5c7, "dadd reg/reg opcode (hi bits)");
        assert_eq!(e.get_field(39..41), enc, "rounding field");
    }
}

#[test]
fn op_dadd_src1_imm_and_cbuf() {
    let op_imm = OpDAdd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 4, 2)),
        srcs: [gpr_src(1), Src::new_imm_u32(f64_imm_f20())],
        rnd_mode: FRndMode::NearestEven,
    };
    let mut e = encoder_sm50();
    op_imm.encode(&mut e);
    assert_eq!(sm50_opcode_hi12(&e), 0x387);

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
    let mut e = encoder_sm50();
    op_cb.encode(&mut e);
    assert_eq!(sm50_opcode_hi12(&e), 0x4c7);
}

#[test]
fn op_dmul_reg_imm_fneg_and_rounding() {
    let op = OpDMul {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 8, 2)),
        srcs: [gpr_src_fneg(1), gpr_src(2)],
        rnd_mode: FRndMode::PosInf,
    };
    let mut e = encoder_sm50();
    op.encode(&mut e);
    assert_eq!(sm50_opcode_hi12(&e), 0x5c8);
    assert_eq!(e.get_field(39..41), 2, "rounding");
    assert!(e.get_bit(48), "shared fneg bit");
}

#[test]
fn op_dfma_reg_reg_reg_and_fneg_bits() {
    let op = OpDFma {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, 2)),
        srcs: [gpr_src_fneg(1), gpr_src(2), gpr_src(3)],
        rnd_mode: FRndMode::Zero,
    };
    let mut e = encoder_sm50();
    op.encode(&mut e);
    assert_eq!(sm50_opcode_hi12(&e), 0x5b7);
    assert_eq!(e.get_field(50..52), 3, "dfma rounding");
    assert!(e.get_bit(48), "fneg fmul (src0 XOR src1)");
    assert!(!e.get_bit(49), "fneg src2");
}

#[test]
fn op_dfma_src1_imm_and_src2_reg() {
    let op = OpDFma {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, 2)),
        srcs: [gpr_src(1), Src::new_imm_u32(f64_imm_f20()), gpr_src(3)],
        rnd_mode: FRndMode::NearestEven,
    };
    let mut e = encoder_sm50();
    op.encode(&mut e);
    assert_eq!(sm50_opcode_hi12(&e), 0x367);
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
    let mut e = encoder_sm50();
    op.encode(&mut e);
    assert_eq!(sm50_opcode_hi12(&e), 0x4b7);
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
    let mut e = encoder_sm50();
    op.encode(&mut e);
    assert_eq!(sm50_opcode_hi12(&e), 0x537);
}

#[test]
fn op_dmnmx_reg_imm_cbuf() {
    let op_reg = OpDMnMx {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 2)),
        srcs: [gpr_src(1), gpr_src(2), Src::new_imm_bool(true)],
    };
    let mut e = encoder_sm50();
    op_reg.encode(&mut e);
    assert_eq!(sm50_opcode_hi12(&e), 0x5c5);

    let op_imm = OpDMnMx {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 2)),
        srcs: [
            gpr_src(1),
            Src::new_imm_u32(f64_imm_f20()),
            Src::new_imm_bool(false),
        ],
    };
    let mut e = encoder_sm50();
    op_imm.encode(&mut e);
    assert_eq!(sm50_opcode_hi12(&e), 0x385);

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
    let mut e = encoder_sm50();
    op_cb.encode(&mut e);
    assert_eq!(sm50_opcode_hi12(&e), 0x4c5);
    assert!(e.get_bit(49), "cbuf fabs");
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
        let mut e = encoder_sm50();
        op.encode(&mut e);
        assert_eq!(sm50_opcode_hi12(&e), 0x5b8);
        assert_eq!(e.get_field(48..52), expected_cmp, "float cmp encoding");
        assert_eq!(e.get_field(45..47), 0, "PredSetOp::And");
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
    let mut e = encoder_sm50();
    op_or.encode(&mut e);
    assert_eq!(e.get_field(45..47), 1, "PredSetOp::Or");
    assert_eq!(e.get_field(48..52), 0x0a, "UnordEq");
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
    let mut e = encoder_sm50();
    op.encode(&mut e);
    assert_eq!(sm50_opcode_hi12(&e), 0x368);
}
