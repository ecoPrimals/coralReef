// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unit tests for conversion and move ops.

use super::*;
use crate::codegen::ir::FRndMode;
use crate::codegen::ir::SrcSwizzle;

#[test]
fn test_prmt_sel_byte_new_and_fold_u32() {
    let b0 = PrmtSelByte::new(0, 0, false);
    assert_eq!(b0.src(), 0);
    assert_eq!(b0.byte(), 0);
    assert!(!b0.msb());
    assert_eq!(b0.fold_u32(0x4433_2211), 0x11);

    let b1 = PrmtSelByte::new(0, 1, false);
    assert_eq!(b1.fold_u32(0x4433_2211), 0x22);

    let b2 = PrmtSelByte::new(1, 2, false);
    assert_eq!(b2.fold_u32(0x8877_6655), 0x77);

    let b_msb = PrmtSelByte::new(0, 3, true);
    assert!(b_msb.msb());
    assert_eq!(b_msb.fold_u32(0x8000_0000), 0xff);
    assert_eq!(b_msb.fold_u32(0x7f00_0000), 0x00);

    assert!(
        !PrmtSelByte::INVALID.is_valid(),
        "INVALID must not alias a valid nibble"
    );
}

#[test]
fn test_prmt_sel_construction_and_get() {
    let bytes = [
        PrmtSelByte::new(0, 0, false),
        PrmtSelByte::new(0, 1, false),
        PrmtSelByte::new(0, 2, false),
        PrmtSelByte::new(0, 3, false),
    ];
    let sel = PrmtSel::new(bytes);
    assert_eq!(sel.0, 0x3210);

    for i in 0..4 {
        let b = sel.get(i);
        assert_eq!(b.src(), 0);
        assert_eq!(b.byte(), i);
    }
}

#[test]
fn test_op_f2f_is_high() {
    let op = OpF2F {
        dst: Dst::None,
        src: Src::new_imm_u32(0).swizzle(SrcSwizzle::Yy),
        src_type: FloatType::F16,
        dst_type: FloatType::F32,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dst_high: false,
        integer_rnd: false,
    };
    assert!(op.is_high());

    let op_none = OpF2F {
        src: Src::new_imm_u32(0),
        ..op
    };
    assert!(!op_none.is_high());

    let op_dst_high = OpF2F {
        dst: Dst::None,
        src: Src::ZERO,
        src_type: FloatType::F32,
        dst_type: FloatType::F16,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dst_high: true,
        integer_rnd: false,
    };
    assert!(op_dst_high.is_high());

    let op_f32 = OpF2F {
        dst_high: false,
        ..op_dst_high
    };
    let op_f32 = OpF2F {
        src_type: FloatType::F32,
        dst_type: FloatType::F32,
        ..op_f32
    };
    assert!(!op_f32.is_high());
}

#[test]
fn test_display_conv_ops() {
    let s = format!(
        "{}",
        OpMov {
            dst: Dst::None,
            src: Src::new_imm_u32(0x42),
            quad_lanes: 0xf,
        }
    );
    assert!(s.contains("mov"));
    assert!(s.contains("0x42"));

    let s = format!(
        "{}",
        OpF2F {
            dst: Dst::None,
            src: Src::ZERO,
            src_type: FloatType::F32,
            dst_type: FloatType::F32,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dst_high: false,
            integer_rnd: false,
        }
    );
    assert!(s.contains("f2f"));
}

#[test]
fn test_op_f2f_display() {
    fn f2f(
        dst_type: FloatType,
        src_type: FloatType,
        rnd_mode: FRndMode,
        ftz: bool,
        dst_high: bool,
        integer_rnd: bool,
    ) -> OpF2F {
        OpF2F {
            dst: Dst::None,
            src: Src::ZERO,
            src_type,
            dst_type,
            rnd_mode,
            ftz,
            dst_high,
            integer_rnd,
        }
    }

    assert_eq!(
        format!(
            "{}",
            f2f(
                FloatType::F32,
                FloatType::F32,
                FRndMode::NearestEven,
                false,
                false,
                false
            )
        ),
        "null = f2f.f32.f32.re rZ"
    );

    assert_eq!(
        format!(
            "{}",
            f2f(
                FloatType::F32,
                FloatType::F32,
                FRndMode::NearestEven,
                true,
                false,
                false
            )
        ),
        "null = f2f.ftz.f32.f32.re rZ"
    );

    assert_eq!(
        format!(
            "{}",
            f2f(
                FloatType::F32,
                FloatType::F32,
                FRndMode::NearestEven,
                false,
                false,
                true
            )
        ),
        "null = f2f.int.f32.f32.re rZ"
    );

    assert_eq!(
        format!(
            "{}",
            f2f(
                FloatType::F16,
                FloatType::F32,
                FRndMode::NearestEven,
                false,
                true,
                false
            )
        ),
        "null = f2f.high.f16.f32.re rZ"
    );

    assert_eq!(
        format!(
            "{}",
            f2f(
                FloatType::F16,
                FloatType::F64,
                FRndMode::Zero,
                false,
                false,
                false
            )
        ),
        "null = f2f.f16.f64.rz rZ"
    );

    assert_eq!(
        format!(
            "{}",
            f2f(
                FloatType::F16,
                FloatType::F32,
                FRndMode::NegInf,
                true,
                true,
                true,
            )
        ),
        "null = f2f.ftz.int.high.f16.f32.rm rZ"
    );
}

