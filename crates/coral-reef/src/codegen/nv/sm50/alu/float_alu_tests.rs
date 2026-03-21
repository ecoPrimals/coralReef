// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals

//! Encoder tests for float ALU ops: opcode and key operand bits.

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    CBuf, CBufRef, Dst, FRndMode, FSwzAddOp, FloatCmpOp, OpFAdd, OpFFma, OpFMnMx, OpFMul, OpFSetP,
    OpFSwzAdd, PredSetOp, RegFile, RegRef, Src, SrcMod, SrcRef, SrcSwizzle, TexDerivMode,
};
use crate::codegen::nv::sm50::encoder::{SM50Encoder, SM50Op, ShaderModel50};

fn gpr_f32(idx: u32) -> Src {
    Src {
        reference: SrcRef::Reg(RegRef::new(RegFile::GPR, idx, 1)),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn pred_src(idx: u32) -> Src {
    Src {
        reference: SrcRef::Reg(RegRef::new(RegFile::Pred, idx, 1)),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn cb() -> Src {
    Src {
        reference: SrcRef::CBuf(CBufRef {
            buf: CBuf::Binding(2),
            offset: 0x30,
        }),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn opcode_hi(e: &SM50Encoder<'_>) -> u64 {
    e.get_field(48..64)
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

#[test]
fn op_fadd_fmul_ffma_fmnmx_encode() {
    let dst = Dst::Reg(RegRef::new(RegFile::GPR, 1, 1));
    let mut e = encoder_sm50();
    OpFAdd {
        dst,
        srcs: [gpr_f32(2), gpr_f32(3)],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: true,
    }
    .encode(&mut e);
    assert_ne!(e.inst, [0, 0]);

    let mut e = encoder_sm50();
    OpFMul {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 4, 1)),
        srcs: [gpr_f32(1), gpr_f32(2)],
        saturate: false,
        rnd_mode: FRndMode::PosInf,
        ftz: false,
        dnz: false,
    }
    .encode(&mut e);
    assert_ne!(e.inst, [0, 0]);

    let mut e = encoder_sm50();
    OpFFma {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 5, 1)),
        srcs: [gpr_f32(1), gpr_f32(2), gpr_f32(3)],
        saturate: false,
        rnd_mode: FRndMode::Zero,
        ftz: false,
        dnz: true,
    }
    .encode(&mut e);
    assert_ne!(e.inst, [0, 0]);

    let mut e = encoder_sm50();
    OpFMnMx {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 6, 1)),
        srcs: [gpr_f32(1), cb(), pred_src(0)],
        ftz: false,
    }
    .encode(&mut e);
    assert_ne!(e.inst, [0, 0]);
}

#[test]
fn op_fadd_imm32_wide_ftz_and_f20_cb_reg_mods() {
    let dst = Dst::Reg(RegRef::new(RegFile::GPR, 1, 1));
    let mut e = encoder_sm50();
    OpFAdd {
        dst,
        srcs: [gpr_f32(2), Src::new_imm_u32(0x0000_0f01)],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: true,
    }
    .encode(&mut e);
    assert_eq!(
        opcode_hi(&e),
        0x0880,
        "0x0800 with ftz merged into opcode field"
    );
    assert!(e.get_bit(55), "ftz");

    let mut e = encoder_sm50();
    OpFAdd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 3, 1)),
        srcs: [
            Src {
                reference: gpr_f32(1).reference,
                modifier: SrcMod::FAbs,
                swizzle: SrcSwizzle::None,
            },
            Src::new_imm_u32(0x3f80_0000),
        ],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }
    .encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x3858, "fadd imm f20");

    let mut e = encoder_sm50();
    OpFAdd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 4, 1)),
        srcs: [
            Src {
                reference: gpr_f32(5).reference,
                modifier: SrcMod::FNeg,
                swizzle: SrcSwizzle::None,
            },
            cb(),
        ],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }
    .encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x4c59, "cbuf fadd with fneg on src0");
}

#[test]
fn op_fmul_imm32_fneg_xor_and_reg_cb() {
    let dst = Dst::Reg(RegRef::new(RegFile::GPR, 1, 1));
    let mut e = encoder_sm50();
    OpFMul {
        dst,
        srcs: [
            Src {
                reference: gpr_f32(2).reference,
                modifier: SrcMod::FNeg,
                swizzle: SrcSwizzle::None,
            },
            Src::new_imm_u32(0x4000_0f01),
        ],
        saturate: true,
        rnd_mode: FRndMode::NearestEven,
        ftz: true,
        dnz: true,
    }
    .encode(&mut e);
    assert!(e.get_bit(53), "ftz");
    assert!(e.get_bit(54), "dnz");
    assert_ne!(
        e.get_field(20..52),
        0x4000_0000,
        "shared fneg must flip the immediate sign"
    );

    let mut e = encoder_sm50();
    OpFMul {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 3, 1)),
        srcs: [
            Src {
                reference: gpr_f32(1).reference,
                modifier: SrcMod::FNeg,
                swizzle: SrcSwizzle::None,
            },
            gpr_f32(2),
        ],
        saturate: false,
        rnd_mode: FRndMode::Zero,
        ftz: false,
        dnz: false,
    }
    .encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x5c69, "0x5c68 | fneg");
    assert_eq!(e.get_field(39..41), 3, "rounding zero");

    let mut e = encoder_sm50();
    OpFMul {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 6, 1)),
        srcs: [gpr_f32(1), cb()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }
    .encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x4c68, "cbuf fmul");
}

