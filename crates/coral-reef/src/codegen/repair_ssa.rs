// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

use super::ir::*;
use super::union_find::UnionFind;

use coral_reef_stubs::bitset::BitSet;
use coral_reef_stubs::fxhash::{FxBuildHasher, FxHashMap};
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

struct PhiTracker {
    phi: Phi,
    orig: SSAValue,
    dst: SSAValue,
    srcs: FxHashMap<usize, SSAValue>,
}

struct DefTrackerBlock {
    pred: Vec<usize>,
    succ: Vec<usize>,
    defs: RefCell<FxHashMap<SSAValue, SSAValue>>,
    phis: RefCell<Vec<PhiTracker>>,
}

fn get_ssa_or_phi(
    worklist: &mut BinaryHeap<Reverse<usize>>,
    ssa_alloc: &mut SSAValueAllocator,
    phi_alloc: &mut PhiAllocator,
    blocks: &[DefTrackerBlock],
    needs_src: &mut BitSet<Phi>,
    synth_undefs: &mut Vec<(usize, SSAValue)>,
    b_idx: usize,
    ssa: SSAValue,
) -> Result<SSAValue, crate::CompileError> {
    // Annoyingly, Rust stack sizes get to be a problem so we don't want to use
    // actual recursion here.  Instead we use a worklist in the form of a binary
    // heap.  Using a binary heap ensures that we process the earliest blocks
    // first.
    debug_assert!(worklist.is_empty());
    worklist.push(Reverse(b_idx));

    loop {
        let b_idx = worklist.peek().expect("worklist must not be empty").0;
        let b = &blocks[b_idx];

        if let Some(&b_ssa) = b.defs.borrow().get(&ssa) {
            // We already sorted this one out, pop the stack.
            worklist.pop();
            if worklist.is_empty() {
                return Ok(b_ssa);
            } else {
                continue;
            }
        }

        let mut pushed_pred = false;
        let mut pred_ssa = None;
        let mut all_same = true;
        for &p_idx in &b.pred {
            if p_idx >= b_idx {
                // This is a loop back-edge, add a phi just in case.  We'll
                // remove it later if it's not needed
                all_same = false;
            } else if let Some(&p_ssa) = blocks[p_idx].defs.borrow().get(&ssa) {
                if *pred_ssa.get_or_insert(p_ssa) != p_ssa {
                    all_same = false;
                }
            } else {
                worklist.push(Reverse(p_idx));
                pushed_pred = true;
            }
        }

        // If we pushed any predecessors to the stack, loop again to sort them
        // out before we try to sort out this block.
        if pushed_pred {
            continue;
        }

        // We now have everything we need to sort out this block
        let b_ssa = if all_same {
            match pred_ssa {
                Some(v) => v,
                None if b_idx != 0 && b.pred.is_empty() => {
                    let undef_ssa = ssa_alloc.alloc(ssa.file());
                    synth_undefs.push((b_idx, undef_ssa));
                    undef_ssa
                }
                None => {
                    // Entry block reached with no definition — this
                    // should have been handled by fix_entry_live_in.
                    return Err(crate::CompileError::Validation(
                        format!(
                            "Undefined SSA value {ssa:?} at entry — fix_entry_live_in missed it"
                        )
                        .into(),
                    ));
                }
            }
        } else {
            let phi = phi_alloc.alloc();
            let phi_ssa = ssa_alloc.alloc(ssa.file());
            let mut pt = PhiTracker {
                phi,
                orig: ssa,
                dst: phi_ssa,
                srcs: FxHashMap::default(),
            };
            for &p_idx in &b.pred {
                if p_idx >= b_idx {
                    needs_src.insert(p_idx);
                    continue;
                }
                // Earlier iterations of the loop ensured this exists
                let p_ssa = *blocks[p_idx]
                    .defs
                    .borrow()
                    .get(&ssa)
                    .expect("predecessor must have def for SSA value");
                pt.srcs.insert(p_idx, p_ssa);
            }
            blocks[b_idx].phis.borrow_mut().push(pt);
            phi_ssa
        };

        blocks[b_idx].defs.borrow_mut().insert(ssa, b_ssa);
        worklist.pop();
        if worklist.is_empty() {
            return Ok(b_ssa);
        }
    }
}

fn get_or_insert_phi_dsts(bb: &mut BasicBlock) -> &mut OpPhiDsts {
    let ip = if let Some(ip) = bb.phi_dsts_ip() {
        ip
    } else {
        bb.instrs.insert(0, Instr::new(OpPhiDsts::new()));
        0
    };
    match &mut bb.instrs[ip].op {
        Op::PhiDsts(op) => op,
        _ => super::ice!("Expected to find the phi we just inserted"),
    }
}

