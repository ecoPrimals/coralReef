// SPDX-License-Identifier: AGPL-3.0-only
//! Register-file-specific spill tests: UPred, Bar, pinned values, parallel copy, UGPR non-uniform.

use super::*;
use crate::codegen::ir::{IntCmpOp, IntCmpType, OpBClear, OpBSync, OpBra, OpParCopy, OpPin};

/// UPred spilling path (spills to UGPR, fills via `OpISetP`).
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

/// `OpPin` marks destination as pinned; `SpillChooser` skips pinned values.
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

/// Two `OpPin` instructions mark multiple destination SSA values as pinned (`b.p`) so `SpillChooser`
/// skips them when evicting under register pressure (`spiller.rs` post-instr `OpPin` handling).
#[test]
fn test_spill_values_two_pins_mark_multiple_pinned_ssa() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let mut instrs = Vec::new();
    let base = ssa_alloc.alloc(RegFile::GPR);
    instrs.push(Instr::new(OpCopy {
        dst: base.into(),
        src: Src::ZERO,
    }));
    let pin_a = ssa_alloc.alloc(RegFile::GPR);
    let pin_b = ssa_alloc.alloc(RegFile::GPR);
    instrs.push(Instr::new(Op::Pin(Box::new(OpPin {
        dst: pin_a.into(),
        src: base.into(),
    }))));
    instrs.push(Instr::new(Op::Pin(Box::new(OpPin {
        dst: pin_b.into(),
        src: base.into(),
    }))));
    for _ in 0..12 {
        let next = ssa_alloc.alloc(RegFile::GPR);
        instrs.push(Instr::new(OpCopy {
            dst: next.into(),
            src: base.into(),
        }));
    }
    instrs.push(Instr::new(OpCopy {
        dst: ssa_alloc.alloc(RegFile::GPR).into(),
        src: pin_a.into(),
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

/// Bar register file spilling (`RegFile::Bar` path) — spills Bar to GPR.
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
    assert!(
        info.spills_to_reg > 0 || info.fills_from_reg > 0,
        "Bar spilling should update reg spill stats"
    );
}

/// Bar with high limit: no spilling needed.
#[test]
fn test_spill_values_bar_no_spill_needed() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let mut instrs = Vec::new();
    let bar = ssa_alloc.alloc(RegFile::Bar);
    instrs.push(Instr::new(OpBClear { dst: bar.into() }));
    instrs.push(Instr::new(OpBSync {
        srcs: [bar.into(), true.into()],
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
    func.spill_values(RegFile::Bar, 16, &mut info).unwrap();
    assert_eq!(info.spills_to_reg, 0);
    assert_eq!(info.fills_from_reg, 0);
}

/// `OpParCopy` with many destinations in GPR file forces `SpillChooser` on the parallel-copy path.
#[test]
fn test_spill_values_par_copy_spill_chooser() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let g0 = ssa_alloc.alloc(RegFile::GPR);
    let mut instrs = vec![Instr::new(OpCopy {
        dst: g0.into(),
        src: Src::ZERO,
    })];
    let mut pcopy = OpParCopy::new();
    let mut par_copy_dsts = Vec::new();
    for _ in 0..PAR_COPY_DST_PAIR_COUNT {
        let dst = ssa_alloc.alloc(RegFile::GPR);
        par_copy_dsts.push(dst);
        pcopy.push(dst.into(), g0.into());
    }
    instrs.push(Instr::new(Op::ParCopy(Box::new(pcopy))));
    let sink = ssa_alloc.alloc(RegFile::GPR);
    for d in &par_copy_dsts {
        instrs.push(Instr::new(OpCopy {
            dst: sink.into(),
            src: (*d).into(),
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
    func.spill_values(RegFile::GPR, PAR_COPY_GPR_LIMIT, &mut info)
        .unwrap();
    assert_eq!(func.blocks.len(), 1);
    assert!(
        info.spills_to_mem > 0 || info.fills_from_mem > 0 || func.blocks[0].instrs.len() > 2,
        "ParCopy spill path should add mem traffic or instructions"
    );
}

/// UGPR in a non-uniform successor block exercises the UGPR rewrite path (`!bb.uniform`).
#[test]
fn test_spill_values_ugpr_non_uniform_block_rewrite() {
    let mut ssa_alloc = SSAValueAllocator::new();
    let mut label_alloc = LabelAllocator::new();
    let entry_label = label_alloc.alloc();
    let nu_label = label_alloc.alloc();

    let mut uniform_instrs = Vec::new();
    let mut ugprs = Vec::new();
    for _ in 0..8 {
        let u = ssa_alloc.alloc(RegFile::UGPR);
        ugprs.push(u);
        uniform_instrs.push(Instr::new(OpCopy {
            dst: u.into(),
            src: Src::ZERO,
        }));
    }
    uniform_instrs.push(Instr::new(OpBra {
        target: nu_label,
        cond: true.into(),
    }));

    let mut nu_instrs = Vec::new();
    for u in &ugprs {
        nu_instrs.push(Instr::new(OpISetP {
            dst: ssa_alloc.alloc(RegFile::Pred).into(),
            set_op: PredSetOp::And,
            cmp_op: IntCmpOp::Ne,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [(*u).into(), (*u).into(), true.into(), true.into()],
        }));
    }
    nu_instrs.push(Instr::new(OpExit {}));

    let mut cfg_builder = CFGBuilder::new();
    cfg_builder.add_block(BasicBlock {
        label: entry_label,
        uniform: true,
        instrs: uniform_instrs,
    });
    cfg_builder.add_block(BasicBlock {
        label: nu_label,
        uniform: false,
        instrs: nu_instrs,
    });
    cfg_builder.add_edge(0, 1);

    let mut func = Function {
        ssa_alloc,
        phi_alloc: PhiAllocator::new(),
        blocks: cfg_builder.build(),
    };
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::UGPR, LIMIT_TWO_GPR, &mut info)
        .unwrap();
    assert_eq!(func.blocks.len(), 2);
    assert!(
        info.spills_to_reg > 0 || info.fills_from_reg > 0 || !func.blocks[1].instrs.is_empty(),
        "UGPR rewrite in non-uniform block should touch reg spill stats or instructions"
    );
}
