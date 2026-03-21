// SPDX-License-Identifier: AGPL-3.0-only

use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    CBuf, CBufRef, Dst, FRndMode, FloatType, IntType, OpF2F, OpF2I, OpI2F, OpI2I, RegFile, RegRef,
    Src, SrcMod, SrcRef, SrcSwizzle,
};
use crate::codegen::nv::sm32::encoder::{SM32Encoder, SM32Op, ShaderModel32};

fn gpr_f32(idx: u32) -> Src {
    Src {
        reference: SrcRef::Reg(RegRef::new(RegFile::GPR, idx, 1)),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn cb_f32() -> Src {
    Src {
        reference: SrcRef::CBuf(CBufRef {
            buf: CBuf::Binding(0),
            offset: 0x10,
        }),
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
fn op_f2f_reg_and_cbuf_f32() {
    let dst = Dst::Reg(RegRef::new(RegFile::GPR, 1, 1));
    let op = OpF2F {
        dst,
        src: gpr_f32(2),
        src_type: FloatType::F32,
        dst_type: FloatType::F32,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dst_high: false,
        integer_rnd: false,
    };
    let mut e = encoder_sm32();
    op.encode(&mut e);
    let reg_inst = e.inst;

    let mut e = encoder_sm32();
    OpF2F {
        src: cb_f32(),
        ..op
    }
    .encode(&mut e);
    assert_ne!(reg_inst, e.inst);
}

#[test]
fn op_f2i_reg_and_cbuf() {
    let dst = Dst::Reg(RegRef::new(RegFile::GPR, 3, 1));
    let op = OpF2I {
        dst,
        src: gpr_f32(4),
        src_type: FloatType::F32,
        dst_type: IntType::U32,
        rnd_mode: FRndMode::NegInf,
        ftz: false,
    };
    let mut e = encoder_sm32();
    op.encode(&mut e);
    let reg_inst = e.inst;

    let mut e = encoder_sm32();
    OpF2I {
        src: cb_f32(),
        ..op
    }
    .encode(&mut e);
    assert_ne!(reg_inst, e.inst);
}

#[test]
fn op_i2f_reg_and_cbuf() {
    let dst = Dst::Reg(RegRef::new(RegFile::GPR, 5, 1));
    let op = OpI2F {
        dst,
        src: gpr_f32(6),
        dst_type: FloatType::F32,
        src_type: IntType::I32,
        rnd_mode: FRndMode::Zero,
    };
    let mut e = encoder_sm32();
    op.encode(&mut e);
    let reg_inst = e.inst;

    let mut e = encoder_sm32();
    OpI2F {
        src: cb_f32(),
        ..op
    }
    .encode(&mut e);
    assert_ne!(reg_inst, e.inst);
}

#[test]
fn op_i2i_reg_and_cbuf() {
    let dst = Dst::Reg(RegRef::new(RegFile::GPR, 7, 1));
    let op = OpI2I {
        dst,
        src: gpr_f32(8),
        src_type: IntType::I32,
        dst_type: IntType::I32,
        saturate: true,
        abs: false,
        neg: true,
    };
    let mut e = encoder_sm32();
    op.encode(&mut e);
    let reg_inst = e.inst;

    let mut e = encoder_sm32();
    OpI2I {
        src: cb_f32(),
        ..op
    }
    .encode(&mut e);
    assert_ne!(reg_inst, e.inst);
}