fn get_or_insert_phi_srcs(bb: &mut BasicBlock) -> &mut OpPhiSrcs {
    let ip = if let Some(ip) = bb.phi_srcs_ip() {
        ip
    } else if let Some(ip) = bb.branch_ip() {
        bb.instrs.insert(ip, Instr::new(OpPhiSrcs::new()));
        ip
    } else {
        bb.instrs.push(Instr::new(OpPhiSrcs::new()));
        bb.instrs.len() - 1
    };
    match &mut bb.instrs[ip].op {
        Op::PhiSrcs(op) => op,
        _ => super::ice!("Expected to find the phi we just inserted"),
    }
}

impl Function {
    /// Repairs SSA form
    ///
    /// Certain passes such as register spilling may produce a program that is
    /// no longer in SSA form.  This pass is able to repair SSA by inserting
    /// phis as needed.  Even though we do not require dominance or that each
    /// value be defined once we do require that, for every use of an SSAValue
    /// and for every path from the start of the program to that use, there must
    /// be some definition of the value along that path.
    ///
    /// The algorithm implemented here is based on the one in "Simple and
    /// Efficient Construction of Static Single Assignment Form" by Braun, et.
    /// al.  The primary difference between our implementation and the paper is
    /// that we can't rewrite the IR on-the-fly.  Instead, we store everything
    /// in hash tables and handle removing redundant phis with back-edges as a
    /// separate pass between figuring out where phis are needed and actually
    /// constructing the phi instructions.
    pub fn repair_ssa(&mut self) -> Result<(), crate::CompileError> {
        // First, count the number of defs for each SSA value.  This will allow
        // us to skip any SSA values which only have a single definition in
        // later passes.
        let mut has_mult_defs = false;
        let mut num_defs = FxHashMap::default();
        for b in &self.blocks {
            for instr in &b.instrs {
                instr.for_each_ssa_def(|ssa| {
                    num_defs
                        .entry(*ssa)
                        .and_modify(|e| {
                            has_mult_defs = true;
                            *e += 1;
                        })
                        .or_insert(1);
                });
            }
        }

        if !has_mult_defs {
            return Ok(());
        }

        let cfg = &mut self.blocks;
        let ssa_alloc = &mut self.ssa_alloc;
        let phi_alloc = &mut self.phi_alloc;

        let mut blocks = Vec::new();
        let mut needs_src = BitSet::<Phi>::new(super::PHI_BITSET_CAPACITY);
        let mut synth_undefs: Vec<(usize, SSAValue)> = Vec::new();
        let mut ssa_or_phi_worklist = BinaryHeap::new();
        for b_idx in 0..cfg.len() {
            assert!(blocks.len() == b_idx);
            blocks.push(DefTrackerBlock {
                pred: cfg.pred_indices(b_idx).to_vec(),
                succ: cfg.succ_indices(b_idx).to_vec(),
                defs: RefCell::new(FxHashMap::default()),
                phis: RefCell::new(Vec::new()),
            });

            for instr in &mut cfg[b_idx].instrs {
                let mut err = Ok(());
                instr.for_each_ssa_use_mut(|ssa| {
                    if err.is_ok() && num_defs.get(ssa).copied().unwrap_or(0) > 1 {
                        match get_ssa_or_phi(
                            &mut ssa_or_phi_worklist,
                            ssa_alloc,
                            phi_alloc,
                            &blocks,
                            &mut needs_src,
                            &mut synth_undefs,
                            b_idx,
                            *ssa,
                        ) {
                            Ok(v) => *ssa = v,
                            Err(e) => err = Err(e),
                        }
                    }
                });
                err?;

                instr.for_each_ssa_def_mut(|ssa| {
                    if num_defs.get(ssa).copied().unwrap_or(0) > 1 {
                        let new_ssa = ssa_alloc.alloc(ssa.file());
                        blocks[b_idx].defs.borrow_mut().insert(*ssa, new_ssa);
                        *ssa = new_ssa;
                    }
                });
            }
        }

        // Populate phi sources for any back-edges
        loop {
            let Some(b_idx) = needs_src.iter().next() else {
                break;
            };
            needs_src.remove(b_idx);

            for s_idx in &blocks[b_idx].succ {
                if *s_idx <= b_idx {
                    let s = &blocks[*s_idx];

                    // We do a mutable borrow here.  The algorithm is recursive
                    // and may insert phis into other blocks.  However, because
                    // this is phi exists, its destination should be in the def
                    // set for s and so no new phis should need to be added.
                    // RefCell's dynamic borrow checks will assert this.
                    for phi in s.phis.borrow_mut().iter_mut() {
                        phi.srcs.entry(b_idx).or_insert_with(|| {
                            get_ssa_or_phi(
                                &mut ssa_or_phi_worklist,
                                ssa_alloc,
                                phi_alloc,
                                &blocks,
                                &mut needs_src,
                                &mut synth_undefs,
                                b_idx,
                                phi.orig,
                            )
                            .expect("ICE: phi source for back-edge must resolve via get_ssa_or_phi")
                        });
                    }
                }
            }
        }

        // For loop back-edges, we inserted a phi whether we need one or not.
        // We want to eliminate any redundant phis.
        let mut ssa_map = UnionFind::<SSAValue, FxBuildHasher>::new();
        if cfg.has_loop() {
            let mut to_do = true;
            while to_do {
                to_do = false;
                for b_idx in 0..cfg.len() {
                    let b = &blocks[b_idx];
                    b.phis.borrow_mut().retain_mut(|phi| {
                        let mut ssa = None;
                        #[expect(
                            clippy::explicit_iter_loop,
                            reason = "phi.srcs type requires explicit .iter_mut()"
                        )]
                        for (_, p_ssa) in phi.srcs.iter_mut() {
                            // Apply the remap to the phi sources so that we
                            // pick up any remaps from previous loop iterations.
                            *p_ssa = ssa_map.find(*p_ssa);

                            if *p_ssa == phi.dst {
                                continue;
                            }
                            if *ssa.get_or_insert(*p_ssa) != *p_ssa {
                                // Multiple unique sources
                                return true;
                            }
                        }

                        // All sources are identical or the phi destination so
                        // we can delete this phi and add it to the remap
                        let ssa = ssa.expect("Circular SSA def");
                        // union(a, b) ensures that the representative is the representative
                        // for a.  This means union(ssa, phi.dst) ensures that phi.dst gets
                        // mapped to ssa, not the other way around.
                        ssa_map.union(ssa, phi.dst);
                        to_do = true;
                        false
                    });
                }
            }
        }

        // Now we apply the remap to instruction sources and place the actual
        // phis
        for b_idx in 0..cfg.len() {
            // Grab successor indices for inserting OpPhiSrc. When there is a
            // single successor, phi sources go in this block. When there are
            // multiple successors, each successor's phis are handled if needed.
            let succ = cfg.succ_indices(b_idx);
            let succ_list: Vec<usize> = succ.to_vec();
            let s_idx = if succ_list.len() == 1 {
                Some(succ_list[0])
            } else {
                None
            };

            let bb = &mut cfg[b_idx];

            // First we have phi destinations
            let b_phis = blocks[b_idx].phis.borrow();
            if !b_phis.is_empty() {
                let phi_dst = get_or_insert_phi_dsts(bb);
                for pt in b_phis.iter() {
                    phi_dst.dsts.push(pt.phi, pt.dst.into());
                }
            }

            // Fix up any remapped SSA values in sources
            if !ssa_map.is_empty() {
                for instr in &mut bb.instrs {
                    instr.for_each_ssa_use_mut(|ssa| {
                        *ssa = ssa_map.find(*ssa);
                    });
                }
            }

            // Insert phi sources for each successor that has phi nodes.
            // Single-successor is the common case; multi-successor happens at
            // critical edges created by the SPIR-V roundtrip path.
            let phi_succs = if let Some(s_idx) = s_idx {
                vec![s_idx]
            } else {
                succ_list
                    .iter()
                    .copied()
                    .filter(|&si| !blocks[si].phis.borrow().is_empty())
                    .collect()
            };
            for si in phi_succs {
                let s_phis = blocks[si].phis.borrow();
                if !s_phis.is_empty() {
                    let phi_src = get_or_insert_phi_srcs(bb);
                    for pt in s_phis.iter() {
                        if let Some(&src_ssa) = pt.srcs.get(&b_idx) {
                            let ssa = ssa_map.find(src_ssa);
                            phi_src.srcs.push(pt.phi, ssa.into());
                        }
                    }
                }
            }
        }

        // Insert OpUndef instructions at the start of unreachable blocks
        // for any SSA values we synthesized above. Without these, the
        // scheduler would see a use with no corresponding definition.
        for (b_idx, undef_ssa) in synth_undefs {
            cfg[b_idx].instrs.insert(
                0,
                Instr::new(OpUndef {
                    dst: undef_ssa.into(),
                }),
            );
        }

        Ok(())
    }
}

