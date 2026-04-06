// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::const_tracker::ConstTracker;
use crate::codegen::ir::{
    CBuf, CBufRef, Dst, FRndMode, FloatCmpOp, OpDAdd, OpDFma, OpDMnMx, OpDMul, OpDSetP, PredSetOp,
    RegFile, RegRef, SSAValueAllocator, Src, SrcMod, SrcRef, SrcSwizzle,
};
use crate::codegen::legalize::LegalizeBuilder;
use crate::codegen::nv::sm20::encoder::{SM20Encoder, SM20Op, SM20Unit, ShaderModel20};

fn gpr_src_f64(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 2).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn gpr_src_f64_fabs(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 2).into(),
        modifier: SrcMod::FAbs,
        swizzle: SrcSwizzle::None,
    }
}

fn gpr_src_f64_fneg(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 2).into(),
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

fn sm20_encoder() -> SM20Encoder<'static> {
    let sm: &'static ShaderModel20 = Box::leak(Box::new(ShaderModel20::new(20)));
    let labels: &'static FxHashMap<crate::codegen::ir::Label, usize> =
        Box::leak(Box::new(FxHashMap::default()));
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
fn op_dadd_encode_rounding_modes_and_fabs_fneg_bits() {
    for (rnd, enc) in [
        (FRndMode::NearestEven, 0_u64),
        (FRndMode::NegInf, 1),
        (FRndMode::PosInf, 2),
        (FRndMode::Zero, 3),
    ] {
        let op = OpDAdd {
            dst: Dst::Reg(RegRef::new(RegFile::GPR, 4, 2)),
            srcs: [gpr_src_f64(1), gpr_src_f64(2)],
            rnd_mode: rnd,
        };
        let mut e = sm20_encoder();
        op.encode(&mut e);
        assert_eq!(unit(&e), SM20Unit::Double as u64);
        assert_eq!(opcode_byte(&e), 0x12);
        assert_eq!(e.get_field(55..57), enc, "rounding field");
    }

    let op = OpDAdd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, 2)),
        srcs: [gpr_src_f64(1), gpr_src_f64_fabs(2)],
        rnd_mode: FRndMode::NearestEven,
    };
    let mut e = sm20_encoder();
    op.encode(&mut e);
    assert!(e.get_bit(6), "src1 fabs");
    let op = OpDAdd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, 2)),
        srcs: [gpr_src_f64_fabs(1), gpr_src_f64(2)],
        rnd_mode: FRndMode::NearestEven,
    };
    let mut e = sm20_encoder();
    op.encode(&mut e);
    assert!(e.get_bit(7), "src0 fabs");
    let op = OpDAdd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, 2)),
        srcs: [gpr_src_f64_fneg(1), gpr_src_f64(2)],
        rnd_mode: FRndMode::NearestEven,
    };
    let mut e = sm20_encoder();
    op.encode(&mut e);
    assert!(e.get_bit(9), "src0 fneg");
}

#[test]
fn op_dadd_encode_imm_and_cbuf_src1() {
    let op_imm = OpDAdd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 4, 2)),
        srcs: [gpr_src_f64(1), Src::new_imm_u32(f64_imm_f20())],
        rnd_mode: FRndMode::NearestEven,
    };
    let mut e = sm20_encoder();
    op_imm.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Double as u64);
    assert_eq!(opcode_byte(&e), 0x12);

    let op_cb = OpDAdd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 4, 2)),
        srcs: [
            gpr_src_f64(1),
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
    let mut e = sm20_encoder();
    op_cb.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Double as u64);
    assert_eq!(opcode_byte(&e), 0x12);
}

#[test]
fn op_dmul_encode_fneg_xor_and_rounding() {
    let op = OpDMul {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 8, 2)),
        srcs: [gpr_src_f64_fneg(1), gpr_src_f64(2)],
        rnd_mode: FRndMode::PosInf,
    };
    let mut e = sm20_encoder();
    op.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Double as u64);
    assert_eq!(opcode_byte(&e), 0x14);
    assert_eq!(e.get_field(55..57), 2, "rounding");
    assert!(e.get_bit(9), "shared fneg bit");
}

#[test]
fn op_dfma_encode_reg_reg_reg_fneg_bits() {
    let op = OpDFma {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, 2)),
        srcs: [gpr_src_f64_fneg(1), gpr_src_f64(2), gpr_src_f64(3)],
        rnd_mode: FRndMode::Zero,
    };
    let mut e = sm20_encoder();
    op.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Double as u64);
    assert_eq!(opcode_byte(&e), 0x08);
    assert_eq!(e.get_field(55..57), 3, "dfma rounding");
    assert!(e.get_bit(9), "fneg fmul (src0 XOR src1)");
    assert!(!e.get_bit(8), "fneg src2");
}

