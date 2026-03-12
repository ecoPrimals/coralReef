// SPDX-License-Identifier: AGPL-3.0-only
//! Unit tests for conversion, move, shuffle, and permutation ops.

use super::*;
use crate::codegen::ir::{FRndMode, RegFile, SrcMod, SrcSwizzle};
use crate::codegen::ir::{FoldData, OpFoldData, ShaderModelInfo};
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