#[test]
fn test_op_f2fp_display() {
    assert_eq!(
        format!(
            "{}",
            OpF2FP {
                dst: Dst::None,
                srcs: [Src::ZERO, Src::new_imm_u32(1)],
                rnd_mode: FRndMode::NearestEven,
            }
        ),
        "null = f2fp.pack_ab rZ, 0x1"
    );

    assert_eq!(
        format!(
            "{}",
            OpF2FP {
                dst: Dst::None,
                srcs: [Src::ZERO, Src::new_imm_u32(1)],
                rnd_mode: FRndMode::PosInf,
            }
        ),
        "null = f2fp.pack_ab.rp rZ, 0x1"
    );
}

#[test]
fn test_op_f2i_display() {
    assert_eq!(
        format!(
            "{}",
            OpF2I {
                dst: Dst::None,
                src: Src::ZERO,
                src_type: FloatType::F64,
                dst_type: IntType::I32,
                rnd_mode: FRndMode::NearestEven,
                ftz: false,
            }
        ),
        "null = f2i.i32.f64.re rZ"
    );

    assert_eq!(
        format!(
            "{}",
            OpF2I {
                dst: Dst::None,
                src: Src::ZERO,
                src_type: FloatType::F16,
                dst_type: IntType::U8,
                rnd_mode: FRndMode::Zero,
                ftz: true,
            }
        ),
        "null = f2i.u8.f16.rz.ftz rZ"
    );
}

#[test]
fn test_op_i2f_display() {
    assert_eq!(
        format!(
            "{}",
            OpI2F {
                dst: Dst::None,
                src: Src::ZERO,
                dst_type: FloatType::F64,
                src_type: IntType::I64,
                rnd_mode: FRndMode::NegInf,
            }
        ),
        "null = i2f.f64.i64.rm rZ"
    );

    assert_eq!(
        format!(
            "{}",
            OpI2F {
                dst: Dst::None,
                src: Src::ZERO,
                dst_type: FloatType::F16,
                src_type: IntType::U16,
                rnd_mode: FRndMode::NearestEven,
            }
        ),
        "null = i2f.f16.u16.re rZ"
    );
}

#[test]
fn test_op_i2i_display() {
    assert_eq!(
        format!(
            "{}",
            OpI2I {
                dst: Dst::None,
                src: Src::ZERO,
                src_type: IntType::I32,
                dst_type: IntType::U32,
                saturate: false,
                abs: false,
                neg: false,
            }
        ),
        "null = i2i.u32.i32 rZ"
    );

    assert_eq!(
        format!(
            "{}",
            OpI2I {
                dst: Dst::None,
                src: Src::ZERO,
                src_type: IntType::I32,
                dst_type: IntType::U32,
                saturate: true,
                abs: false,
                neg: false,
            }
        ),
        "null = i2i.sat .u32.i32 rZ"
    );

    assert_eq!(
        format!(
            "{}",
            OpI2I {
                dst: Dst::None,
                src: Src::ZERO,
                src_type: IntType::U8,
                dst_type: IntType::I8,
                saturate: false,
                abs: true,
                neg: true,
            }
        ),
        "null = i2i.i8.u8 rZ.abs.neg"
    );
}

#[test]
fn test_op_frnd_display() {
    assert_eq!(
        format!(
            "{}",
            OpFRnd {
                dst: Dst::None,
                src: Src::ZERO,
                dst_type: FloatType::F32,
                src_type: FloatType::F32,
                rnd_mode: FRndMode::NearestEven,
                ftz: false,
            }
        ),
        "null = frnd.f32.f32.re rZ"
    );

    assert_eq!(
        format!(
            "{}",
            OpFRnd {
                dst: Dst::None,
                src: Src::ZERO,
                dst_type: FloatType::F16,
                src_type: FloatType::F64,
                rnd_mode: FRndMode::PosInf,
                ftz: true,
            }
        ),
        "null = frnd.f16.f64.rp.ftz rZ"
    );
}

#[test]
fn test_op_mov_display_quad_lanes() {
    assert_eq!(
        format!(
            "{}",
            OpMov {
                dst: Dst::None,
                src: Src::new_imm_u32(0x42),
                quad_lanes: 0xf,
            }
        ),
        "null = mov 0x42"
    );

    assert_eq!(
        format!(
            "{}",
            OpMov {
                dst: Dst::None,
                src: Src::new_imm_u32(0x42),
                quad_lanes: 0xa,
            }
        ),
        "null = mov[0xa] 0x42"
    );
}

#[test]
fn test_op_movm_display() {
    let op = OpMovm {
        dst: Dst::None,
        src: Src::ZERO,
    };
    assert_eq!(format!("{op}"), "null = movm.16.m8n8.trans rZ");
}
