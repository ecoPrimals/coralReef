// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
//! Unit tests for [`super::emit::SSABuilder`] helpers in `emit.rs`.

use super::Builder;
use super::SSABuilder;
use super::SSAInstrBuilder;
use super::SSAValueAllocator;
use super::tests::make_sm50;
use super::tests::make_sm70;
use crate::codegen::ir::{
    FloatCmpOp, HasRegFile, IntCmpOp, IntCmpType, IntType, LogicOp2, Op, RegFile, SrcRef,
    TranscendentalOp,
};

#[test]
fn emit_shl_sm50_emits_op_shl() {
    let sm = make_sm50();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa(RegFile::GPR);
    let s = b.alloc_ssa(RegFile::GPR);
    let _ = b.shl(x.into(), s.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 1);
    let Op::Shl(op) = &instrs[0].op else {
        panic!("expected OpShl on SM50");
    };
    assert!(op.wrap);
}

#[test]
fn emit_shl_sm70_emits_op_shf_left_u32() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa(RegFile::GPR);
    let s = b.alloc_ssa(RegFile::GPR);
    let _ = b.shl(x.into(), s.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 1);
    let Op::Shf(op) = &instrs[0].op else {
        panic!("expected OpShf on SM70");
    };
    assert!(!op.right);
    assert!(op.wrap);
    assert_eq!(op.data_type, IntType::I32);
    assert!(!op.dst_high);
}

#[test]
fn emit_shl64_sm70_two_shf_u64() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa_vec(RegFile::GPR, 2);
    let s = b.alloc_ssa(RegFile::GPR);
    let _ = b.shl64(x.into(), s.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    assert!(instrs.iter().all(|i| matches!(i.op, Op::Shf(_))));
    let Op::Shf(first) = &instrs[0].op else {
        unreachable!();
    };
    assert_eq!(first.data_type, IntType::U64);
    assert!(!first.dst_high);
}

#[test]
fn emit_shl64_sm50_first_shf_uses_high_dst() {
    let sm = make_sm50();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa_vec(RegFile::GPR, 2);
    let s = b.alloc_ssa(RegFile::GPR);
    let _ = b.shl64(x.into(), s.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    let Op::Shf(first) = &instrs[0].op else {
        panic!("expected OpShf");
    };
    assert!(first.dst_high);
    assert_eq!(first.data_type, IntType::U64);
}

#[test]
fn emit_shr_sm50_emits_op_shr_signed_unsigned() {
    let sm = make_sm50();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa(RegFile::GPR);
    let s = b.alloc_ssa(RegFile::GPR);
    let _ = b.shr(x.into(), s.into(), false);
    let _ = b.shr(x.into(), s.into(), true);
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    let Op::Shr(u) = &instrs[0].op else {
        panic!("expected OpShr");
    };
    assert!(!u.signed);
    let Op::Shr(signed) = &instrs[1].op else {
        panic!("expected OpShr");
    };
    assert!(signed.signed);
}

#[test]
fn emit_shr_sm70_emits_op_shf_right() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa(RegFile::GPR);
    let s = b.alloc_ssa(RegFile::GPR);
    let _ = b.shr(x.into(), s.into(), false);
    let _ = b.shr(x.into(), s.into(), true);
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    let Op::Shf(u) = &instrs[0].op else {
        panic!("expected OpShf");
    };
    assert!(u.right);
    assert_eq!(u.data_type, IntType::U32);
    let Op::Shf(signed) = &instrs[1].op else {
        panic!("expected OpShf");
    };
    assert_eq!(signed.data_type, IntType::I32);
}

#[test]
fn emit_shr64_signed_and_unsigned() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x_u = b.alloc_ssa_vec(RegFile::GPR, 2);
    let x_s = b.alloc_ssa_vec(RegFile::GPR, 2);
    let s = b.alloc_ssa(RegFile::GPR);
    let _ = b.shr64(x_u.into(), s.into(), false);
    let _ = b.shr64(x_s.into(), s.into(), true);
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 4);
    let Op::Shf(a) = &instrs[0].op else {
        panic!("expected OpShf");
    };
    assert_eq!(a.data_type, IntType::U64);
    let Op::Shf(c) = &instrs[2].op else {
        panic!("expected OpShf");
    };
    assert_eq!(c.data_type, IntType::I64);
}

