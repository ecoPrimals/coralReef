// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Mel Henning (2023)

use super::ir::*;

use coral_reef_stubs::cfg::CFGBuilder;
use coral_reef_stubs::fxhash::FxHashMap;

fn clone_branch(op: &Op) -> Op {
    match op {
        Op::Bra(b) => {
            assert!(b.cond.is_true());
            Op::Bra(b.clone())
        }
        Op::Exit(e) => Op::Exit(e.clone()),
        _ => crate::codegen::ice!("clone_branch: expected Bra or Exit, got unexpected op"),
    }
}

fn jump_thread(func: &mut Function) -> bool {
    // Let's call a basic block "trivial" if its only instruction is an
    // unconditional branch. If a block is trivial, we can update all of its
    // predecessors to jump to its sucessor.
    //
    // A single reverse pass over the basic blocks is enough to update all of
    // the edges we're interested in. Roughly, if we assume that all loops in
    // the shader can terminate, then loop heads are never trivial and we
    // never replace a backward edge. Therefore, in each step we only need to
    // make sure that later control flow has been replaced in order to update
    // the current block as much as possible.
    //
    // We additionally try to update a branch-to-empty-block to point to the
    // block's successor, which along with block dce/reordering can sometimes
    // enable a later optimization that converts branches to fallthrough.
    let mut progress = false;

    // A branch to label can be replaced with Op
    let mut replacements: FxHashMap<Label, Op> = FxHashMap::default();

    // Invariant 1: At the end of each loop iteration,
    //              every trivial block with an index in [i, blocks.len())
    //              is represented in replacements.keys()
    // Invariant 2: replacements.values() never contains
    //              a branch to a trivial block
    for i in (0..func.blocks.len()).rev() {
        // Replace the branch if possible
        if let Some(instr) = func.blocks[i].instrs.last_mut() {
            if let Op::Bra(bra) = &mut instr.op {
                if let Some(replacement) = replacements.get(&bra.target) {
                    if bra.cond.is_true() {
                        // Unconditional branches can just be replaced
                        instr.op = clone_branch(replacement);
                        progress = true;
                    } else {
                        // OpExit has a form that takes an input predicate but it
                        // doesn't support upred so there's nothing we can do here.
                        // EVOLUTION(feature): Jump threading for OpBra with non-uniform predicate.
                        if let Op::Bra(replacement) = replacement {
                            bra.target = replacement.target;
                            progress = true;
                        }
                    }
                }
                // If the branch target was previously a trivial block then the
                // branch was previously a forward edge (see above) and by
                // invariants 1 and 2 we just updated the branch to target
                // a nontrivial block
            }
        }

        // Is this block trivial?
        let block_label = func.blocks[i].label;
        match &func.blocks[i].instrs[..] {
            [instr] => {
                if instr.is_branch_always_taken() {
                    // Upholds invariant 2 because we updated the branch above
                    replacements.insert(block_label, clone_branch(&instr.op));
                }
            }
            [] if i + 1 < func.blocks.len() => {
                // Empty block - falls through
                // Our successor might be trivial, so we need to
                // apply the rewrite map to uphold invariant 2
                let target_label = func.blocks[i + 1].label;
                let replacement = replacements.get(&target_label).map_or_else(
                    || {
                        Op::Bra(
                            OpBra {
                                target: target_label,
                                cond: true.into(),
                            }
                            .into(),
                        )
                    },
                    clone_branch,
                );
                replacements.insert(block_label, replacement);
            }
            _ => (),
        }
    }

    if progress {
        // We don't update the CFG above, so rewrite it if we made progress
        rewrite_cfg(func);
    }

    progress
}

fn rewrite_cfg(func: &mut Function) {
    // CFGBuilder takes care of removing dead blocks for us.
    // Build label->index map for edge resolution.
    let label_to_idx: FxHashMap<Label, usize> = func
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (b.label, i))
        .collect();

    let mut builder = CFGBuilder::<super::ir::BasicBlock>::new();

    for i in 0..func.blocks.len() {
        let block = &func.blocks[i];
        // Note: fall-through must be first edge
        if block.falls_through() {
            builder.add_edge(i, i + 1);
        }
        if let Some(control_flow) = block.branch() {
            match &control_flow.op {
                Op::Bra(bra) => {
                    if let Some(&to) = label_to_idx.get(&bra.target) {
                        builder.add_edge(i, to);
                    }
                }
                Op::Exit(_) => (),
                _ => crate::codegen::ice!("CFG branch must be Bra or Exit"),
            }
        }
    }

    for block in func.blocks.drain() {
        builder.add_node(block);
    }
    let _ = std::mem::replace(&mut func.blocks, builder.as_cfg());
}

/// Replace jumps to the following block with fall-through
fn opt_fall_through(func: &mut Function) {
    for i in 0..func.blocks.len() - 1 {
        let remove_last_instr = match func.blocks[i].branch() {
            Some(b) => match &b.op {
                Op::Bra(bra) => bra.target == func.blocks[i + 1].label,
                _ => false,
            },
            None => false,
        };

        if remove_last_instr {
            func.blocks[i].instrs.pop();
        }
    }
}

impl Function {
    pub fn opt_jump_thread(&mut self) {
        if jump_thread(self) {
            opt_fall_through(self);
        }
    }
}

impl Shader<'_> {
    /// A simple jump threading pass
    ///
    /// Note that this can introduce critical edges, so it cannot be run before RA
    pub fn opt_jump_thread(&mut self) {
        for f in &mut self.functions {
            f.opt_jump_thread();
        }
    }
}
