// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

use crate::codegen::ir::LogicOp3;
use crate::codegen::ir::{
    BasicBlock, ComputeShaderInfo, Dst, FRndMode, Function, Instr, LabelAllocator, Op, OpCopy,
    OpExit, OpFAdd, OpHAdd2, OpIAdd2, OpIAdd3, OpLop3, OpPrmt, OpRegOut, PhiAllocator, PrmtMode,
    RegFile, SSAValueAllocator, Shader, ShaderInfo, ShaderIoInfo, ShaderModelInfo, ShaderStageInfo,
    Src, SrcRef, SrcType,
};
use coral_reef_stubs::cfg::CFGBuilder;

fn make_shader_with_function(instrs: Vec<Instr>, ssa_alloc: SSAValueAllocator) -> Shader<'static> {
    let sm = Box::leak(Box::new(ShaderModelInfo::new(70, 64)));
    let mut label_alloc = LabelAllocator::new();
    let mut cfg_builder = CFGBuilder::new();
    let block = BasicBlock {
        label: label_alloc.alloc(),
        uniform: false,
        instrs,
    };
    cfg_builder.add_block(block);
    let function = Function {
        ssa_alloc,
        phi_alloc: PhiAllocator::new(),
        blocks: cfg_builder.build(),
    };
    Shader {
        sm,
        info: ShaderInfo {
            max_warps_per_sm: 0,
            gpr_count: 0,
            control_barrier_count: 0,
            instr_count: 0,
            static_cycle_count: 0,
            spills_to_mem: 0,
            fills_from_mem: 0,
            spills_to_reg: 0,
            fills_from_reg: 0,
            shared_local_mem_size: 0,
            max_crs_depth: 0,
            uses_global_mem: false,
            writes_global_mem: false,
            uses_fp64: false,
            stage: ShaderStageInfo::Compute(ComputeShaderInfo {
                local_size: [1, 1, 1],
                shared_mem_size: 0,
            }),
            io: ShaderIoInfo::None,
        },
        functions: vec![function],
    }
}

#[test]
fn test_copy_prop_propagates_copy() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: Src::ZERO,
            }),
            Instr::new(OpCopy {
                dst: dst_b.into(),
                src: dst_a.into(),
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_b.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    let reg_out = &shader.functions[0].blocks[0].instrs[2];
    let Op::RegOut(op) = &reg_out.op else {
        panic!("expected RegOut");
    };
    assert!(op.srcs[0].is_zero(), "copy should be propagated to zero");
}

#[test]
fn test_copy_prop_chain() {
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
            Instr::new(OpCopy {
                dst: dst_b.into(),
                src: dst_a.into(),
            }),
            Instr::new(OpCopy {
                dst: dst_c.into(),
                src: dst_b.into(),
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_c.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    let reg_out = &shader.functions[0].blocks[0].instrs[3];
    let Op::RegOut(op) = &reg_out.op else {
        panic!("expected RegOut");
    };
    assert!(
        op.srcs[0].is_zero(),
        "chain of copies should propagate to zero"
    );
}

#[test]
fn test_copy_prop_iadd2_zero() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: Src::ZERO,
            }),
            Instr::new(OpIAdd2 {
                dst: dst_b.into(),
                carry_out: Dst::None,
                srcs: [dst_a.into(), dst_a.into()],
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_b.into()],
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
    assert!(op.srcs[0].is_zero(), "0 + x should propagate to x");
}

#[test]
fn test_copy_prop_iadd3_two_zeros() {
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
                dst: dst_b.into(),
                overflow: [Dst::None, Dst::None],
                srcs: [dst_a.into(), dst_a.into(), dst_c.into()],
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
    assert!(op.srcs[0].is_zero() && op.srcs[1].is_zero());
}

#[test]
fn test_copy_prop_fadd_fneg_zero() {
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
            Instr::new(OpFAdd {
                dst: dst_b.into(),
                srcs: [dst_a.into(), dst_c.into()],
                saturate: false,
                rnd_mode: FRndMode::NearestEven,
                ftz: false,
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_b.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    let fadd = &shader.functions[0].blocks[0].instrs[1];
    let Op::FAdd(op) = &fadd.op else {
        panic!("expected FAdd");
    };
    assert!(op.srcs[0].is_zero(), "0.0 + x should propagate");
}

#[test]
fn test_copy_prop_chain_to_imm32_in_iadd2() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let dst_c = ssa_alloc.alloc(RegFile::GPR);
    let imm = Src::new_imm_u32(42);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: imm,
            }),
            Instr::new(OpCopy {
                dst: dst_b.into(),
                src: dst_a.into(),
            }),
            Instr::new(OpIAdd2 {
                dst: dst_c.into(),
                carry_out: Dst::None,
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

    let iadd2 = &shader.functions[0].blocks[0].instrs[2];
    let Op::IAdd2(op) = &iadd2.op else {
        panic!("expected IAdd2");
    };
    assert!(
        matches!(op.srcs[0].reference, SrcRef::Imm32(_)),
        "src0 should propagate"
    );
    assert!(
        matches!(op.srcs[1].reference, SrcRef::Imm32(_)),
        "src1 should propagate"
    );
}

#[test]
fn test_copy_prop_lop3_zero() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let dst_c = ssa_alloc.alloc(RegFile::GPR);
    let op_zero = LogicOp3::new_const(false);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpLop3 {
                dst: dst_a.into(),
                srcs: [dst_b.into(), dst_c.into(), Src::ZERO],
                op: op_zero,
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_a.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    // Lop3 with lut=0 produces zero; copy prop records dst_a -> Zero.
    // The RegOut consumes dst_a, so it should get Zero propagated.
    let reg_out = &shader.functions[0].blocks[0].instrs[1];
    let Op::RegOut(op) = &reg_out.op else {
        panic!("expected RegOut");
    };
    assert!(
        op.srcs[0].is_zero(),
        "Lop3 with lut=0 produces zero; RegOut should receive propagated Zero"
    );
}

#[test]
fn test_copy_prop_hadd2_fneg_zero() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let dst_c = ssa_alloc.alloc(RegFile::GPR);
    let zero_f16 = Src::ZERO;
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: zero_f16,
            }),
            Instr::new(OpHAdd2 {
                dst: dst_b.into(),
                srcs: [Src::from(dst_a).fneg(), dst_c.into()],
                saturate: false,
                ftz: false,
                f32: false,
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_b.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    let hadd2 = &shader.functions[0].blocks[0].instrs[1];
    let Op::HAdd2(op) = &hadd2.op else {
        panic!("expected HAdd2");
    };
    assert!(
        op.srcs[0].is_fneg_zero(SrcType::F16v2),
        "-0 + x should be recognized for copy prop"
    );
}

#[test]
fn test_copy_prop_lop3_src0_pass_through() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let dst_c = ssa_alloc.alloc(RegFile::GPR);
    let op_src0 = LogicOp3::new_lut(&|x, _y, _z| x);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: Src::new_imm_u32(0x1234_5678),
            }),
            Instr::new(OpLop3 {
                dst: dst_b.into(),
                srcs: [dst_a.into(), dst_c.into(), Src::ZERO],
                op: op_src0,
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
        matches!(op.srcs[0].reference, SrcRef::Imm32(0x1234_5678)),
        "Lop3 pass-through src0 should propagate to imm"
    );
}