#[test]
fn emit_urol_uror_distinct_dst_high() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa(RegFile::GPR);
    let s = b.alloc_ssa(RegFile::GPR);
    let _ = b.urol(x.into(), s.into());
    let _ = b.uror(x.into(), s.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    let Op::Shf(rol) = &instrs[0].op else {
        panic!("expected OpShf");
    };
    assert!(!rol.right);
    assert!(rol.dst_high);
    let Op::Shf(ror) = &instrs[1].op else {
        panic!("expected OpShf");
    };
    assert!(ror.right);
    assert!(!ror.dst_high);
}

#[test]
fn emit_fsetp_hadd2_hset2_dsetp() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa(RegFile::GPR);
    let y = b.alloc_ssa(RegFile::GPR);
    let p = b.fsetp(FloatCmpOp::OrdLt, x.into(), y.into());
    assert!(p.file().is_predicate());
    let _ = b.hadd2(x.into(), y.into());
    let _ = b.hset2(FloatCmpOp::OrdEq, x.into(), y.into());
    let _ = b.dsetp(FloatCmpOp::OrdGt, x.into(), y.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 4);
    let Op::FSetP(fp) = &instrs[0].op else {
        panic!("expected OpFSetP");
    };
    assert_eq!(fp.cmp_op, FloatCmpOp::OrdLt);
    assert!(matches!(instrs[1].op, Op::HAdd2(_)));
    let Op::HSet2(hs) = &instrs[2].op else {
        panic!("expected OpHSet2");
    };
    assert_eq!(hs.cmp_op, FloatCmpOp::OrdEq);
    let Op::DSetP(ds) = &instrs[3].op else {
        panic!("expected OpDSetP");
    };
    assert_eq!(ds.cmp_op, FloatCmpOp::OrdGt);
}

#[test]
fn emit_iabs_sm70_vs_sm50() {
    let sm70 = make_sm70();
    let mut alloc70 = SSAValueAllocator::new();
    let mut b70 = SSAInstrBuilder::new(&sm70, &mut alloc70);
    let x = b70.alloc_ssa(RegFile::GPR);
    let _ = b70.iabs(x.into());
    let iabs = b70.into_vec();
    assert!(matches!(iabs[0].op, Op::IAbs(_)));

    let sm50 = make_sm50();
    let mut alloc50 = SSAValueAllocator::new();
    let mut b50 = SSAInstrBuilder::new(&sm50, &mut alloc50);
    let y = b50.alloc_ssa(RegFile::GPR);
    let _ = b50.iabs(y.into());
    let i2i = b50.into_vec();
    let Op::I2I(op) = &i2i[0].op else {
        panic!("expected OpI2I fallback on SM50");
    };
    assert!(op.abs);
    assert!(!op.neg);
}

#[test]
fn emit_iadd_sm50_requires_zero_third_source() {
    let sm = make_sm50();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa(RegFile::GPR);
    let y = b.alloc_ssa(RegFile::GPR);
    let _ = b.iadd(x.into(), y.into(), 0.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 1);
    assert!(matches!(instrs[0].op, Op::IAdd2(_)));
}

#[test]
fn emit_iadd_sm70_three_source() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa(RegFile::GPR);
    let y = b.alloc_ssa(RegFile::GPR);
    let z = b.alloc_ssa(RegFile::GPR);
    let _ = b.iadd(x.into(), y.into(), z.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 1);
    assert!(matches!(instrs[0].op, Op::IAdd3(_)));
}

