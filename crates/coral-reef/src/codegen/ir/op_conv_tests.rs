// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unit tests for conversion, move, shuffle, and permutation ops.

use super::*;
use crate::codegen::ir::{FRndMode, IntCmpType, LogicOp3, PredSetOp, RegFile, ShflOp};
use crate::codegen::ir::{FoldData, OpFoldData, ShaderModelInfo};
use crate::codegen::ir::{SrcMod, SrcSwizzle};
use crate::codegen::ssa_value::SSAValueAllocator;

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

    assert_ne!(PrmtSelByte::INVALID.0, 0xf);
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
fn test_op_prmt_as_u32() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst: Dst = ssa_alloc.alloc(RegFile::GPR).into();

    let op = OpPrmt {
        dst: dst.clone(),
        srcs: [
            Src::new_imm_u32(0x4433_2211),
            Src::new_imm_u32(0x8877_6655),
            Src::new_imm_u32(0x6510),
        ],
        mode: PrmtMode::Index,
    };
    assert_eq!(op.as_u32(), Some(0x7766_2211));

    let op_id = OpPrmt {
        dst: dst.clone(),
        srcs: [
            Src::new_imm_u32(0xdead_beef),
            Src::new_imm_u32(0x1234_5678),
            Src::new_imm_u32(0x3210),
        ],
        mode: PrmtMode::Index,
    };
    assert_eq!(op_id.as_u32(), Some(0xdead_beef));

    let op_non_index = OpPrmt {
        dst,
        srcs: [
            Src::new_imm_u32(0x4433_2211),
            Src::new_imm_u32(0x8877_6655),
            Src::new_imm_u32(0x6510),
        ],
        mode: PrmtMode::Forward4Extract,
    };
    assert_eq!(op_non_index.as_u32(), None);
}

#[test]
fn test_op_prmt_foldable() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst = ssa_alloc.alloc(RegFile::GPR).into();
    let op = OpPrmt {
        dst,
        srcs: [
            Src::new_imm_u32(0x4433_2211),
            Src::new_imm_u32(0x8877_6655),
            Src::new_imm_u32(0x6510),
        ],
        mode: PrmtMode::Index,
    };

    let sm = ShaderModelInfo::new(70, 64);
    let mut dsts = [FoldData::U32(0)];
    let srcs: [FoldData; 0] = [];
    let mut f = OpFoldData {
        dsts: &mut dsts,
        srcs: &srcs,
    };
    op.fold(&sm, &mut f);
    assert_eq!(dsts[0], FoldData::U32(0x7766_2211));
}

#[test]
fn test_op_popc_foldable() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst = ssa_alloc.alloc(RegFile::GPR).into();
    let op = OpPopC {
        dst,
        src: Src::new_imm_u32(0b1011),
    };

    let sm = ShaderModelInfo::new(70, 64);
    let mut dsts = [FoldData::U32(0)];
    let mut f = OpFoldData {
        dsts: &mut dsts,
        srcs: &[],
    };
    op.fold(&sm, &mut f);
    assert_eq!(dsts[0], FoldData::U32(3));

    let op_bnot = OpPopC {
        src: Src::new_imm_u32(0b1011).modify(SrcMod::BNot),
        ..op
    };
    let mut dsts2 = [FoldData::U32(0)];
    let mut f2 = OpFoldData {
        dsts: &mut dsts2,
        srcs: &[],
    };
    op_bnot.fold(&sm, &mut f2);
    assert_eq!(dsts2[0], FoldData::U32(32 - 3));
}

#[test]
fn test_display_op_formatting() {
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
        OpPrmt {
            dst: Dst::None,
            srcs: [
                Src::new_imm_u32(0x11),
                Src::new_imm_u32(0x22),
                Src::new_imm_u32(0x3210),
            ],
            mode: PrmtMode::Index,
        }
    );
    assert!(s.contains("prmt"));
    assert!(s.contains("0x11"));
    assert!(s.contains("0x22"));

    let s = format!(
        "{}",
        OpPopC {
            dst: Dst::None,
            src: Src::new_imm_u32(7),
        }
    );
    assert!(s.contains("popc"));

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
fn test_prmt_mode_display_all_variants() {
    assert_eq!(format!("{}", PrmtMode::Index), "");
    assert_eq!(format!("{}", PrmtMode::Forward4Extract), ".f4e");
    assert_eq!(format!("{}", PrmtMode::Backward4Extract), ".b4e");
    assert_eq!(format!("{}", PrmtMode::Replicate8), ".rc8");
    assert_eq!(format!("{}", PrmtMode::EdgeClampLeft), ".ecl");
    assert_eq!(format!("{}", PrmtMode::EdgeClampRight), ".ecl");
    assert_eq!(format!("{}", PrmtMode::Replicate16), ".rc16");
}

