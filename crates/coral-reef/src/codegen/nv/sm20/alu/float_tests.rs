// SPDX-License-Identifier: AGPL-3.0-or-later

use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    Dst, FRndMode, FSwzAddOp, FSwzShuffle, FloatCmpOp, Label, OpFAdd, OpFFma, OpFMnMx, OpFMul,
    OpFSet, OpFSetP, OpFSwz, OpRro, OpTranscendental, PredSetOp, RegFile, RegRef, RroOp, Src,
    SrcMod, SrcSwizzle, TexDerivMode, TranscendentalOp,
};

use super::super::super::encoder::{SM20Encoder, SM20Op, SM20Unit, ShaderModel20};

fn gpr(idx: u32) -> RegRef {
    RegRef::new(RegFile::GPR, idx, 1)
}

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: gpr(idx).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn gpr_dst(idx: u32) -> Dst {
    Dst::Reg(gpr(idx))
}

fn pred_dst(idx: u32) -> Dst {
    Dst::Reg(RegRef::new(RegFile::Pred, idx, 1))
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
fn op_fadd_float_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpFAdd {
        dst: gpr_dst(1),
        srcs: [gpr_src(2), gpr_src(3)],
        saturate: false,
        ftz: false,
        rnd_mode: FRndMode::NearestEven,
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Float as u64);
    assert_eq!(opcode_byte(&e), 0x14);
}

#[test]
fn op_fadd_ftz_bit() {
    let mut e = sm20_encoder();
    OpFAdd {
        dst: gpr_dst(1),
        srcs: [gpr_src(2), gpr_src(3)],
        saturate: false,
        ftz: true,
        rnd_mode: FRndMode::NearestEven,
    }
    .encode(&mut e);
    assert!(e.get_bit(5), "FTZ bit should be set");
}

#[test]
fn op_fadd_saturate_bit() {
    let mut e = sm20_encoder();
    OpFAdd {
        dst: gpr_dst(1),
        srcs: [gpr_src(2), gpr_src(3)],
        saturate: true,
        ftz: false,
        rnd_mode: FRndMode::NearestEven,
    }
    .encode(&mut e);
    assert!(e.get_bit(49), "saturate bit should be set");
}

#[test]
fn op_fadd_fneg_modifier_bits() {
    let mut e = sm20_encoder();
    OpFAdd {
        dst: gpr_dst(1),
        srcs: [
            Src {
                reference: gpr(2).into(),
                modifier: SrcMod::FNeg,
                swizzle: SrcSwizzle::None,
            },
            gpr_src(3),
        ],
        saturate: false,
        ftz: false,
        rnd_mode: FRndMode::NearestEven,
    }
    .encode(&mut e);
    assert!(e.get_bit(9), "src0 fneg should be bit 9");
    assert!(!e.get_bit(8), "src1 fneg should be clear");
}

#[test]
fn op_ffma_float_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpFFma {
        dst: gpr_dst(1),
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        saturate: false,
        ftz: false,
        dnz: false,
        rnd_mode: FRndMode::NearestEven,
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Float as u64);
    assert_eq!(opcode_byte(&e), 0xc);
}

#[test]
fn op_ffma_dnz_bit() {
    let mut e = sm20_encoder();
    OpFFma {
        dst: gpr_dst(1),
        srcs: [gpr_src(2), gpr_src(3), gpr_src(4)],
        saturate: false,
        ftz: false,
        dnz: true,
        rnd_mode: FRndMode::NearestEven,
    }
    .encode(&mut e);
    assert!(e.get_bit(7), "DNZ bit should be set");
}

#[test]
fn op_fmnmx_float_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpFMnMx {
        dst: gpr_dst(1),
        srcs: [gpr_src(2), gpr_src(3), true.into()],
        ftz: false,
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Float as u64);
    assert_eq!(opcode_byte(&e), 0x2);
}

#[test]
fn op_fmul_float_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpFMul {
        dst: gpr_dst(1),
        srcs: [gpr_src(2), gpr_src(3)],
        saturate: false,
        ftz: false,
        dnz: false,
        rnd_mode: FRndMode::NearestEven,
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Float as u64);
    assert_eq!(opcode_byte(&e), 0x16);
}

