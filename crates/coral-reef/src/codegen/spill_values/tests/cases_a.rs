// SPDX-License-Identifier: AGPL-3.0-only

use super::super::*;
use super::fixtures::*;
use crate::codegen::ir::{
    BasicBlock, ComputeShaderInfo, Dst, Instr, IntCmpOp, IntCmpType, LabelAllocator, Op, OpBClear,
    OpBSync, OpBra, OpCopy, OpExit, OpISetP, OpPhiDsts, OpPhiSrcs, OpPin, PhiAllocator, PredSetOp,
    ShaderIoInfo, ShaderStageInfo, Src,
};
use crate::codegen::ssa_value::SSAValueAllocator;
use coral_reef_stubs::cfg::CFGBuilder;

#[test]
fn test_spill_values_ugpr_with_high_pressure() {
    let mut func = make_function_with_many_ugprs(15);
    let mut info = ShaderInfo {
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
    };
    func.to_cssa();
    func.spill_values(RegFile::UGPR, 4, &mut info).unwrap();
    assert!(!func.blocks[0].instrs.is_empty());
}

#[test]
fn test_spill_values_preserves_semantics() {
    let mut func = make_function_with_many_gprs(8);
    let mut info = ShaderInfo {
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
    };
    func.to_cssa();
    func.spill_values(RegFile::GPR, 4, &mut info).unwrap();
    assert!(!func.blocks[0].instrs.is_empty());
    let last = func.blocks[0].instrs.last().unwrap();
    assert!(matches!(last.op, Op::Exit(_)));
}

#[test]
fn test_spill_values_pred_with_high_pressure() {
    let mut func = make_function_with_many_preds(8);
    let mut info = ShaderInfo {
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
    };
    func.to_cssa();
    func.spill_values(RegFile::Pred, 4, &mut info).unwrap();
    assert!(!func.blocks[0].instrs.is_empty());
}
/// Two-block linear CFG (entry -> exit) exercises single-predecessor path in spiller.
#[test]
fn test_spill_values_two_blocks_single_predecessor() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let mut label_alloc = LabelAllocator::new();

    let mut entry_instrs = Vec::new();
    let base = ssa_alloc.alloc(RegFile::GPR);
    entry_instrs.push(Instr::new(OpCopy {
        dst: base.into(),
        src: Src::ZERO,
    }));
    for _ in 1..8 {
        let next = ssa_alloc.alloc(RegFile::GPR);
        entry_instrs.push(Instr::new(OpCopy {
            dst: next.into(),
            src: base.into(),
        }));
    }

    let mut exit_instrs = Vec::new();
    let use_val = ssa_alloc.alloc(RegFile::GPR);
    exit_instrs.push(Instr::new(OpCopy {
        dst: use_val.into(),
        src: base.into(),
    }));
    exit_instrs.push(Instr::new(OpExit {}));

    let mut cfg_builder = CFGBuilder::new();
    cfg_builder.add_block(BasicBlock {
        label: label_alloc.alloc(),
        uniform: false,
        instrs: entry_instrs,
    });
    cfg_builder.add_block(BasicBlock {
        label: label_alloc.alloc(),
        uniform: false,
        instrs: exit_instrs,
    });
    cfg_builder.add_edge(0, 1);

    let mut func = Function {
        ssa_alloc,
        phi_alloc: PhiAllocator::new(),
        blocks: cfg_builder.build(),
    };
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::GPR, 4, &mut info).unwrap();
    assert_eq!(func.blocks.len(), 2);
    assert!(
        !func.blocks[0].instrs.is_empty() || !func.blocks[1].instrs.is_empty(),
        "at least one block should have instructions"
    );
}

/// Very low limit with high pressure exercises spill cost/selection paths.
#[test]
fn test_spill_values_very_low_limit() {
    let mut func = make_function_with_many_gprs(20);
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::GPR, 2, &mut info).unwrap();
    let last = func.blocks[0].instrs.last().unwrap();
    assert!(matches!(last.op, Op::Exit(_)));
}

/// High limit: no spilling needed; exercises early-exit paths.
#[test]
fn test_spill_values_no_spill_needed() {
    let mut func = make_function_with_many_gprs(4);
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::GPR, 64, &mut info).unwrap();
    assert_eq!(info.spills_to_mem, 0);
    assert_eq!(info.fills_from_mem, 0);
}

/// UPred spilling path (spills to UGPR, fills via OpISetP).
#[test]
fn test_spill_values_upred_with_pressure() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let mut instrs = Vec::new();
    let base = ssa_alloc.alloc(RegFile::UGPR);
    instrs.push(Instr::new(OpCopy {
        dst: base.into(),
        src: Src::ZERO,
    }));
    for _ in 0..6 {
        let p = ssa_alloc.alloc(RegFile::UPred);
        instrs.push(Instr::new(OpISetP {
            dst: p.into(),
            set_op: PredSetOp::And,
            cmp_op: IntCmpOp::Ne,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [base.into(), base.into(), true.into(), true.into()],
        }));
    }
    instrs.push(Instr::new(OpExit {}));

    let mut label_alloc = LabelAllocator::new();
    let mut cfg_builder = CFGBuilder::new();
    cfg_builder.add_block(BasicBlock {
        label: label_alloc.alloc(),
        uniform: true,
        instrs,
    });
    let mut func = Function {
        ssa_alloc,
        phi_alloc: PhiAllocator::new(),
        blocks: cfg_builder.build(),
    };
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::UPred, 3, &mut info).unwrap();
    assert!(!func.blocks[0].instrs.is_empty());
}