#[test]
fn emit_iadd64_sm70_two_source_single_carry_slot() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa_vec(RegFile::GPR, 2);
    let y = b.alloc_ssa_vec(RegFile::GPR, 2);
    let _ = b.iadd64(x.into(), y.into(), 0.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    assert!(matches!(instrs[0].op, Op::IAdd3(_)));
    let Op::IAdd3X(ix) = &instrs[1].op else {
        panic!("expected OpIAdd3X");
    };
    assert_eq!(
        ix.srcs[4].as_bool(),
        Some(false),
        "second carry should be constant false when z is zero"
    );
}

#[test]
fn emit_iadd64_sm70_three_source_two_carries() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa_vec(RegFile::GPR, 2);
    let y = b.alloc_ssa_vec(RegFile::GPR, 2);
    let z = b.alloc_ssa_vec(RegFile::GPR, 2);
    let _ = b.iadd64(x.into(), y.into(), z.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    let Op::IAdd3X(ix) = &instrs[1].op else {
        panic!("expected OpIAdd3X");
    };
    assert!(
        matches!(ix.srcs[4].reference, SrcRef::SSA(_)),
        "second carry should be a real predicate when all sources are non-zero"
    );
}

#[test]
fn emit_iadd64_sm50_carry_chain() {
    let sm = make_sm50();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa_vec(RegFile::GPR, 2);
    let y = b.alloc_ssa_vec(RegFile::GPR, 2);
    let _ = b.iadd64(x.into(), y.into(), 0.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    assert!(matches!(instrs[0].op, Op::IAdd2(_)));
    assert!(matches!(instrs[1].op, Op::IAdd2X(_)));
}

#[test]
fn emit_iadd64_negated_splits_sources() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa_vec(RegFile::GPR, 2);
    let y = b.alloc_ssa_vec(RegFile::GPR, 2);
    let x_src: crate::codegen::ir::Src = x.into();
    let x_neg = x_src.ineg();
    let _ = b.iadd64(x_neg, y.into(), 0.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    let Op::IAdd3(a3) = &instrs[0].op else {
        panic!("expected OpIAdd3");
    };
    assert!(a3.srcs[0].modifier.is_ineg());
}

#[test]
fn emit_imnmx_immediate_min_bit() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa(RegFile::GPR);
    let y = b.alloc_ssa(RegFile::GPR);
    let _ = b.imnmx(IntCmpType::I32, x.into(), y.into(), true.into());
    let instrs = b.into_vec();
    let Op::IMnMx(op) = &instrs[0].op else {
        panic!("expected OpIMnMx");
    };
    assert!(matches!(op.cmp_type, IntCmpType::I32));
}

#[test]
fn emit_imul_sm50_vs_sm70() {
    let sm50 = make_sm50();
    let mut a50 = SSAValueAllocator::new();
    let mut b50 = SSAInstrBuilder::new(&sm50, &mut a50);
    let x50 = b50.alloc_ssa(RegFile::GPR);
    let y50 = b50.alloc_ssa(RegFile::GPR);
    let _ = b50.imul(x50.into(), y50.into());
    let v50 = b50.into_vec();
    assert!(matches!(v50[0].op, Op::IMul(_)));

    let sm70 = make_sm70();
    let mut a70 = SSAValueAllocator::new();
    let mut b70 = SSAInstrBuilder::new(&sm70, &mut a70);
    let x70 = b70.alloc_ssa(RegFile::GPR);
    let y70 = b70.alloc_ssa(RegFile::GPR);
    let _ = b70.imul(x70.into(), y70.into());
    let v70 = b70.into_vec();
    assert!(matches!(v70[0].op, Op::IMad(_)));
}

#[test]
fn emit_imul_2x32_64_sm70_mad64() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa(RegFile::GPR);
    let y = b.alloc_ssa(RegFile::GPR);
    let _ = b.imul_2x32_64(x.into(), y.into(), true);
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 1);
    let Op::IMad64(m) = &instrs[0].op else {
        panic!("expected OpIMad64");
    };
    assert!(m.signed);
}