#[test]
fn op_rro_sincos_opcode() {
    let mut e = sm20_encoder();
    OpRro {
        dst: gpr_dst(1),
        op: RroOp::SinCos,
        src: gpr_src(2),
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Float as u64);
    assert_eq!(opcode_byte(&e), 0x18);
    let subop: u64 = e.get_field(5..6);
    assert_eq!(subop, 0, "SinCos subop should be 0");
}

#[test]
fn op_rro_exp2_subop() {
    let mut e = sm20_encoder();
    OpRro {
        dst: gpr_dst(1),
        op: RroOp::Exp2,
        src: gpr_src(2),
    }
    .encode(&mut e);
    let subop: u64 = e.get_field(5..6);
    assert_eq!(subop, 1, "Exp2 subop should be 1");
}

#[test]
fn op_transcendental_cos_subop() {
    let mut e = sm20_encoder();
    OpTranscendental {
        dst: gpr_dst(1),
        op: TranscendentalOp::Cos,
        src: gpr_src(2),
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Float as u64);
    assert_eq!(opcode_byte(&e), 0x32);
    let subop: u64 = e.get_field(26..30);
    assert_eq!(subop, 0, "Cos subop should be 0");
}

#[test]
fn op_transcendental_sin_subop() {
    let mut e = sm20_encoder();
    OpTranscendental {
        dst: gpr_dst(1),
        op: TranscendentalOp::Sin,
        src: gpr_src(2),
    }
    .encode(&mut e);
    let subop: u64 = e.get_field(26..30);
    assert_eq!(subop, 1, "Sin subop should be 1");
}

#[test]
fn op_transcendental_rcp_subop() {
    let mut e = sm20_encoder();
    OpTranscendental {
        dst: gpr_dst(1),
        op: TranscendentalOp::Rcp,
        src: gpr_src(2),
    }
    .encode(&mut e);
    let subop: u64 = e.get_field(26..30);
    assert_eq!(subop, 4, "Rcp subop should be 4");
}

#[test]
fn op_fset_float_unit_and_cmp_field() {
    let mut e = sm20_encoder();
    OpFSet {
        dst: gpr_dst(1),
        srcs: [gpr_src(2), gpr_src(3)],
        cmp_op: FloatCmpOp::OrdLt,
        ftz: false,
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Float as u64);
    let cmp: u64 = e.get_field(55..59);
    assert_eq!(cmp, 0x01, "OrdLt should be 0x01");
}

#[test]
fn op_fset_ftz_field() {
    let mut e = sm20_encoder();
    OpFSet {
        dst: gpr_dst(1),
        srcs: [gpr_src(2), gpr_src(3)],
        cmp_op: FloatCmpOp::OrdLt,
        ftz: true,
    }
    .encode(&mut e);
    assert!(e.get_bit(59), "FTZ bit should be set");
}

#[test]
fn op_fsetp_float_unit_and_set_op() {
    let mut e = sm20_encoder();
    OpFSetP {
        dst: pred_dst(0),
        srcs: [gpr_src(2), gpr_src(3), true.into()],
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdEq,
        ftz: true,
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Float as u64);
    assert!(e.get_bit(59), "FTZ bit should be set on FSetP");
    let set_op: u64 = e.get_field(53..55);
    assert_eq!(set_op, 0, "PredSetOp::And should be 0");
}

#[test]
fn op_fswz_unit_and_shuffle_field() {
    let mut e = sm20_encoder();
    OpFSwz {
        dst: gpr_dst(1),
        srcs: [gpr_src(2), gpr_src(3)],
        shuffle: FSwzShuffle::SwapHorizontal,
        ops: [FSwzAddOp::Add; 4],
        ftz: false,
        rnd_mode: FRndMode::NearestEven,
        deriv_mode: TexDerivMode::Auto,
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Float as u64);
    assert_eq!(opcode_byte(&e), 0x12);
    let shuffle: u64 = e.get_field(6..9);
    assert_eq!(shuffle, 4, "SwapHorizontal should be 4");
}
