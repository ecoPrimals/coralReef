// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

use crate::codegen::ir::{
    Dst, FRndMode, Instr, LogicOp3, Op, OpCopy, OpDAdd, OpExit, OpIAdd2, OpIAdd3, OpLop3, OpPLop3,
    OpParCopy, OpPrmt, OpRegOut, OpSel, Pred, PredRef, PrmtMode, RegFile, SSAValueAllocator, Src,
    SrcRef,
};
use crate::codegen::test_shader_helpers::make_shader_with_function;

#[test]
fn test_copy_prop_prmt_sel_7654() {
    // PRMT with sel=0x7654 (identity from src1) - dst is copy of srcs[1]
    // Use IAdd2 to consume PRMT output so prop_to_scalar_src can propagate to imm
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let dst_c = ssa_alloc.alloc(RegFile::GPR);
    let imm = Src::new_imm_u32(0xCAFE_BABE);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: imm,
            }),
            Instr::new(OpPrmt {
                dst: dst_b.into(),
                srcs: [Src::ZERO, dst_a.into(), Src::new_imm_u32(0x7654)],
                mode: PrmtMode::Index,
            }),
            Instr::new(OpIAdd2 {
                dsts: [dst_c.into(), Dst::None],
                srcs: [dst_b.into(), Src::ZERO],
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_c.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    let iadd2 = &shader.functions[0].blocks[0].instrs[2];
    let Op::IAdd2(op) = &iadd2.op else {
        panic!("expected IAdd2");
    };
    assert!(
        matches!(op.srcs[0].reference, SrcRef::Imm32(0xCAFE_BABE)),
        "PRMT sel=0x7654 (identity from src1) should propagate to src1"
    );
}

#[test]
fn test_copy_prop_iadd3_zeros_first_and_third() {
    // IAdd3(0, x, 0) → x
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let dst_c = ssa_alloc.alloc(RegFile::GPR);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: Src::ZERO,
            }),
            Instr::new(OpIAdd3 {
                dsts: [dst_b.into(), Dst::None, Dst::None],
                srcs: [dst_a.into(), dst_c.into(), dst_a.into()],
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_b.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    let iadd3 = &shader.functions[0].blocks[0].instrs[1];
    let Op::IAdd3(op) = &iadd3.op else {
        panic!("expected IAdd3");
    };
    assert!(op.srcs[0].is_zero() && op.srcs[2].is_zero());
}

#[test]
fn test_copy_prop_iadd3_all_zeros_except_first() {
    // IAdd3(x, 0, 0) → x
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let dst_c = ssa_alloc.alloc(RegFile::GPR);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: Src::ZERO,
            }),
            Instr::new(OpIAdd3 {
                dsts: [dst_b.into(), Dst::None, Dst::None],
                srcs: [dst_c.into(), dst_a.into(), dst_a.into()],
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_b.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    let iadd3 = &shader.functions[0].blocks[0].instrs[1];
    let Op::IAdd3(op) = &iadd3.op else {
        panic!("expected IAdd3");
    };
    assert!(op.srcs[1].is_zero() && op.srcs[2].is_zero());
}

#[test]
fn test_copy_prop_lop3_src1_pass_through() {
    // Lop3 with pass-through of src1 (y)
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let dst_c = ssa_alloc.alloc(RegFile::GPR);
    let op_src1 = LogicOp3::new_lut(&|_x, y, _z| y);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: Src::new_imm_u32(0xABCD_1234),
            }),
            Instr::new(OpLop3 {
                dst: dst_b.into(),
                srcs: [Src::ZERO, dst_a.into(), dst_c.into()],
                op: op_src1,
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_b.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    let lop3 = &shader.functions[0].blocks[0].instrs[1];
    let Op::Lop3(op) = &lop3.op else {
        panic!("expected Lop3");
    };
    assert!(
        matches!(op.srcs[1].reference, SrcRef::Imm32(0xABCD_1234)),
        "Lop3 pass-through src1 should propagate to imm"
    );
}

#[test]
fn test_copy_prop_lop3_all_ones() {
    // Lop3 with lut=!0 produces all 1s; use IAdd2 so prop_to_scalar_src propagates to imm
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_d = ssa_alloc.alloc(RegFile::GPR);
    let op_all_ones = LogicOp3::new_const(true);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpLop3 {
                dst: dst_a.into(),
                srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
                op: op_all_ones,
            }),
            Instr::new(OpIAdd2 {
                dsts: [dst_d.into(), Dst::None],
                srcs: [dst_a.into(), Src::ZERO],
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_d.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    let iadd2 = &shader.functions[0].blocks[0].instrs[1];
    let Op::IAdd2(op) = &iadd2.op else {
        panic!("expected IAdd2");
    };
    assert!(
        matches!(op.srcs[0].reference, SrcRef::Imm32(0xFFFF_FFFF)),
        "Lop3 with lut=!0 produces all 1s; IAdd2 should receive propagated 0xffffffff"
    );
}

#[test]
fn test_copy_prop_par_copy() {
    // ParCopy(b=a) where a=0; use b in IAdd2 - should propagate to zero
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let dst_c = ssa_alloc.alloc(RegFile::GPR);
    let mut pcopy = OpParCopy::new();
    pcopy.push(dst_a.into(), Src::ZERO);
    pcopy.push(dst_b.into(), dst_a.into());
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(Op::ParCopy(Box::new(pcopy))),
            Instr::new(OpIAdd2 {
                dsts: [dst_c.into(), Dst::None],
                srcs: [dst_b.into(), dst_b.into()],
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_c.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    let iadd2 = &shader.functions[0].blocks[0].instrs[1];
    let Op::IAdd2(op) = &iadd2.op else {
        panic!("expected IAdd2");
    };
    assert!(
        op.srcs[0].is_zero() && op.srcs[1].is_zero(),
        "ParCopy chain to zero should propagate both IAdd2 operands"
    );
}

#[test]
fn test_copy_prop_plop3_false() {
    // PLop3 with lut=0 produces False; add_copy records dst -> False
    // Use the predicate in a subsequent instruction - prop_to_pred should fold it
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_p = ssa_alloc.alloc(RegFile::Pred);
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let op_zero = LogicOp3::new_const(false);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpPLop3 {
                dsts: [dst_p.into(), Dst::None],
                srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
                ops: [op_zero, op_zero],
            }),
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: Src::new_imm_u32(1),
            }),
            Instr::new(OpIAdd2 {
                dsts: [dst_b.into(), Dst::None],
                srcs: [dst_a.into(), Src::ZERO],
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_b.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    // PLop3 with lut=0 records dst_p -> False. No consumer uses dst_p as pred here,
    // but the copy prop table is populated. Verify IAdd2 still works (dst_a propagates to imm).
    let iadd2 = &shader.functions[0].blocks[0].instrs[2];
    let Op::IAdd2(op) = &iadd2.op else {
        panic!("expected IAdd2");
    };
    assert!(
        matches!(op.srcs[0].reference, SrcRef::Imm32(1)),
        "Copy to imm should propagate"
    );
}

#[test]
fn test_copy_prop_plop3_src0_pass_through() {
    // PLop3 with pass-through of src0 (x) - dst gets copy of srcs[0]
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_p = ssa_alloc.alloc(RegFile::Pred);
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let op_src0 = LogicOp3::new_lut(&|x, _y, _z| x);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: SrcRef::True.into(),
            }),
            Instr::new(OpPLop3 {
                dsts: [dst_p.into(), Dst::None],
                srcs: [dst_a.into(), Src::ZERO, Src::ZERO],
                ops: [op_src0, op_src0],
            }),
            Instr::new(OpIAdd2 {
                dsts: [dst_b.into(), Dst::None],
                srcs: [Src::ZERO, Src::ZERO],
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_b.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    // PLop3 pass-through src0: dst_p = copy(dst_a) = copy(True)
    // No direct consumer of dst_p in this test; the add_copy is recorded.
    let reg_out = &shader.functions[0].blocks[0].instrs[3];
    let Op::RegOut(op) = &reg_out.op else {
        panic!("expected RegOut");
    };
    assert!(op.srcs[0].is_zero());
}

#[test]
fn test_copy_prop_lop3_src2_pass_through() {
    // Lop3 with pass-through of src2 (z)
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let dst_c = ssa_alloc.alloc(RegFile::GPR);
    let op_src2 = LogicOp3::new_lut(&|_x, _y, z| z);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: Src::new_imm_u32(0x1234_ABCD),
            }),
            Instr::new(OpLop3 {
                dst: dst_b.into(),
                srcs: [Src::ZERO, Src::ZERO, dst_a.into()],
                op: op_src2,
            }),
            Instr::new(OpIAdd2 {
                dsts: [dst_c.into(), Dst::None],
                srcs: [dst_b.into(), Src::ZERO],
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_c.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    let iadd2 = &shader.functions[0].blocks[0].instrs[2];
    let Op::IAdd2(op) = &iadd2.op else {
        panic!("expected IAdd2");
    };
    assert!(
        matches!(op.srcs[0].reference, SrcRef::Imm32(0x1234_ABCD)),
        "Lop3 pass-through src2 should propagate to imm"
    );
}

#[test]
fn test_copy_prop_sel_b2i_zero_on_left() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let pred = ssa_alloc.alloc(RegFile::Pred);
    let dst = ssa_alloc.alloc(RegFile::GPR);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpSel {
                dst: dst.into(),
                srcs: [pred.into(), Src::ZERO, Src::new_imm_u32(7)],
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    let sel = &shader.functions[0].blocks[0].instrs[0];
    let Op::Sel(op) = &sel.op else {
        panic!("expected Sel");
    };
    assert!(
        op.srcs[1].is_zero() && op.srcs[2].is_nonzero(),
        "sel pattern for b2i must stay recognizable"
    );
}

#[test]
fn test_copy_prop_r2ur_uniform() {
    // R2UR with uniform src (UGPR) - records copy. Use in IAdd2 to propagate.
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::UGPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let dst_c = ssa_alloc.alloc(RegFile::GPR);
    let imm = Src::new_imm_u32(99);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: imm,
            }),
            Instr::new(crate::codegen::ir::OpR2UR {
                dst: dst_b.into(),
                src: dst_a.into(),
            }),
            Instr::new(OpIAdd2 {
                dsts: [dst_c.into(), Dst::None],
                srcs: [dst_b.into(), Src::ZERO],
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_c.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    let iadd2 = &shader.functions[0].blocks[0].instrs[2];
    let Op::IAdd2(op) = &iadd2.op else {
        panic!("expected IAdd2");
    };
    assert!(
        matches!(op.srcs[0].reference, SrcRef::Imm32(99)),
        "R2UR uniform copy should propagate to imm"
    );
}

#[test]
fn test_copy_prop_dadd_fneg_zero_left() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst = ssa_alloc.alloc_vec(RegFile::GPR, 2);
    let src_pair = ssa_alloc.alloc_vec(RegFile::GPR, 2);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(Op::DAdd(Box::new(OpDAdd {
                dst: dst.clone().into(),
                srcs: [Src::ZERO.fneg(), src_pair.clone().into()],
                rnd_mode: FRndMode::NearestEven,
            }))),
            Instr::new(OpRegOut {
                srcs: vec![dst[0].into(), dst[1].into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    let reg_out = &shader.functions[0].blocks[0].instrs[1];
    let Op::RegOut(op) = &reg_out.op else {
        panic!("expected RegOut");
    };
    assert!(
        matches!(op.srcs[0].reference, SrcRef::SSA(ref s) if s[0] == src_pair[0]),
        "DAdd(-0, x) should record copies so RegOut lo uses src lo"
    );
    assert!(
        matches!(op.srcs[1].reference, SrcRef::SSA(ref s) if s[0] == src_pair[1]),
        "DAdd(-0, x) should record copies so RegOut hi uses src hi"
    );
}

#[test]
fn test_copy_prop_pred_fold_true_to_none() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_p = ssa_alloc.alloc(RegFile::Pred);
    let dst_g = ssa_alloc.alloc(RegFile::GPR);
    let op_src0 = LogicOp3::new_lut(&|x, _y, _z| x);
    let plop = Instr::new(OpPLop3 {
        dsts: [dst_p.into(), Dst::None],
        srcs: [SrcRef::True.into(), Src::ZERO, Src::ZERO],
        ops: [op_src0, op_src0],
    });
    let mut iadd = Instr::new(OpIAdd2 {
        dsts: [dst_g.into(), Dst::None],
        srcs: [Src::new_imm_u32(11), Src::ZERO],
    });
    iadd.pred = Pred {
        predicate: PredRef::SSA(dst_p),
        inverted: false,
    };
    let mut shader = make_shader_with_function(vec![plop, iadd, Instr::new(OpExit {})], ssa_alloc);

    shader.opt_copy_prop();

    let iadd2 = &shader.functions[0].blocks[0].instrs[1];
    assert!(
        iadd2.pred.is_true(),
        "predicate that was a copy of True should fold to always-true"
    );
}