#[test]
fn test_copy_prop_fadd_zero_right() {
    // FAdd(x, 0) → x: zero on right operand
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
            Instr::new(OpFAdd {
                dst: dst_b.into(),
                srcs: [dst_c.into(), dst_a.into()],
                saturate: false,
                rnd_mode: FRndMode::NearestEven,
                ftz: false,
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_b.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    let fadd = &shader.functions[0].blocks[0].instrs[1];
    let Op::FAdd(op) = &fadd.op else {
        panic!("expected FAdd");
    };
    assert!(
        op.srcs[1].is_zero(),
        "x + 0.0 should propagate right operand"
    );
}

#[test]
fn test_copy_prop_chain_ssa_to_imm() {
    // Copy chain: a = imm, b = a, c = b; use c in IAdd2 - should propagate to imm
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let dst_c = ssa_alloc.alloc(RegFile::GPR);
    let dst_d = ssa_alloc.alloc(RegFile::GPR);
    let imm = Src::new_imm_u32(100);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: imm,
            }),
            Instr::new(OpCopy {
                dst: dst_b.into(),
                src: dst_a.into(),
            }),
            Instr::new(OpCopy {
                dst: dst_c.into(),
                src: dst_b.into(),
            }),
            Instr::new(OpIAdd2 {
                dst: dst_d.into(),
                carry_out: Dst::None,
                srcs: [dst_c.into(), dst_c.into()],
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_d.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    shader.opt_copy_prop();

    let iadd2 = &shader.functions[0].blocks[0].instrs[3];
    let Op::IAdd2(op) = &iadd2.op else {
        panic!("expected IAdd2");
    };
    assert!(
        matches!(op.srcs[0].reference, SrcRef::Imm32(100)),
        "copy chain should propagate to imm"
    );
    assert!(
        matches!(op.srcs[1].reference, SrcRef::Imm32(100)),
        "copy chain should propagate to imm"
    );
}

#[test]
fn test_copy_prop_prmt_sel_3210() {
    // PRMT with sel=0x3210 (identity from src0) - dst is copy of srcs[0]
    // Use IAdd2 to consume PRMT output so prop_to_scalar_src can propagate to imm
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let dst_c = ssa_alloc.alloc(RegFile::GPR);
    let imm = Src::new_imm_u32(0xDEAD_BEEF);
    let mut shader = make_shader_with_function(
        vec![
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: imm,
            }),
            Instr::new(OpPrmt {
                dst: dst_b.into(),
                srcs: [dst_a.into(), Src::ZERO],
                sel: Src::new_imm_u32(0x3210),
                mode: PrmtMode::Index,
            }),
            Instr::new(OpIAdd2 {
                dst: dst_c.into(),
                carry_out: Dst::None,
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
        matches!(op.srcs[0].reference, SrcRef::Imm32(0xDEAD_BEEF)),
        "PRMT sel=0x3210 (identity from src0) should propagate to src0"
    );
}

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
                srcs: [Src::ZERO, dst_a.into()],
                sel: Src::new_imm_u32(0x7654),
                mode: PrmtMode::Index,
            }),
            Instr::new(OpIAdd2 {
                dst: dst_c.into(),
                carry_out: Dst::None,
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
                dst: dst_b.into(),
                overflow: [Dst::None, Dst::None],
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
                dst: dst_b.into(),
                overflow: [Dst::None, Dst::None],
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
                dst: dst_d.into(),
                carry_out: Dst::None,
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
