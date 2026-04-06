// SPDX-License-Identifier: AGPL-3.0-or-later

//! Encoder smoke tests for float ALU ops on SM32.

use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    Dst, FRndMode, FSwzAddOp, FSwzShuffle, FloatCmpOp, OpFAdd, OpFFma, OpFMnMx, OpFMul, OpFSetP,
    OpFSwz, PredSetOp, RegFile, RegRef, Src, SrcMod, SrcRef, SrcSwizzle, TexDerivMode,
};
use crate::codegen::nv::sm32::encoder::{SM32Encoder, SM32Op, ShaderModel32};

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

#[test]
fn op_fadd_fmul_ffma_fmnmx_encode() {
    let mut e = encoder_sm32();
    OpFAdd {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_f32(2), gpr_f32(3)],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }
    .encode(&mut e);
    assert_ne!(e.inst, [0, 0]);

    let mut e = encoder_sm32();
    OpFMul {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 4, 1)),
        srcs: [gpr_f32(1), gpr_f32(2)],
        saturate: false,
        rnd_mode: FRndMode::NegInf,
        ftz: false,
        dnz: false,
    }
    .encode(&mut e);
    assert_ne!(e.inst, [0, 0]);

    let mut e = encoder_sm32();
    OpFFma {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 5, 1)),
        srcs: [gpr_f32(1), gpr_f32(2), gpr_f32(3)],
        saturate: false,
        rnd_mode: FRndMode::PosInf,
        ftz: false,
        dnz: true,
    }
    .encode(&mut e);
    assert_ne!(e.inst, [0, 0]);

    let mut e = encoder_sm32();
    OpFMnMx {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 6, 1)),
        srcs: [gpr_f32(1), gpr_f32(2), pred_src(0)],
        ftz: true,
    }
    .encode(&mut e);
    assert_ne!(e.inst, [0, 0]);
}

#[test]
fn op_fsetp_encode() {
    let mut e = encoder_sm32();
    OpFSetP {
        dst: Dst::Reg(RegRef::new(RegFile::Pred, 2, 1)),
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdEq,
        srcs: [gpr_f32(1), gpr_f32(2), Src::new_imm_bool(true)],
        ftz: true,
    }
    .encode(&mut e);
    assert_ne!(e.inst, [0, 0]);
}

#[test]
fn op_fswz_encode() {
    let mut e = encoder_sm32();
    OpFSwz {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 8, 1)),
        srcs: [gpr_f32(1), gpr_f32(2)],
        rnd_mode: FRndMode::Zero,
        ftz: false,
        deriv_mode: TexDerivMode::NonDivergent,
        shuffle: FSwzShuffle::SwapHorizontal,
        ops: [
            FSwzAddOp::Add,
            FSwzAddOp::SubRight,
            FSwzAddOp::SubLeft,
            FSwzAddOp::MoveLeft,
        ],
    }
    .encode(&mut e);
    assert_ne!(e.inst, [0, 0]);
}
