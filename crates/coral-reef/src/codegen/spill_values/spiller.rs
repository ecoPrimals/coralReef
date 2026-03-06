// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

#![allow(clippy::wildcard_imports)]

use super::*;
use coral_reef_stubs::bitset::BitSet;
use coral_reef_stubs::fxhash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub(super) fn spill_values<S: Spill>(func: &mut Function, file: RegFile, limit: u32, spill: S) {
    let files = RegFileSet::from_iter([file]);
    let live = NextUseLiveness::for_function(func, &files);
    let blocks = &mut func.blocks;

    // Record the set of SSA values used within each loop
    let mut phi_dst_maps = Vec::new();
    let mut phi_src_maps = Vec::new();
    let mut loop_uses = FxHashMap::default();
    for b_idx in 0..blocks.len() {
        phi_dst_maps.push(PhiDstMap::from_block(&blocks[b_idx]));
        phi_src_maps.push(PhiSrcMap::from_block(&blocks[b_idx]));

        if let Some(lh_idx) = blocks.loop_header_index(b_idx) {
            let uses = loop_uses
                .entry(lh_idx)
                .or_insert_with(|| RefCell::new(FxHashSet::default()));
            let uses: &mut FxHashSet<_> = uses.get_mut();

            for instr in &blocks[b_idx].instrs {
                instr.for_each_ssa_use(|ssa| {
                    if ssa.file() == file {
                        uses.insert(*ssa);
                    }
                });
            }
        }
    }

    if !loop_uses.is_empty() {
        // The previous loop only added values to the uses set for the
        // inner-most loop.  Propagate from inner loops to outer loops.
        for b_idx in (0..blocks.len()).rev() {
            let Some(uses) = loop_uses.get(&b_idx) else {
                continue;
            };
            let uses = uses.borrow();

            let Some(dom) = blocks.dom_parent_index(b_idx) else {
                continue;
            };

            let Some(dom_lh_idx) = blocks.loop_header_index(dom) else {
                continue;
            };

            let mut parent_uses = loop_uses
                .get(&dom_lh_idx)
                .expect("dominator loop header must be in loop_uses")
                .borrow_mut();
            for ssa in uses.iter() {
                parent_uses.insert(*ssa);
            }
        }
    }

    let mut spill = SpillCache::new(&mut func.ssa_alloc, spill);
    let mut spilled_phis: BitSet<Phi> = BitSet::new(4096);

    let mut ssa_state_in: Vec<SSAState> = Vec::new();
    let mut ssa_state_out: Vec<SSAState> = Vec::new();

    for b_idx in 0..blocks.len() {
        let bl = live.block_live(b_idx);

        let preds = blocks.pred_indices(b_idx).to_vec();
        let w = if preds.is_empty() {
            // This is the start block so we start with nothing in
            // registers.
            LiveSet::new()
        } else if preds.len() == 1 {
            // If we only have one predecessor then it can't possibly be a
            // loop header and we can just copy the predecessor's w.
            assert!(!blocks.is_loop_header(b_idx));
            assert!(preds[0] < b_idx);
            let p_w = &ssa_state_out[preds[0]].w;
            LiveSet::from_iter(p_w.iter().filter(|ssa| bl.is_live_in(ssa)).copied())
        } else if !blocks[b_idx].uniform && file.is_uniform() {
            // If this is a non-uniform block, then we can't spill or fill any
            // uniform registers.  The good news is that none of our non-uniform
            // predecessors could spill, either, so we know that everything that
            // was resident coming in will fit in the register file.
            let mut w = LiveSet::new();
            for p_idx in &preds {
                if *p_idx < b_idx {
                    let p_w = &ssa_state_out[*p_idx].w;
                    w.extend(p_w.iter().filter(|ssa| bl.is_live_in(ssa)).copied());
                }
            }
            debug_assert!(w.count(file) <= limit);
            w
        } else if blocks.is_loop_header(b_idx) {
            let mut i_b: FxHashSet<SSAValue> = FxHashSet::from_iter(bl.iter_live_in().copied());

            if let Some(phi) = blocks[b_idx].phi_dsts() {
                for (_, dst) in phi.dsts.iter() {
                    if let Dst::SSA(vec) = dst {
                        assert!(vec.comps() == 1);
                        let ssa = vec[0];
                        if ssa.file() == file {
                            i_b.insert(ssa);
                        }
                    }
                }
            }

            let lu = loop_uses
                .get(&b_idx)
                .expect("loop header must have loop uses")
                .borrow();
            let mut w = LiveSet::new();

            let mut some = BinaryHeap::new();
            for ssa in &i_b {
                if lu.contains(ssa) {
                    let next_use = bl
                        .first_use(ssa)
                        .expect("live-in SSA value must have next use in block");
                    some.push(Reverse(SSANextUse::new(*ssa, next_use)));
                }
            }
            while w.count(file) < limit {
                let Some(entry) = some.pop() else {
                    break;
                };
                w.insert(entry.0.ssa);
            }

            // If we still have room, consider values which aren't used
            // inside the loop.
            if w.count(file) < limit {
                for ssa in &i_b {
                    debug_assert!(ssa.file() == file);
                    if !lu.contains(ssa) {
                        let next_use = bl
                            .first_use(ssa)
                            .expect("live-in SSA value must have next use");
                        some.push(Reverse(SSANextUse::new(*ssa, next_use)));
                    }
                }

                while w.count(file) < limit {
                    let Some(entry) = some.pop() else {
                        break;
                    };
                    w.insert(entry.0.ssa);
                }
            }

            w
        } else {
            let phi_dst_map = &phi_dst_maps[b_idx];

            struct SSAPredInfo {
                predecessor_count: usize,
                next_use: usize,
            }
            let mut live: FxHashMap<SSAValue, SSAPredInfo> = FxHashMap::default();

            for p_idx in &preds {
                let phi_src_map = &phi_src_maps[*p_idx];

                for mut ssa in ssa_state_out[*p_idx].w.iter().copied() {
                    if let Some(phi) = phi_src_map.get_phi(&ssa) {
                        ssa = *phi_dst_map
                            .get_dst_ssa(phi)
                            .expect("phi must have destination in phi_dst_map");
                    }

                    if let Some(next_use) = bl.first_use(&ssa) {
                        live.entry(ssa)
                            .and_modify(|e| e.predecessor_count += 1)
                            .or_insert_with(|| SSAPredInfo {
                                predecessor_count: 1,
                                next_use,
                            });
                    }
                }
            }

            let mut w = LiveSet::new();
            let mut some = BinaryHeap::new();

            for (ssa, info) in live.drain() {
                if info.predecessor_count == preds.len() {
                    // This one is in all the input sets
                    w.insert(ssa);
                } else {
                    some.push(Reverse(SSANextUse::new(ssa, info.next_use)));
                }
            }
            while w.count(file) < limit {
                let Some(entry) = some.pop() else {
                    break;
                };
                let ssa = entry.0.ssa;
                assert!(ssa.file() == file);
                w.insert(ssa);
            }

            w
        };

        let s = if preds.is_empty() {
            FxHashSet::default()
        } else if preds.len() == 1 {
            let p_s = &ssa_state_out[preds[0]].s;
            FxHashSet::from_iter(p_s.iter().filter(|ssa| bl.is_live_in(ssa)).copied())
        } else {
            let mut s: FxHashSet<_> = FxHashSet::default();
            for p_idx in &preds {
                if *p_idx >= b_idx {
                    continue;
                }

                // We diverge a bit from Braun and Hack here.  They assume that
                // S is is a subset of W which is clearly bogus.  Instead, we
                // take the union of all forward edge predecessor S_out and
                // intersect with live-in for the current block.
                for ssa in &ssa_state_out[*p_idx].s {
                    if bl.is_live_in(ssa) {
                        s.insert(*ssa);
                    }
                }
            }

            // The loop header heuristic sometimes drops stuff from W that has
            // never been spilled so we need to make sure everything live-in
            // which isn't in W is included in the spill set so that it gets
            // properly spilled when we spill across CF edges.
            if blocks.is_loop_header(b_idx) {
                for ssa in bl.iter_live_in() {
                    if !w.contains(ssa) {
                        s.insert(*ssa);
                    }
                }
            }

            s
        };

        let mut p: FxHashSet<_> = FxHashSet::default();
        for p_idx in &preds {
            if *p_idx < b_idx {
                let p_p = &ssa_state_out[*p_idx].p;
                p.extend(p_p.iter().filter(|ssa| bl.is_live_in(ssa)).copied());
            }
        }

        for ssa in bl.iter_live_in() {
            debug_assert!(w.contains(ssa) || s.contains(ssa) || spill.is_const(ssa));
        }

        let mut b = SSAState { w, s, p };

        assert!(ssa_state_in.len() == b_idx);
        ssa_state_in.push(b.clone());

        let bb = &mut blocks[b_idx];

        let mut instrs = Vec::new();
        for (ip, mut instr) in bb.instrs.drain(..).enumerate() {
            if let Op::Copy(op) = &instr.op {
                spill.add_copy_if_const(op);
            }

            match &mut instr.op {
                Op::PhiDsts(op) => {
                    // For phis, anything that is not in W needs to be spilled
                    // by setting the destination to some spill value.
                    for (phi, dst) in op.dsts.iter_mut() {
                        let vec = dst.as_ssa().expect("phi dst must be SSA value");
                        debug_assert!(vec.comps() == 1);
                        let ssa = &vec[0];

                        if ssa.file() == file && !b.w.contains(ssa) {
                            spilled_phis.insert(*phi);
                            b.s.insert(*ssa);
                            *dst = spill.get_spill(*ssa).into();
                        }
                    }
                }
                Op::PhiSrcs(_) => {
                    // We handle phi sources later.  For now, leave them be.
                }
                Op::ParCopy(pcopy) => {
                    let mut num_w_dsts = 0_u32;
                    for (dst, src) in pcopy.dsts_srcs.iter_mut() {
                        let dst_vec = dst.as_ssa().expect("par copy dst must be SSA value");
                        debug_assert!(dst_vec.comps() == 1);
                        let dst_ssa = &dst_vec[0];

                        debug_assert!(src.is_unmodified());
                        let Some(src_vec) = src.reference.as_ssa() else {
                            continue;
                        };
                        debug_assert!(src_vec.comps() == 1);
                        let src_ssa = &src_vec[0];

                        debug_assert!(dst_ssa.file() == src_ssa.file());
                        if src_ssa.file() != file {
                            continue;
                        }

                        // If it's not resident, rewrite to just move from one
                        // spill to another, assuming that copying in spill
                        // space is efficient
                        if b.w.contains(src_ssa) {
                            num_w_dsts += 1;
                        } else {
                            if b.s.insert(*src_ssa) {
                                assert!(spill.is_const(src_ssa));
                                instrs.push(spill.spill(*src_ssa));
                            }
                            b.s.insert(*dst_ssa);
                            *src = spill.get_spill(*src_ssa).into();
                            *dst = spill.get_spill(*dst_ssa).into();
                        }
                    }

                    // We can now assume that a source is in W if and only if
                    // the file matches.  Remove all killed sources from W.
                    for (_, src) in pcopy.dsts_srcs.iter() {
                        let Some(src_vec) = src.reference.as_ssa() else {
                            continue;
                        };
                        let src_ssa = &src_vec[0];
                        if !bl.is_live_after_ip(src_ssa, ip) {
                            b.w.remove(src_ssa);
                        }
                    }

                    let rel_limit = limit - b.w.count(file);
                    if num_w_dsts > rel_limit {
                        // We can't spill uniform registers in a non-uniform
                        // block
                        assert!(bb.uniform || !file.is_uniform());

                        let count = num_w_dsts - rel_limit;
                        let count = count.try_into().expect("spill count must fit in u32");

                        let mut spills = SpillChooser::new(bl, &b.p, ip, count);
                        for (dst, _) in pcopy.dsts_srcs.iter() {
                            let dst_ssa = &dst.as_ssa().expect("par copy dst must be SSA value")[0];
                            if dst_ssa.file() == file {
                                spills.add_candidate(*dst_ssa);
                            }
                        }

                        let spills: FxHashSet<SSAValue> = FxHashSet::from_iter(spills);

                        for (dst, src) in pcopy.dsts_srcs.iter_mut() {
                            let dst_ssa = &dst.as_ssa().expect("par copy dst must be SSA value")[0];
                            let src_ssa = &src
                                .reference
                                .as_ssa()
                                .expect("par copy src must be SSA value")[0];
                            if spills.contains(dst_ssa) {
                                if b.s.insert(*src_ssa) {
                                    if DEBUG.annotate() {
                                        instrs.push(Instr::new(OpAnnotate {
                                            annotation: "generated by spill_values".into(),
                                        }));
                                    }
                                    instrs.push(spill.spill(*src_ssa));
                                }
                                b.s.insert(*dst_ssa);
                                *src = spill.get_spill(*src_ssa).into();
                                *dst = spill.get_spill(*dst_ssa).into();
                            }
                        }
                    }

                    for (dst, _) in pcopy.dsts_srcs.iter() {
                        let dst_ssa = &dst.as_ssa().expect("par copy dst must be SSA value")[0];
                        if dst_ssa.file() == file {
                            b.w.insert(*dst_ssa);
                        }
                    }
                }
                _ => {
                    if file == RegFile::UGPR && !bb.uniform {
                        // We can't spill UGPRs in a non-uniform block.
                        // Instead, we depend on two facts:
                        //
                        //  1. Uniform instructions are not allowed in
                        //     non-uniform blocks
                        //
                        //  2. Non-uniform instructions can always accept a wave
                        //     register in leu of a uniform register
                        //
                        debug_assert!(spill.spill_file(file) == RegFile::GPR);
                        instr.for_each_ssa_use_mut(|ssa| {
                            if ssa.file() == file && !b.w.contains(ssa) {
                                if b.s.insert(*ssa) {
                                    assert!(spill.is_const(ssa));
                                    instrs.push(spill.spill(*ssa));
                                }
                                *ssa = spill.get_spill(*ssa);
                            }
                        });
                    } else if file == RegFile::UPred && !bb.uniform {
                        // We can't spill UPreds in a non-uniform block.
                        // Instead, we depend on two facts:
                        //
                        //  1. Uniform instructions are not allowed in
                        //     non-uniform blocks
                        //
                        //  2. Non-uniform instructions can always accept a wave
                        //     register in leu of a uniform register
                        //
                        //  3. We can un-spill from a UGPR directly to a Pred
                        //
                        // This also shouldn't come up that often in practice
                        // so it's okay to un-spill every time on the spot.
                        //
                        instr.for_each_ssa_use_mut(|ssa| {
                            if ssa.file() == file && !b.w.contains(ssa) {
                                if DEBUG.annotate() {
                                    instrs.push(Instr::new(OpAnnotate {
                                        annotation: "generated by spill_values".into(),
                                    }));
                                }
                                let tmp = spill.alloc.alloc(RegFile::Pred);
                                instrs.push(spill.fill_dst(tmp.into(), *ssa));
                                *ssa = tmp;
                            }
                        });
                    } else {
                        // First compute fills even though those have to come
                        // after spills.
                        let mut fills = Vec::new();
                        instr.for_each_ssa_use(|ssa| {
                            if ssa.file() == file && !b.w.contains(ssa) {
                                debug_assert!(b.s.contains(ssa) || spill.is_const(ssa));
                                debug_assert!(bb.uniform || !ssa.is_uniform());
                                fills.push(spill.fill(*ssa));
                                b.w.insert(*ssa);
                            }
                        });

                        let rel_pressure = bl.get_instr_pressure(ip, &instr)[file];
                        let abs_pressure = b.w.count(file) + u32::from(rel_pressure);

                        if abs_pressure > limit {
                            let count = abs_pressure - limit;
                            let count = count.try_into().expect("spill count must fit in u32");

                            let mut spills = SpillChooser::new(bl, &b.p, ip, count);
                            for ssa in b.w.iter() {
                                spills.add_candidate(*ssa);
                            }

                            for ssa in spills {
                                debug_assert!(ssa.file() == file);
                                b.w.remove(&ssa);
                                if !spill.is_const(&ssa) {
                                    if DEBUG.annotate() {
                                        instrs.push(Instr::new(OpAnnotate {
                                            annotation: "generated by spill_values".into(),
                                        }));
                                    }
                                    instrs.push(spill.spill(ssa));
                                    b.s.insert(ssa);
                                }
                            }
                        }

                        if DEBUG.annotate() {
                            instrs.push(Instr::new(OpAnnotate {
                                annotation: "generated by spill_values".into(),
                            }));
                        }
                        instrs.append(&mut fills);

                        instr.for_each_ssa_use(|ssa| {
                            if ssa.file() == file {
                                debug_assert!(b.w.contains(ssa));
                            }
                        });

                        b.w.insert_instr_top_down(ip, &instr, bl);
                    }
                }
            }

            // OpPin takes the normal spilling path but we want to also mark any
            // of its destination SSA values as pinned.
            if matches!(&instr.op, Op::Pin(_)) {
                instr.for_each_ssa_def(|ssa| {
                    b.p.insert(*ssa);
                });
            }

            instrs.push(instr);
        }
        bb.instrs = instrs;

        assert!(ssa_state_out.len() == b_idx);
        ssa_state_out.push(b);
    }

    // Now that everthing is spilled, we handle phi sources and connect the
    // blocks by adding spills and fills as needed along edges.
    for p_idx in 0..blocks.len() {
        let succ = blocks.succ_indices(p_idx);
        if succ.len() != 1 {
            // We don't have any critical edges
            for s_idx in succ {
                debug_assert!(blocks.pred_indices(*s_idx).len() == 1);
            }
            continue;
        }
        let s_idx = succ[0];

        let pb = &mut blocks[p_idx];
        let p_out = &ssa_state_out[p_idx];
        let s_in = &ssa_state_in[s_idx];
        let phi_dst_map = &phi_dst_maps[s_idx];

        let mut spills = Vec::new();
        let mut fills = Vec::new();

        if let Some(op) = pb.phi_srcs_mut() {
            for (phi, src) in op.srcs.iter_mut() {
                debug_assert!(src.is_unmodified());
                let vec = src.reference.as_ssa().expect("phi src must be SSA value");
                debug_assert!(vec.comps() == 1);
                let ssa = &vec[0];

                if ssa.file() != file {
                    continue;
                }

                if spilled_phis.contains(*phi) {
                    if !p_out.s.contains(ssa) {
                        spills.push(*ssa);
                    }
                    *src = spill.get_spill(*ssa).into();
                } else {
                    if !p_out.w.contains(ssa) {
                        fills.push(*ssa);
                    }
                }
            }
        }

        for ssa in &s_in.s {
            if !p_out.s.contains(ssa) {
                assert!(p_out.w.contains(ssa) || spill.is_const(ssa));
                spills.push(*ssa);
            }
        }

        for ssa in s_in.w.iter() {
            if phi_dst_map.get_phi(ssa).is_some() {
                continue;
            }

            if !p_out.w.contains(ssa) {
                fills.push(*ssa);
            }
        }

        if spills.is_empty() && fills.is_empty() {
            continue;
        }

        // Sort to ensure stability of the algorithm
        spills.sort_by_key(|ssa| ssa.idx());
        fills.sort_by_key(|ssa| ssa.idx());

        let mut instrs = Vec::new();
        for ssa in spills {
            instrs.push(spill.spill(ssa));
        }
        for ssa in fills {
            debug_assert!(pb.uniform || !ssa.is_uniform());
            instrs.push(spill.fill(ssa));
        }

        // Insert spills and fills right after the phi (if any)
        let ip = pb
            .phi_srcs_ip()
            .or_else(|| pb.branch_ip())
            .unwrap_or(pb.instrs.len());
        pb.instrs.splice(ip..ip, instrs.into_iter());
    }
}
