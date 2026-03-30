// SPDX-License-Identifier: AGPL-3.0-only
//! Multi-block CFG and phi-node spill tests.

use super::*;
use crate::codegen::ir::{Dst, OpBra, OpPhiDsts, OpPhiSrcs};

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

/// Three-way merge (phi from three predecessors) exercises `phi_dst_maps` / `phi_src_maps` merge paths.
///
/// Loop-header W/S heuristics (`spiller.rs` loop blocks) are exercised by integration tests such as
/// `coverage_spill_loop_with_high_pressure` in `tests/codegen_coverage_extended.rs`: hand-built cyclic
/// CFG fixtures here tend to diverge in `repair_ssa` after `spill_values`, so we rely on the compiler
/// pipeline for that path.
#[test]
fn test_spill_values_three_way_merge_phi() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let mut phi_alloc = PhiAllocator::new();
    let mut label_alloc = LabelAllocator::new();

    let phi = phi_alloc.alloc();
    let merged = ssa_alloc.alloc(RegFile::GPR);
    let merge_label = label_alloc.alloc();
    let a_label = label_alloc.alloc();
    let b_label = label_alloc.alloc();
    let c_label = label_alloc.alloc();

    let va = ssa_alloc.alloc(RegFile::GPR);
    let vb = ssa_alloc.alloc(RegFile::GPR);
    let vc = ssa_alloc.alloc(RegFile::GPR);

    let entry_instrs = vec![
        Instr::new(OpCopy {
            dst: ssa_alloc.alloc(RegFile::Pred).into(),
            src: Src::ZERO,
        }),
        Instr::new(OpBra {
            target: a_label,
            cond: true.into(),
        }),
    ];

    let a_instrs = vec![
        Instr::new(OpCopy {
            dst: va.into(),
            src: Src::ZERO,
        }),
        Instr::new({
            let mut phi_srcs = OpPhiSrcs::new();
            phi_srcs.srcs.push(phi, Src::from(va));
            phi_srcs
        }),
        Instr::new(OpBra {
            target: merge_label,
            cond: true.into(),
        }),
    ];

    let b_instrs = vec![
        Instr::new(OpCopy {
            dst: vb.into(),
            src: Src::ZERO,
        }),
        Instr::new({
            let mut phi_srcs = OpPhiSrcs::new();
            phi_srcs.srcs.push(phi, Src::from(vb));
            phi_srcs
        }),
        Instr::new(OpBra {
            target: merge_label,
            cond: true.into(),
        }),
    ];

    let c_instrs = vec![
        Instr::new(OpCopy {
            dst: vc.into(),
            src: Src::ZERO,
        }),
        Instr::new({
            let mut phi_srcs = OpPhiSrcs::new();
            phi_srcs.srcs.push(phi, Src::from(vc));
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
        label: a_label,
        uniform: false,
        instrs: a_instrs,
    });
    cfg_builder.add_block(BasicBlock {
        label: b_label,
        uniform: false,
        instrs: b_instrs,
    });
    cfg_builder.add_block(BasicBlock {
        label: c_label,
        uniform: false,
        instrs: c_instrs,
    });
    cfg_builder.add_block(BasicBlock {
        label: merge_label,
        uniform: false,
        instrs: merge_instrs,
    });
    cfg_builder.add_edge(0, 1);
    cfg_builder.add_edge(0, 2);
    cfg_builder.add_edge(0, 3);
    cfg_builder.add_edge(1, 4);
    cfg_builder.add_edge(2, 4);
    cfg_builder.add_edge(3, 4);

    let mut func = Function {
        ssa_alloc,
        phi_alloc,
        blocks: cfg_builder.build(),
    };
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::GPR, LIMIT_TWO_GPR, &mut info)
        .unwrap();
    assert_eq!(func.blocks.len(), 5);
}