impl Function {
    /// Fixes SSA dominance violations produced by `naga_translate` and
    /// optimization passes.
    ///
    /// Strategy: for every SSA value that has more than one definition
    /// or is live-in at the entry block, prepend `OpUndef` at the very
    /// start of the entry block. This guarantees `repair_ssa` can
    /// always trace backward to a definition (the OpUndef provides a
    /// reaching def on paths that miss the real definitions). DCE
    /// removes unused undefs and dead phi inputs afterward.
    pub fn fix_entry_live_in(&mut self) -> Result<(), crate::CompileError> {
        use super::liveness::SimpleLiveness;
        use coral_reef_stubs::fxhash::FxHashSet;

        // Collect values that are live-in at entry (no definition
        // dominates entry on some path to a use).
        let live = SimpleLiveness::for_function(self);
        let entry_li = live.live_in_values(0);

        // Collect ALL SSA values defined anywhere in the function.
        // We conservatively insert OpUndef for every defined value so
        // that repair_ssa always finds a reaching definition when
        // tracing backward to entry. This handles:
        // - single-def values whose def doesn't dominate all uses
        // - multi-def values with unreachable entry paths
        // - values defined at entry that are used before their def
        // DCE removes unused undefs afterward.
        let mut needs_undef: FxHashSet<SSAValue> = FxHashSet::default();
        for ssa in &entry_li {
            needs_undef.insert(*ssa);
        }
        for b in &self.blocks {
            for instr in &b.instrs {
                instr.for_each_ssa_def(|ssa| {
                    needs_undef.insert(*ssa);
                });
            }
        }

        if needs_undef.is_empty() {
            return Ok(());
        }

        // Also collect all SSA values that appear as uses but not as defs.
        // These can arise from naga_translate producing non-SSA IR.
        let mut all_uses: FxHashSet<SSAValue> = FxHashSet::default();
        for b in &self.blocks {
            for instr in &b.instrs {
                instr.for_each_ssa_use(|ssa| {
                    all_uses.insert(*ssa);
                });
            }
        }
        for ssa in &all_uses {
            needs_undef.insert(*ssa);
        }

        // Forward-reachability: BFS from entry to find live blocks.
        // Disconnect and clear unreachable blocks so repair_ssa, the
        // scheduler, and the register allocator never see stale code.
        {
            let n = self.blocks.len();
            let mut reachable = vec![false; n];
            let mut queue = std::collections::VecDeque::new();
            reachable[0] = true;
            queue.push_back(0);
            while let Some(b) = queue.pop_front() {
                for &s in self.blocks.succ_indices(b) {
                    if !reachable[s] {
                        reachable[s] = true;
                        queue.push_back(s);
                    }
                }
            }
            for bi in 1..n {
                if !reachable[bi] {
                    self.blocks[bi].instrs.clear();
                    self.blocks.disconnect_block(bi);
                }
            }
        }

        let mut undefs: Vec<Instr> = needs_undef
            .iter()
            .map(|ssa| Instr::new(OpUndef { dst: (*ssa).into() }))
            .collect();

        let entry = &mut self.blocks[0];
        undefs.append(&mut entry.instrs);
        entry.instrs = undefs;

        self.repair_ssa()?;
        self.opt_dce();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::codegen::ir::{
        BasicBlock, Function, Instr, LabelAllocator, OpCopy, OpExit, PhiAllocator, RegFile,
        SSAValueAllocator, Src,
    };
    use coral_reef_stubs::cfg::CFGBuilder;

    #[test]
    fn test_repair_ssa_no_op_for_single_def() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let a = ssa_alloc.alloc(RegFile::GPR);
        let instrs = vec![
            Instr::new(OpCopy {
                dst: a.into(),
                src: Src::ZERO,
            }),
            Instr::new(OpExit {}),
        ];
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
        let instr_count_before = func.blocks[0].instrs.len();
        func.repair_ssa().unwrap();
        let instr_count_after = func.blocks[0].instrs.len();
        assert_eq!(
            instr_count_before, instr_count_after,
            "single def should not add phis"
        );
    }

    #[test]
    fn test_repair_ssa_handles_multi_def() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let a = ssa_alloc.alloc(RegFile::GPR);
        let b = ssa_alloc.alloc(RegFile::GPR);
        let instrs = vec![
            Instr::new(OpCopy {
                dst: a.into(),
                src: Src::ZERO,
            }),
            Instr::new(OpCopy {
                dst: b.into(),
                src: a.into(),
            }),
            Instr::new(OpCopy {
                dst: a.into(),
                src: b.into(),
            }),
            Instr::new(OpExit {}),
        ];
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
        func.repair_ssa().unwrap();
        assert!(!func.blocks[0].instrs.is_empty());
    }
}