#[test]
fn test_redux_op_display_all_variants() {
    assert_eq!(format!("{}", ReduxOp::And), ".and");
    assert_eq!(format!("{}", ReduxOp::Or), ".or");
    assert_eq!(format!("{}", ReduxOp::Xor), ".xor");
    assert_eq!(format!("{}", ReduxOp::Sum), ".sum");
    assert_eq!(format!("{}", ReduxOp::Min(IntCmpType::U32)), ".min.u32");
    assert_eq!(format!("{}", ReduxOp::Min(IntCmpType::I32)), ".min.i32");
    assert_eq!(format!("{}", ReduxOp::Max(IntCmpType::U32)), ".max.u32");
    assert_eq!(format!("{}", ReduxOp::Max(IntCmpType::I32)), ".max.i32");
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
fn test_op_f2fp_display_rounding_modes() {
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

#[test]
fn test_op_prmt_display_modes() {
    assert_eq!(
        format!(
            "{}",
            OpPrmt {
                dst: Dst::None,
                srcs: [
                    Src::new_imm_u32(0x11),
                    Src::new_imm_u32(0x22),
                    Src::new_imm_u32(0x3210),
                ],
                mode: PrmtMode::Index,
            }
        ),
        "null = prmt 0x11 [0x3210] 0x22"
    );

    assert_eq!(
        format!(
            "{}",
            OpPrmt {
                dst: Dst::None,
                srcs: [
                    Src::new_imm_u32(0x11),
                    Src::new_imm_u32(0x22),
                    Src::new_imm_u32(0x3210),
                ],
                mode: PrmtMode::Forward4Extract,
            }
        ),
        "null = prmt.f4e 0x11 [0x3210] 0x22"
    );
}

#[test]
fn test_op_prmt_reduce_sel_imm_masks_high_bits() {
    let mut op = OpPrmt {
        dst: Dst::None,
        srcs: [Src::ZERO, Src::ZERO, Src::new_imm_u32(0xdead_0000 | 0x3210)],
        mode: PrmtMode::Index,
    };
    op.reduce_sel_imm();
    let sel = op.get_sel().expect("index mode should yield PrmtSel");
    assert_eq!(sel.0, 0x3210);
}

#[test]
fn test_op_sel_display() {
    let op = OpSel {
        dst: Dst::None,
        srcs: [
            Src::new_imm_bool(true),
            Src::new_imm_u32(1),
            Src::new_imm_u32(2),
        ],
    };
    assert_eq!(format!("{op}"), "null = sel pT 0x1 0x2");
}

#[test]
fn test_op_sgxt_display() {
    let signed = OpSgxt {
        dst: Dst::None,
        srcs: [Src::new_imm_u32(0xff), Src::new_imm_u32(8)],
        signed: true,
    };
    assert_eq!(format!("{signed}"), "null = sgxt 0xff 0x8");

    let unsigned = OpSgxt {
        signed: false,
        ..signed
    };
    assert_eq!(format!("{unsigned}"), "null = sgxt.u32 0xff 0x8");
}

#[test]
fn test_op_sgxt_fold_edges() {
    let sm = ShaderModelInfo::new(70, 64);

    let mut dsts = [FoldData::U32(0)];
    let op_ge_32 = OpSgxt {
        dst: Dst::None,
        srcs: [Src::new_imm_u32(0x1234_5678), Src::new_imm_u32(40)],
        signed: true,
    };
    op_ge_32.fold(
        &sm,
        &mut OpFoldData {
            dsts: &mut dsts,
            srcs: &[],
        },
    );
    assert_eq!(dsts[0], FoldData::U32(0x1234_5678));

    let mut dsts0 = [FoldData::U32(0xffff_ffff)];
    let op_zero_bits = OpSgxt {
        dst: Dst::None,
        srcs: [Src::new_imm_u32(0xfeed), Src::new_imm_u32(0)],
        signed: true,
    };
    op_zero_bits.fold(
        &sm,
        &mut OpFoldData {
            dsts: &mut dsts0,
            srcs: &[],
        },
    );
    assert_eq!(dsts0[0], FoldData::U32(0));

    let mut dsts_sign = [FoldData::U32(0)];
    let op_sign_ext = OpSgxt {
        dst: Dst::None,
        srcs: [Src::new_imm_u32(0x0000_0080), Src::new_imm_u32(8)],
        signed: true,
    };
    op_sign_ext.fold(
        &sm,
        &mut OpFoldData {
            dsts: &mut dsts_sign,
            srcs: &[],
        },
    );
    assert_eq!(dsts_sign[0], FoldData::U32(0xffff_ff80));

    let mut dsts_us = [FoldData::U32(0)];
    let op_unsigned = OpSgxt {
        dst: Dst::None,
        srcs: [Src::new_imm_u32(0x0000_00ff), Src::new_imm_u32(8)],
        signed: false,
    };
    op_unsigned.fold(
        &sm,
        &mut OpFoldData {
            dsts: &mut dsts_us,
            srcs: &[],
        },
    );
    assert_eq!(dsts_us[0], FoldData::U32(0xff));
}

#[test]
fn test_op_shfl_display() {
    assert_eq!(
        format!(
            "{}",
            OpShfl {
                dsts: [Dst::None, Dst::None],
                srcs: [
                    Src::new_imm_u32(3),
                    Src::new_imm_u32(4),
                    Src::new_imm_u32(5),
                ],
                op: ShflOp::Idx,
            }
        ),
        "null = shfl.idx 0x3 0x4 0x5"
    );

    assert_eq!(
        format!(
            "{}",
            OpShfl {
                dsts: [Dst::None, Dst::None],
                srcs: [
                    Src::new_imm_u32(3),
                    Src::new_imm_u32(4),
                    Src::new_imm_u32(5),
                ],
                op: ShflOp::Up,
            }
        ),
        "null = shfl.up 0x3 0x4 0x5"
    );
}

#[test]
fn test_op_plop3_display() {
    let op = OpPLop3 {
        dsts: [Dst::None, Dst::None],
        srcs: [
            Src::new_imm_bool(false),
            Src::new_imm_bool(true),
            Src::new_imm_bool(false),
        ],
        ops: [LogicOp3 { lut: 0xaa }, LogicOp3 { lut: 0x55 }],
    };
    assert_eq!(
        format!("{op}"),
        "null null = plop3 pF pT pF LUT[0xaa] LUT[0x55]"
    );
}

#[test]
fn test_op_psetp_display() {
    let op = OpPSetP {
        dsts: [Dst::None, Dst::None],
        ops: [PredSetOp::And, PredSetOp::Or],
        srcs: [
            Src::new_imm_bool(true),
            Src::new_imm_bool(false),
            Src::new_imm_bool(true),
        ],
    };
    assert_eq!(format!("{op}"), "null = psetp.and.or pT pF pT");
}

#[test]
fn test_op_psetp_fold() {
    let sm = ShaderModelInfo::new(70, 64);
    let op = OpPSetP {
        dsts: [Dst::None, Dst::None],
        ops: [PredSetOp::And, PredSetOp::Or],
        srcs: [
            Src::new_imm_bool(true),
            Src::new_imm_bool(true),
            Src::new_imm_bool(false),
        ],
    };
    let mut dsts = [FoldData::Pred(false), FoldData::Pred(false)];
    op.fold(
        &sm,
        &mut OpFoldData {
            dsts: &mut dsts,
            srcs: &[],
        },
    );
    assert_eq!(dsts[0], FoldData::Pred(true));
    assert_eq!(dsts[1], FoldData::Pred(false));
}

#[test]
fn test_op_r2ur_display() {
    let op = OpR2UR {
        dst: Dst::None,
        src: Src::ZERO,
    };
    assert_eq!(format!("{op}"), "null = r2ur rZ");
}

#[test]
fn test_op_redux_display() {
    let op = OpRedux {
        dst: Dst::None,
        src: Src::ZERO,
        op: ReduxOp::Sum,
    };
    assert_eq!(format!("{op}"), "null = redux.sum rZ");
}
