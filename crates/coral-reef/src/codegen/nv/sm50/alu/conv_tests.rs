// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals

use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    CBuf, CBufRef, Dst, FRndMode, FloatType, IntType, OpF2F, OpF2I, OpI2F, OpI2I, RegFile, RegRef,
    Src, SrcMod, SrcRef, SrcSwizzle,
};
use crate::codegen::nv::sm50::encoder::{SM50Encoder, SM50Op, ShaderModel50};

fn gpr_f32(idx: u32) -> Src {
    Src {
        reference: SrcRef::Reg(RegRef::new(RegFile::GPR, idx, 1)),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn zero_src() -> Src {
    Src {
        reference: SrcRef::Zero,
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn cb_f32() -> Src {
    Src {
        reference: SrcRef::CBuf(CBufRef {
            buf: CBuf::Binding(1),
            offset: 0x20,
        }),
        modifier: SrcMod::None,
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

#[test]
fn op_f2f_src_forms_f32_to_f32() {
    let f2f = |src: Src| OpF2F {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 2, 1)),
        src,
        src_type: FloatType::F32,
        dst_type: FloatType::F32,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dst_high: false,
        integer_rnd: false,
    };

    let mut e = encoder_sm50();
    f2f(gpr_f32(1)).encode(&mut e);
    let reg_pat = e.inst;

    let mut e = encoder_sm50();
    f2f(zero_src()).encode(&mut e);
    let zero_pat = e.inst;
    assert_ne!(reg_pat, zero_pat);

    let mut e = encoder_sm50();
    f2f(cb_f32()).encode(&mut e);
    assert_ne!(reg_pat, e.inst);
    assert_ne!(zero_pat, e.inst);
}

#[test]
fn op_f2i_src_forms() {
    let f2i = |src: Src| OpF2I {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 3, 1)),
        src,
        src_type: FloatType::F32,
        dst_type: IntType::I32,
        rnd_mode: FRndMode::Zero,
        ftz: true,
    };

    let mut e = encoder_sm50();
    f2i(gpr_f32(4)).encode(&mut e);
    let r = e.inst;

    let mut e = encoder_sm50();
    f2i(zero_src()).encode(&mut e);
    let z = e.inst;
    assert_ne!(r, z);

    let mut e = encoder_sm50();
    f2i(cb_f32()).encode(&mut e);
    assert_ne!(r, e.inst);
    assert_ne!(z, e.inst);
}

#[test]
fn op_i2f_src_forms() {
    let i2f = |src: Src| OpI2F {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 5, 1)),
        src,
        dst_type: FloatType::F32,
        src_type: IntType::I32,
        rnd_mode: FRndMode::PosInf,
    };

    let mut e = encoder_sm50();
    i2f(gpr_f32(6)).encode(&mut e);
    let r = e.inst;

    let mut e = encoder_sm50();
    i2f(zero_src()).encode(&mut e);
    let z = e.inst;
    assert_ne!(r, z);

    let mut e = encoder_sm50();
    i2f(cb_f32()).encode(&mut e);
    assert_ne!(r, e.inst);
    assert_ne!(z, e.inst);
}

#[test]
fn op_i2i_src_forms() {
    let i2i = |src: Src| OpI2I {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 7, 1)),
        src,
        src_type: IntType::I32,
        dst_type: IntType::I32,
        saturate: false,
        abs: true,
        neg: false,
    };

    let mut e = encoder_sm50();
    i2i(gpr_f32(8)).encode(&mut e);
    let r = e.inst;

    let mut e = encoder_sm50();
    i2i(zero_src()).encode(&mut e);
    let z = e.inst;
    assert_ne!(r, z);

    let mut e = encoder_sm50();
    i2i(cb_f32()).encode(&mut e);
    assert_ne!(r, e.inst);
    assert_ne!(z, e.inst);
}
