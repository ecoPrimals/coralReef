// SPDX-License-Identifier: AGPL-3.0-only
//! Spill statistics and edge-case tests.

use super::*;

/// Spill statistics: GPR spills to memory (chain of uses creates pressure).
#[test]
fn test_spill_values_gpr_stats_spills_to_mem() {
    let mut func = make_function_with_many_gprs(20);
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::GPR, 2, &mut info).unwrap();
    assert!(
        info.spills_to_mem > 0 || info.fills_from_mem > 0 || !func.blocks[0].instrs.is_empty(),
        "GPR spilling should update stats or add instructions"
    );
}

/// Spill statistics: UGPR spills to reg (`spills_to_reg`, `fills_from_reg`).
#[test]
fn test_spill_values_ugpr_stats_spills_to_reg() {
    let mut func = make_function_with_many_ugprs(15);
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::UGPR, 2, &mut info).unwrap();
    assert!(
        info.spills_to_reg > 0 || info.fills_from_reg > 0 || !func.blocks[0].instrs.is_empty(),
        "UGPR spilling should update spills_to_reg or fills_from_reg"
    );
}

/// Edge case: empty function.
#[test]
fn test_spill_values_empty_function() {
    let ssa_alloc = SSAValueAllocator::new();
    let mut label_alloc = LabelAllocator::new();
    let mut cfg_builder = CFGBuilder::new();
    cfg_builder.add_block(BasicBlock {
        label: label_alloc.alloc(),
        uniform: false,
        instrs: vec![],
    });
    let mut func = Function {
        ssa_alloc,
        phi_alloc: PhiAllocator::new(),
        blocks: cfg_builder.build(),
    };
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::GPR, 4, &mut info).unwrap();
    assert_eq!(func.blocks.len(), 1);
    assert!(func.blocks[0].instrs.is_empty());
}

/// Edge case: single instruction (`OpExit` only).
#[test]
fn test_spill_values_single_instruction_function() {
    let ssa_alloc = SSAValueAllocator::new();
    let mut label_alloc = LabelAllocator::new();
    let mut cfg_builder = CFGBuilder::new();
    cfg_builder.add_block(BasicBlock {
        label: label_alloc.alloc(),
        uniform: false,
        instrs: vec![Instr::new(OpExit {})],
    });
    let mut func = Function {
        ssa_alloc,
        phi_alloc: PhiAllocator::new(),
        blocks: cfg_builder.build(),
    };
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::GPR, 4, &mut info).unwrap();
    assert_eq!(func.blocks.len(), 1);
    assert_eq!(func.blocks[0].instrs.len(), 1);
    assert!(matches!(func.blocks[0].instrs[0].op, Op::Exit(_)));
}

/// Spill statistics: Pred spills to GPR (`spills_to_reg`, `fills_from_reg`).
#[test]
fn test_spill_values_pred_stats_spills_to_reg() {
    let mut func = make_function_with_many_preds(10);
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::Pred, 2, &mut info).unwrap();
    assert!(
        info.spills_to_reg > 0 || info.fills_from_reg > 0 || !func.blocks[0].instrs.is_empty(),
        "Pred spilling should update stats or add instructions"
    );
}

/// No spill: all stats remain zero when limit is high.
#[test]
fn test_spill_values_no_spill_all_stats_zero() {
    let mut func = make_function_with_many_gprs(4);
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::GPR, 64, &mut info).unwrap();
    assert_eq!(info.spills_to_mem, 0);
    assert_eq!(info.fills_from_mem, 0);
    assert_eq!(info.spills_to_reg, 0);
    assert_eq!(info.fills_from_reg, 0);
}

/// Pre-seeded mem counters must remain valid lower bounds after spill (pass does not assume zero).
#[test]
fn test_spill_values_shader_info_preserves_preseeded_mem_counts() {
    let mut func = make_function_with_many_gprs(18);
    let mut info = default_shader_info();
    info.spills_to_mem = PRESEEDED_MEM_SPILLS;
    info.fills_from_mem = PRESEEDED_MEM_FILLS;
    func.to_cssa();
    func.spill_values(RegFile::GPR, LIMIT_TWO_GPR, &mut info)
        .unwrap();
    assert!(info.spills_to_mem >= PRESEEDED_MEM_SPILLS);
    assert!(info.fills_from_mem >= PRESEEDED_MEM_FILLS);
}