#[test]
fn op_dfma_encode_src1_imm_and_cbuf() {
    let op_imm = OpDFma {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, 2)),
        srcs: [
            gpr_src_f64(1),
            Src::new_imm_u32(f64_imm_f20()),
            gpr_src_f64(3),
        ],
        rnd_mode: FRndMode::NearestEven,
    };
    let mut e = sm20_encoder();
    op_imm.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Double as u64);
    assert_eq!(opcode_byte(&e), 0x08);

    let op_cb = OpDFma {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, 2)),
        srcs: [
            gpr_src_f64(1),
            cb_src(
                CBufRef {
                    buf: CBuf::Binding(1),
                    offset: 0,
                },
                SrcMod::None,
            ),
            gpr_src_f64(3),
        ],
        rnd_mode: FRndMode::NearestEven,
    };
    let mut e = sm20_encoder();
    op_cb.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Double as u64);
    assert_eq!(opcode_byte(&e), 0x08);
}

#[test]
fn op_dmnmx_encode_reg_imm_and_pred_min_src() {
    let op_reg = OpDMnMx {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 2)),
        srcs: [gpr_src_f64(1), gpr_src_f64(2), Src::new_imm_bool(true)],
    };
    let mut e = sm20_encoder();
    op_reg.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Double as u64);
    assert_eq!(opcode_byte(&e), 0x02);

    let op_imm = OpDMnMx {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 2)),
        srcs: [
            gpr_src_f64(1),
            Src::new_imm_u32(f64_imm_f20()),
            Src::new_imm_bool(false),
        ],
    };
    let mut e = sm20_encoder();
    op_imm.encode(&mut e);
    assert_eq!(opcode_byte(&e), 0x02);

    let op_cb = OpDMnMx {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 2)),
        srcs: [
            gpr_src_f64(1),
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
    let mut e = sm20_encoder();
    op_cb.encode(&mut e);
    assert!(e.get_bit(6), "cbuf fabs on src1");
}

#[test]
fn op_dsetp_encode_cmp_ops_set_op_and_imm_src1() {
    let base = |cmp: FloatCmpOp, expected_cmp: u64| {
        let op = OpDSetP {
            dst: Dst::Reg(RegRef::new(RegFile::Pred, 2, 1)),
            set_op: PredSetOp::And,
            cmp_op: cmp,
            srcs: [gpr_src_f64(1), gpr_src_f64(2), Src::new_imm_bool(false)],
        };
        let mut e = sm20_encoder();
        op.encode(&mut e);
        assert_eq!(e.get_field(55..59), expected_cmp, "float cmp encoding");
        assert_eq!(e.get_field(53..55), 0, "PredSetOp::And");
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
        srcs: [gpr_src_f64(3), gpr_src_f64(4), Src::new_imm_bool(true)],
    };
    let mut e = sm20_encoder();
    op_or.encode(&mut e);
    assert_eq!(e.get_field(53..55), 1, "PredSetOp::Or");
    assert_eq!(e.get_field(55..59), 0x0a, "UnordEq");

    let op_imm = OpDSetP {
        dst: Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdEq,
        srcs: [
            gpr_src_f64(5),
            Src::new_imm_u32(f64_imm_f20()),
            Src::new_imm_bool(false),
        ],
    };
    let mut e = sm20_encoder();
    op_imm.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Double as u64);
    assert_eq!(opcode_byte(&e), 0x06);
}

fn sm20_for_legalize() -> &'static ShaderModel20 {
    Box::leak(Box::new(ShaderModel20::new(20)))
}