#[test]
fn emit_imul_2x32_64_sm50_two_imul() {
    let sm = make_sm50();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa(RegFile::GPR);
    let y = b.alloc_ssa(RegFile::GPR);
    let _ = b.imul_2x32_64(x.into(), y.into(), false);
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    let Op::IMul(lo) = &instrs[0].op else {
        panic!("expected OpIMul");
    };
    assert!(!lo.high);
    let Op::IMul(hi) = &instrs[1].op else {
        panic!("expected OpIMul");
    };
    assert!(hi.high);
}

#[test]
fn emit_ineg_sm50_iadd2() {
    let sm = make_sm50();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa(RegFile::GPR);
    let _ = b.ineg(x.into());
    let instrs = b.into_vec();
    assert!(matches!(instrs[0].op, Op::IAdd2(_)));
}

#[test]
fn emit_ineg64_delegates_to_iadd64() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa_vec(RegFile::GPR, 2);
    let _ = b.ineg64(x.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    assert!(matches!(instrs[0].op, Op::IAdd3(_)));
}

#[test]
fn emit_isetp_fields() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa(RegFile::GPR);
    let y = b.alloc_ssa(RegFile::GPR);
    let _ = b.isetp(IntCmpType::I32, IntCmpOp::Lt, x.into(), y.into());
    let instrs = b.into_vec();
    let Op::ISetP(op) = &instrs[0].op else {
        panic!("expected OpISetP");
    };
    assert!(matches!(op.cmp_type, IntCmpType::I32));
    assert!(matches!(op.cmp_op, IntCmpOp::Lt));
    assert!(!op.ex);
}

#[test]
fn emit_isetp64_eq_sm70_merges_high() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa_vec(RegFile::GPR, 2);
    let y = b.alloc_ssa_vec(RegFile::GPR, 2);
    let _ = b.isetp64(IntCmpType::I32, IntCmpOp::Eq, x.into(), y.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    assert!(matches!(instrs[0].op, Op::ISetP(_)));
    let Op::ISetP(merge) = &instrs[1].op else {
        panic!("expected merge ISetP");
    };
    assert!(matches!(merge.cmp_op, IntCmpOp::Eq));
    assert!(matches!(merge.cmp_type, IntCmpType::U32));
    assert!(!merge.ex);
}

#[test]
fn emit_isetp64_lt_sm70_uses_ex() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa_vec(RegFile::GPR, 2);
    let y = b.alloc_ssa_vec(RegFile::GPR, 2);
    let _ = b.isetp64(IntCmpType::I32, IntCmpOp::Lt, x.into(), y.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    let Op::ISetP(ord) = &instrs[1].op else {
        panic!("expected ordering ISetP");
    };
    assert!(ord.ex);
    assert!(matches!(ord.cmp_op, IntCmpOp::Lt));
    assert!(matches!(ord.cmp_type, IntCmpType::I32));
}

#[test]
fn emit_isetp64_lt_sm50_three_compare_ops() {
    let sm = make_sm50();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa_vec(RegFile::GPR, 2);
    let y = b.alloc_ssa_vec(RegFile::GPR, 2);
    let _ = b.isetp64(IntCmpType::I32, IntCmpOp::Lt, x.into(), y.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 3);
    assert!(instrs.iter().all(|i| matches!(i.op, Op::ISetP(_))));
}

#[test]
fn emit_lea_shift_reduced_mod_32() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let a = b.alloc_ssa(RegFile::GPR);
    let c = b.alloc_ssa(RegFile::GPR);
    let _ = b.lea(a.into(), c.into(), 40);
    let instrs = b.into_vec();
    let Op::Lea(op) = &instrs[0].op else {
        panic!("expected OpLea");
    };
    assert_eq!(op.shift, 8);
}

#[test]
fn emit_lea64_low_shift_uses_lea_and_leax() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let a = b.alloc_ssa_vec(RegFile::GPR, 2);
    let c = b.alloc_ssa_vec(RegFile::GPR, 2);
    let _ = b.lea64(a.into(), c.into(), 3);
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    assert!(matches!(instrs[0].op, Op::Lea(_)));
    assert!(matches!(instrs[1].op, Op::LeaX(_)));
}