#[test]
fn op_ffma_src1_src2_combinations() {
    let dst = Dst::Reg(RegRef::new(RegFile::GPR, 1, 1));
    let mut e = encoder_sm50();
    OpFFma {
        dst,
        srcs: [gpr_f32(1), gpr_f32(2), gpr_f32(3)],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }
    .encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x5980);

    let mut e = encoder_sm50();
    OpFFma {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 1)),
        srcs: [gpr_f32(1), Src::new_imm_u32(0x3f00_0000), gpr_f32(4)],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }
    .encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x3280, "src1 imm f20");

    let mut e = encoder_sm50();
    OpFFma {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 3, 1)),
        srcs: [gpr_f32(1), cb(), gpr_f32(4)],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }
    .encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x4980, "src1 cbuf");

    let mut e = encoder_sm50();
    OpFFma {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 5, 1)),
        srcs: [gpr_f32(1), gpr_f32(2), cb()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }
    .encode(&mut e);
    assert_eq!(opcode_hi(&e), 0x5180, "src2 cbuf");
}

#[test]
fn op_fsetp_float_cmp_ops_all() {
    let enc: &[(FloatCmpOp, u64)] = &[
        (FloatCmpOp::OrdLt, 0x01),
        (FloatCmpOp::OrdEq, 0x02),
        (FloatCmpOp::OrdLe, 0x03),
        (FloatCmpOp::OrdGt, 0x04),
        (FloatCmpOp::OrdNe, 0x05),
        (FloatCmpOp::OrdGe, 0x06),
        (FloatCmpOp::IsNum, 0x07),
        (FloatCmpOp::IsNan, 0x08),
        (FloatCmpOp::UnordLt, 0x09),
        (FloatCmpOp::UnordEq, 0x0a),
        (FloatCmpOp::UnordLe, 0x0b),
        (FloatCmpOp::UnordGt, 0x0c),
        (FloatCmpOp::UnordNe, 0x0d),
        (FloatCmpOp::UnordGe, 0x0e),
    ];
    for &(cmp_op, bits) in enc {
        let mut e = encoder_sm50();
        OpFSetP {
            dst: Dst::Reg(RegRef::new(RegFile::Pred, 0, 1)),
            set_op: PredSetOp::And,
            cmp_op,
            srcs: [gpr_f32(1), gpr_f32(2), Src::new_imm_bool(false)],
            ftz: false,
        }
        .encode(&mut e);
        assert_eq!(e.get_field(48..52), bits, "{cmp_op:?}");
    }
}

#[test]
fn op_fmnmx_min_vs_max_pred() {
    let mut e = encoder_sm50();
    OpFMnMx {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_f32(1), gpr_f32(2), Src::new_imm_bool(true)],
        ftz: false,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(39..42), 7, "predicate register slot");
    assert!(!e.get_bit(42), "min: SrcRef::True → not bit clear");

    let mut e = encoder_sm50();
    OpFMnMx {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 1)),
        srcs: [gpr_f32(1), gpr_f32(2), Src::new_imm_bool(false)],
        ftz: true,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(39..42), 7, "predicate register slot");
    assert!(e.get_bit(42), "max: SrcRef::False → not bit set");
    assert!(e.get_bit(44), "ftz");
}

#[test]
fn op_fsetp_src_variants_encode() {
    let fsetp = |src_b: Src, accum: Src| OpFSetP {
        dst: Dst::Reg(RegRef::new(RegFile::Pred, 0, 1)),
        set_op: PredSetOp::Or,
        cmp_op: FloatCmpOp::OrdLt,
        srcs: [gpr_f32(1), src_b, accum],
        ftz: false,
    };

    let mut e = encoder_sm50();
    fsetp(gpr_f32(2), Src::new_imm_bool(false)).encode(&mut e);
    let a = e.inst;

    let mut e = encoder_sm50();
    fsetp(cb(), Src::new_imm_bool(false)).encode(&mut e);
    assert_ne!(a, e.inst);
}

#[test]
fn op_fswzadd_encode() {
    let mut e = encoder_sm50();
    OpFSwzAdd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 9, 1)),
        srcs: [gpr_f32(1), gpr_f32(2)],
        rnd_mode: FRndMode::PosInf,
        ftz: true,
        deriv_mode: TexDerivMode::Auto,
        ops: [
            FSwzAddOp::Add,
            FSwzAddOp::SubLeft,
            FSwzAddOp::SubRight,
            FSwzAddOp::MoveLeft,
        ],
    }
    .encode(&mut e);
    assert_ne!(e.inst, [0, 0]);
}