/// OpPin marks destination as pinned; SpillChooser skips pinned values.
#[test]
fn test_spill_values_with_pinned() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let mut instrs = Vec::new();
    let base = ssa_alloc.alloc(RegFile::GPR);
    instrs.push(Instr::new(OpCopy {
        dst: base.into(),
        src: Src::ZERO,
    }));
    let pinned = ssa_alloc.alloc(RegFile::GPR);
    instrs.push(Instr::new(Op::Pin(Box::new(OpPin {
        dst: pinned.into(),
        src: base.into(),
    }))));
    for _ in 0..10 {
        let next = ssa_alloc.alloc(RegFile::GPR);
        instrs.push(Instr::new(OpCopy {
            dst: next.into(),
            src: base.into(),
        }));
    }
    instrs.push(Instr::new(OpCopy {
        dst: ssa_alloc.alloc(RegFile::GPR).into(),
        src: pinned.into(),
    }));
    instrs.push(Instr::new(OpExit {}));

    let mut label_alloc = LabelAllocator::new();
    let mut cfg_builder = CFGBuilder::new();
    cfg_builder.add_block(BasicBlock {
        label: label_alloc.alloc(),
        uniform: false,
        instrs,
    });
    let mut func = Function {
        ssa_alloc,
        phi_alloc: PhiAllocator::new(),
        blocks: cfg_builder.build(),
    };
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::GPR, 4, &mut info).unwrap();
    assert!(!func.blocks[0].instrs.is_empty());
}

/// Bar register file spilling (RegFile::Bar path) — spills Bar to GPR.
#[test]
fn test_spill_values_bar_with_high_pressure() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let mut instrs = Vec::new();
    let mut bars = Vec::new();
    for _ in 0..8 {
        let bar = ssa_alloc.alloc(RegFile::Bar);
        bars.push(bar);
        instrs.push(Instr::new(OpBClear { dst: bar.into() }));
    }
    for bar in &bars {
        instrs.push(Instr::new(OpBSync {
            srcs: [(*bar).into(), true.into()],
        }));
    }
    instrs.push(Instr::new(OpExit {}));

    let mut label_alloc = LabelAllocator::new();
    let mut cfg_builder = CFGBuilder::new();
    cfg_builder.add_block(BasicBlock {
        label: label_alloc.alloc(),
        uniform: false,
        instrs,
    });
    let mut func = Function {
        ssa_alloc,
        phi_alloc: PhiAllocator::new(),
        blocks: cfg_builder.build(),
    };
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::Bar, 2, &mut info).unwrap();
    assert!(!func.blocks[0].instrs.is_empty());
    // Bar spills to GPR (spills_to_reg, fills_from_reg)
    assert!(
        info.spills_to_reg > 0 || info.fills_from_reg > 0,
        "Bar spilling should update reg spill stats"
    );
}

/// Multi-block function with phi nodes exercises cross-block spilling.
#[test]
fn test_spill_values_multi_block_with_phi_nodes() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let mut phi_alloc = PhiAllocator::new();
    let mut label_alloc = LabelAllocator::new();

    let pred = ssa_alloc.alloc(RegFile::Pred);
    let left_val = ssa_alloc.alloc(RegFile::GPR);
    let right_val = ssa_alloc.alloc(RegFile::GPR);
    let phi = phi_alloc.alloc();
    let merged = ssa_alloc.alloc(RegFile::GPR);
    let merge_label = label_alloc.alloc();
    let then_label = label_alloc.alloc();
    let else_label = label_alloc.alloc();

    let entry_instrs = vec![
        Instr::new(OpCopy {
            dst: pred.into(),
            src: Src::ZERO,
        }),
        Instr::new(OpBra {
            target: then_label,
            cond: pred.into(),
        }),
    ];

    let else_instrs = vec![
        Instr::new(OpCopy {
            dst: right_val.into(),
            src: Src::ZERO,
        }),
        Instr::new({
            let mut phi_srcs = OpPhiSrcs::new();
            phi_srcs.srcs.push(phi, Src::from(right_val));
            phi_srcs
        }),
        Instr::new(OpBra {
            target: merge_label,
            cond: true.into(),
        }),
    ];

    let then_instrs = vec![
        Instr::new(OpCopy {
            dst: left_val.into(),
            src: Src::ZERO,
        }),
        Instr::new({
            let mut phi_srcs = OpPhiSrcs::new();
            phi_srcs.srcs.push(phi, Src::from(left_val));
            phi_srcs
        }),
        Instr::new(OpBra {
            target: merge_label,
            cond: true.into(),
        }),
    ];

    let merge_instrs = vec![
        Instr::new({
            let mut phi_dsts = OpPhiDsts::new();
            phi_dsts.dsts.push(phi, Dst::from(merged));
            phi_dsts
        }),
        Instr::new(OpCopy {
            dst: ssa_alloc.alloc(RegFile::GPR).into(),
            src: merged.into(),
        }),
        Instr::new(OpExit {}),
    ];

    let mut cfg_builder = CFGBuilder::new();
    cfg_builder.add_block(BasicBlock {
        label: label_alloc.alloc(),
        uniform: false,
        instrs: entry_instrs,
    });
    cfg_builder.add_block(BasicBlock {
        label: else_label,
        uniform: false,
        instrs: else_instrs,
    });
    cfg_builder.add_block(BasicBlock {
        label: then_label,
        uniform: false,
        instrs: then_instrs,
    });
    cfg_builder.add_block(BasicBlock {
        label: merge_label,
        uniform: false,
        instrs: merge_instrs,
    });
    cfg_builder.add_edge(0, 1);
    cfg_builder.add_edge(0, 2);
    cfg_builder.add_edge(1, 3);
    cfg_builder.add_edge(2, 3);

    let mut func = Function {
        ssa_alloc,
        phi_alloc,
        blocks: cfg_builder.build(),
    };
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::GPR, 1, &mut info).unwrap();
    assert_eq!(func.blocks.len(), 4);
}
