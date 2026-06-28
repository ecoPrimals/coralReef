// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unit tests for shuffle, permutation, and predicate ops.

use super::*;
use crate::codegen::ir::{FoldData, IntCmpType, LogicOp3, OpFoldData, PredSetOp, RegFile, ShflOp};
use crate::codegen::ir::{ShaderModelInfo, SrcMod};
use crate::codegen::ssa_value::SSAValueAllocator;

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
fn test_display_shuffle_ops() {
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