#[test]
fn emit_lea64_high_shift_copies_low_then_lea() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let a = b.alloc_ssa_vec(RegFile::GPR, 2);
    let c = b.alloc_ssa_vec(RegFile::GPR, 2);
    let _ = b.lea64(a.into(), c.into(), 40);
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    assert!(matches!(instrs[0].op, Op::Copy(_)));
    let Op::Lea(op) = &instrs[1].op else {
        panic!("expected OpLea");
    };
    assert_eq!(op.shift, 8);
}

#[test]
fn emit_lop2_gpr_vs_pred_files() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let gx = b.alloc_ssa(RegFile::GPR);
    let gy = b.alloc_ssa(RegFile::GPR);
    let g = b.lop2(LogicOp2::Xor, gx.into(), gy.into());
    assert!(g.file().is_gpr());
    let px = b.alloc_ssa(RegFile::Pred);
    let py = b.alloc_ssa(RegFile::Pred);
    let p = b.lop2(LogicOp2::Or, px.into(), py.into());
    assert!(p.file().is_predicate());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    assert!(matches!(instrs[0].op, Op::Lop3(_) | Op::Lop2(_)));
    assert!(matches!(instrs[1].op, Op::PLop3(_) | Op::PSetP(_)));
}

#[test]
fn emit_brev_sm70_vs_sm50() {
    let sm70 = make_sm70();
    let mut a70 = SSAValueAllocator::new();
    let mut b70 = SSAInstrBuilder::new(&sm70, &mut a70);
    let x70 = b70.alloc_ssa(RegFile::GPR);
    let _ = b70.brev(x70.into());
    let v70 = b70.into_vec();
    assert!(matches!(v70[0].op, Op::BRev(_)));

    let sm50 = make_sm50();
    let mut a50 = SSAValueAllocator::new();
    let mut b50 = SSAInstrBuilder::new(&sm50, &mut a50);
    let x50 = b50.alloc_ssa(RegFile::GPR);
    let _ = b50.brev(x50.into());
    let v50 = b50.into_vec();
    let Op::Bfe(bfe) = &v50[0].op else {
        panic!("expected OpBfe fallback");
    };
    assert!(bfe.reverse);
}

#[test]
fn emit_transcendental_roundtrip() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let s = b.alloc_ssa(RegFile::GPR);
    let _ = b.transcendental(TranscendentalOp::Rcp, s.into());
    let instrs = b.into_vec();
    let Op::Transcendental(op) = &instrs[0].op else {
        panic!("expected OpTranscendental");
    };
    assert!(matches!(op.op, TranscendentalOp::Rcp));
}

#[test]
fn emit_fsin_fcos_fexp2_sm50_use_rro_where_required() {
    let sm = make_sm50();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let s = b.alloc_ssa(RegFile::GPR);
    let _ = b.fsin(s.into());
    let _ = b.fcos(s.into());
    let _ = b.fexp2(s.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 6);
    assert!(matches!(instrs[0].op, Op::Rro(_)));
    assert!(matches!(instrs[2].op, Op::Rro(_)));
    assert!(matches!(instrs[4].op, Op::Rro(_)));
    for i in [1, 3, 5] {
        assert!(matches!(instrs[i].op, Op::Transcendental(_)));
    }
}

#[test]
fn emit_prmt_non_identity_emits_prmt() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa(RegFile::GPR);
    let y = b.alloc_ssa(RegFile::GPR);
    let _ = b.prmt(x.into(), y.into(), [1, 0, 3, 2]);
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 1);
    assert!(matches!(instrs[0].op, Op::Prmt(_)));
}

