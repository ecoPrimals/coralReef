// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT

use super::*;
use crate::nak::ir::{
    BasicBlock, ComputeShaderInfo, Dst, Function, FRndMode, Instr, LabelAllocator, Op, OpCopy,
    OpExit, OpFAdd, OpIAdd2, OpIAdd3, OpRegOut, PhiAllocator, RegFile, Shader, ShaderInfo,
    ShaderIoInfo, ShaderModelInfo, ShaderStageInfo, Src, SrcRef, SSAValueAllocator,
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
            num_gprs: 0,
            num_control_barriers: 0,
            num_instrs: 0,
            num_static_cycles: 0,
            num_spills_to_mem: 0,
            num_fills_from_mem: 0,
            num_spills_to_reg: 0,
            num_fills_from_reg: 0,
            slm_size: 0,
            max_crs_depth: 0,
            uses_global_mem: false,
            writes_global_mem: false,
            uses_fp64: false,
            stage: ShaderStageInfo::Compute(ComputeShaderInfo {
                local_size: [1, 1, 1],
                smem_size: 0,
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
    assert!(op.srcs[0].is_zero(), "chain of copies should propagate to zero");
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
    assert!(matches!(op.srcs[0].src_ref, SrcRef::Imm32(_)), "src0 should propagate");
    assert!(matches!(op.srcs[1].src_ref, SrcRef::Imm32(_)), "src1 should propagate");
}
