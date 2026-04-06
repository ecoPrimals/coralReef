// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use crate::codegen::ir::{
    BasicBlock, Function, Instr, LabelAllocator, OpCopy, OpExit, OpRegOut, PhiAllocator, RegFile,
    SSAValueAllocator, Src,
};
use coral_reef_stubs::cfg::CFGBuilder;

fn make_function(instrs: Vec<Instr>, ssa_alloc: SSAValueAllocator) -> Function {
    let mut label_alloc = LabelAllocator::new();
    let mut cfg_builder = CFGBuilder::new();
    let block = BasicBlock {
        label: label_alloc.alloc(),
        uniform: false,
        instrs,
    };
    cfg_builder.add_block(block);
    Function {
        ssa_alloc,
        phi_alloc: PhiAllocator::new(),
        blocks: cfg_builder.build(),
    }
}

#[test]
fn test_live_set_insert_remove_contains() {
    let mut alloc = SSAValueAllocator::new();
    let a = alloc.alloc(RegFile::GPR);
    let b = alloc.alloc(RegFile::GPR);

    let mut live = LiveSet::new();
    assert!(!live.contains(&a));
    assert!(live.insert(a));
    assert!(live.contains(&a));
    assert!(!live.insert(a));
    assert_eq!(live.count(RegFile::GPR), 1);

    live.insert(b);
    assert_eq!(live.count(RegFile::GPR), 2);
    assert!(live.remove(&a));
    assert!(!live.contains(&a));
    assert_eq!(live.count(RegFile::GPR), 1);
}

#[test]
fn test_live_set_from_iter() {
    let mut alloc = SSAValueAllocator::new();
    let a = alloc.alloc(RegFile::GPR);
    let b = alloc.alloc(RegFile::GPR);
    let live: LiveSet = [a, b].into_iter().collect();
    assert!(live.contains(&a));
    assert!(live.contains(&b));
    assert_eq!(live.count(RegFile::GPR), 2);
}

#[test]
fn test_simple_liveness_single_block() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let function = make_function(
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

    let liveness = SimpleLiveness::for_function(&function);
    assert_eq!(liveness.blocks.len(), 1);

    let (bi, ip) = liveness.def_block_ip(&dst_a);
    assert_eq!(bi, 0);
    assert_eq!(ip, 0);

    let (bi, ip) = liveness.def_block_ip(&dst_b);
    assert_eq!(bi, 0);
    assert_eq!(ip, 1);
}

#[test]
fn test_simple_liveness_interferes() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let function = make_function(
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
                srcs: vec![dst_a.into(), dst_b.into()],
            }),
            Instr::new(OpExit {}),
        ],
        ssa_alloc,
    );

    let liveness = SimpleLiveness::for_function(&function);
    assert!(liveness.interferes(&dst_a, &dst_b));
}

#[test]
fn test_calc_max_live() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let dst_a = ssa_alloc.alloc(RegFile::GPR);
    let dst_b = ssa_alloc.alloc(RegFile::GPR);
    let function = make_function(
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

    let liveness = SimpleLiveness::for_function(&function);
    let max_live = liveness.calc_max_live(&function);
    assert!(max_live[RegFile::GPR] >= 1);
}