#[test]
fn emit_prmt4_branches_cover_byte_ranges() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let s0 = b.alloc_ssa(RegFile::GPR);
    let s1 = b.alloc_ssa(RegFile::GPR);
    let s2 = b.alloc_ssa(RegFile::GPR);
    let s3 = b.alloc_ssa(RegFile::GPR);
    let _ = b.prmt4([s0.into(), s1.into(), s2.into(), s3.into()], [0, 1, 9, 10]);
    let instrs = b.into_vec();
    assert!(
        instrs.iter().any(|i| matches!(i.op, Op::Prmt(_))),
        "8–12 selector range should emit at least one PRMT"
    );

    let mut alloc2 = SSAValueAllocator::new();
    let mut b2 = SSAInstrBuilder::new(&sm, &mut alloc2);
    let t0 = b2.alloc_ssa(RegFile::GPR);
    let t1 = b2.alloc_ssa(RegFile::GPR);
    let t2 = b2.alloc_ssa(RegFile::GPR);
    let t3 = b2.alloc_ssa(RegFile::GPR);
    let _ = b2.prmt4(
        [t0.into(), t1.into(), t2.into(), t3.into()],
        [12, 13, 14, 15],
    );
    let instrs2 = b2.into_vec();
    let prmt_count = instrs2
        .iter()
        .filter(|i| matches!(i.op, Op::Prmt(_)))
        .count();
    assert!(
        prmt_count >= 1 && instrs2.len() >= 2,
        "12–16 selector range should lower through PRMT and/or copy steps (prmt_count={prmt_count}, len={})",
        instrs2.len()
    );
}

#[test]
fn emit_sel_gpr_vs_pred_sm70() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let c = b.alloc_ssa(RegFile::Pred);
    let x = b.alloc_ssa(RegFile::GPR);
    let y = b.alloc_ssa(RegFile::GPR);
    let g = b.sel(c.into(), x.into(), y.into());
    assert!(g.file().is_gpr());
    let p1 = b.alloc_ssa(RegFile::Pred);
    let p2 = b.alloc_ssa(RegFile::Pred);
    let pr = b.sel(c.into(), p1.into(), p2.into());
    assert!(pr.file().is_predicate());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    assert!(matches!(instrs[0].op, Op::Sel(_)));
    assert!(matches!(instrs[1].op, Op::PLop3(_)));
}

#[test]
fn emit_sel_pred_sm50_two_psetp() {
    let sm = make_sm50();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let c = b.alloc_ssa(RegFile::Pred);
    let x = b.alloc_ssa(RegFile::Pred);
    let y = b.alloc_ssa(RegFile::Pred);
    let _ = b.sel(c.into(), x.into(), y.into());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    assert!(matches!(instrs[0].op, Op::PSetP(_)));
    assert!(matches!(instrs[1].op, Op::PSetP(_)));
}

#[test]
fn emit_copy_emits_op_copy() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let x = b.alloc_ssa(RegFile::GPR);
    let _ = b.copy(x.into());
    let instrs = b.into_vec();
    assert!(matches!(instrs[0].op, Op::Copy(_)));
}

#[test]
fn emit_bmov_bar_roundtrip() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let g = b.alloc_ssa(RegFile::GPR);
    let bar = b.bmov_to_bar(g.into());
    assert_eq!(bar.file(), RegFile::Bar);
    let back = b.bmov_to_gpr(bar.into());
    assert!(back.file().is_gpr());
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 2);
    let Op::BMov(a) = &instrs[0].op else {
        panic!("expected OpBMov");
    };
    assert!(!a.clear);
    assert!(matches!(instrs[1].op, Op::BMov(_)));
}

#[test]
fn emit_predicated_emit_wraps_predicate() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let mut b = SSAInstrBuilder::new(&sm, &mut alloc);
    let p = b.alloc_ssa(RegFile::Pred);
    let x = b.alloc_ssa(RegFile::GPR);
    let y = b.alloc_ssa(RegFile::GPR);
    {
        let mut pb = b.predicate(p.into());
        let _ = pb.fadd(x.into(), y.into());
    }
    let instrs = b.into_vec();
    assert_eq!(instrs.len(), 1);
    assert!(!instrs[0].pred.is_true());
}