fn f64_ssa_src(alloc: &mut SSAValueAllocator) -> Src {
    let r = alloc.alloc_vec(RegFile::GPR, 2);
    Src {
        reference: SrcRef::SSA(r),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn pred_ssa_src(alloc: &mut SSAValueAllocator) -> Src {
    let v = alloc.alloc(RegFile::Pred);
    Src {
        reference: SrcRef::SSA(v.into()),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

#[test]
fn op_dadd_legalize_runs_for_ssa_sources() {
    let sm = sm20_for_legalize();
    let mut alloc = SSAValueAllocator::new();
    let mut ct = ConstTracker::new();
    let dst = alloc.alloc_vec(RegFile::GPR, 2);
    let s0 = f64_ssa_src(&mut alloc);
    let s1 = f64_ssa_src(&mut alloc);
    let mut b = LegalizeBuilder::new_for_test(sm, &mut alloc, &mut ct);
    let mut op = OpDAdd {
        dst: Dst::SSA(dst),
        srcs: [s0, s1],
        rnd_mode: FRndMode::NearestEven,
    };
    op.legalize(&mut b);
}

#[test]
fn op_dmul_legalize_runs_for_ssa_sources() {
    let sm = sm20_for_legalize();
    let mut alloc = SSAValueAllocator::new();
    let mut ct = ConstTracker::new();
    let dst = alloc.alloc_vec(RegFile::GPR, 2);
    let s0 = f64_ssa_src(&mut alloc);
    let s1 = f64_ssa_src(&mut alloc);
    let mut b = LegalizeBuilder::new_for_test(sm, &mut alloc, &mut ct);
    let mut op = OpDMul {
        dst: Dst::SSA(dst),
        srcs: [s0, s1],
        rnd_mode: FRndMode::NearestEven,
    };
    op.legalize(&mut b);
}

#[test]
fn op_dmnmx_legalize_runs_for_ssa_sources() {
    let sm = sm20_for_legalize();
    let mut alloc = SSAValueAllocator::new();
    let mut ct = ConstTracker::new();
    let dst = alloc.alloc_vec(RegFile::GPR, 2);
    let s0 = f64_ssa_src(&mut alloc);
    let s1 = f64_ssa_src(&mut alloc);
    let mut b = LegalizeBuilder::new_for_test(sm, &mut alloc, &mut ct);
    let mut op = OpDMnMx {
        dst: Dst::SSA(dst),
        srcs: [s0, s1, Src::new_imm_bool(true)],
    };
    op.legalize(&mut b);
}

#[test]
fn op_dfma_legalize_all_gpr_ssa() {
    let sm = sm20_for_legalize();
    let mut alloc = SSAValueAllocator::new();
    let mut ct = ConstTracker::new();
    let dst = alloc.alloc_vec(RegFile::GPR, 2);
    let s0 = f64_ssa_src(&mut alloc);
    let s1 = f64_ssa_src(&mut alloc);
    let s2 = f64_ssa_src(&mut alloc);
    let mut b = LegalizeBuilder::new_for_test(sm, &mut alloc, &mut ct);
    let mut op = OpDFma {
        dst: Dst::SSA(dst),
        srcs: [s0, s1, s2],
        rnd_mode: FRndMode::NearestEven,
    };
    op.legalize(&mut b);
}

#[test]
fn op_dfma_legalize_imm_src1_branch() {
    let sm = sm20_for_legalize();
    let mut alloc = SSAValueAllocator::new();
    let mut ct = ConstTracker::new();
    let dst = alloc.alloc_vec(RegFile::GPR, 2);
    let s0 = f64_ssa_src(&mut alloc);
    let s2 = f64_ssa_src(&mut alloc);
    let mut b = LegalizeBuilder::new_for_test(sm, &mut alloc, &mut ct);
    let mut op = OpDFma {
        dst: Dst::SSA(dst),
        srcs: [s0, Src::new_imm_u32(f64_imm_f20()), s2],
        rnd_mode: FRndMode::NearestEven,
    };
    op.legalize(&mut b);
}

#[test]
fn op_dfma_legalize_fabs_src0() {
    let sm = sm20_for_legalize();
    let mut alloc = SSAValueAllocator::new();
    let mut ct = ConstTracker::new();
    let dst = alloc.alloc_vec(RegFile::GPR, 2);
    let r0 = alloc.alloc_vec(RegFile::GPR, 2);
    let s0 = Src {
        reference: SrcRef::SSA(r0),
        modifier: SrcMod::FAbs,
        swizzle: SrcSwizzle::None,
    };
    let s1 = f64_ssa_src(&mut alloc);
    let s2 = f64_ssa_src(&mut alloc);
    let mut b = LegalizeBuilder::new_for_test(sm, &mut alloc, &mut ct);
    let mut op = OpDFma {
        dst: Dst::SSA(dst),
        srcs: [s0, s1, s2],
        rnd_mode: FRndMode::NearestEven,
    };
    op.legalize(&mut b);
}

#[test]
fn op_dsetp_legalize_runs_for_ssa_sources() {
    let sm = sm20_for_legalize();
    let mut alloc = SSAValueAllocator::new();
    let mut ct = ConstTracker::new();
    let dst = alloc.alloc(RegFile::Pred).into();
    let s0 = f64_ssa_src(&mut alloc);
    let s1 = f64_ssa_src(&mut alloc);
    let s2 = pred_ssa_src(&mut alloc);
    let mut b = LegalizeBuilder::new_for_test(sm, &mut alloc, &mut ct);
    let mut op = OpDSetP {
        dst: Dst::SSA(dst),
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdEq,
        srcs: [s0, s1, s2],
    };
    op.legalize(&mut b);
}
