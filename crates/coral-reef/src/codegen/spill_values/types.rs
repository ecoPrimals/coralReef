// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

#![allow(clippy::wildcard_imports)]

use super::*;
use coral_reef_stubs::fxhash::{FxHashMap, FxHashSet};
use std::cmp::{Ordering, Reverse, max};
use std::collections::BinaryHeap;

#[derive(Default)]
pub(super) struct PhiDstMap {
    phi_ssa: FxHashMap<Phi, SSAValue>,
    ssa_phi: FxHashMap<SSAValue, Phi>,
}

impl PhiDstMap {
    fn new() -> Self {
        Self {
            phi_ssa: FxHashMap::default(),
            ssa_phi: FxHashMap::default(),
        }
    }

    fn add_phi_dst(&mut self, phi: Phi, dst: &Dst) {
        let vec = dst.as_ssa().expect("Not an SSA destination");
        debug_assert!(vec.comps() == 1);
        self.phi_ssa.insert(phi, vec[0]);
        self.ssa_phi.insert(vec[0], phi);
    }

    pub fn from_block(block: &BasicBlock) -> Self {
        let mut map = Self::new();
        if let Some(op) = block.phi_dsts() {
            for (idx, dst) in op.dsts.iter() {
                map.add_phi_dst(*idx, dst);
            }
        }
        map
    }

    pub fn get_phi(&self, ssa: &SSAValue) -> Option<&Phi> {
        self.ssa_phi.get(ssa)
    }

    pub fn get_dst_ssa(&self, phi: &Phi) -> Option<&SSAValue> {
        self.phi_ssa.get(phi)
    }
}

#[derive(Default)]
pub(super) struct PhiSrcMap {
    src_phi: FxHashMap<SSAValue, Phi>,
}

impl PhiSrcMap {
    fn new() -> Self {
        Self::default()
    }

    fn add_phi_src(&mut self, phi: Phi, src: &Src) {
        debug_assert!(src.is_unmodified());
        let vec = src.reference.as_ssa().expect("Not an SSA source");
        debug_assert!(vec.comps() == 1);
        self.src_phi.insert(vec[0], phi);
    }

    pub fn from_block(block: &BasicBlock) -> Self {
        let mut map = Self::new();
        if let Some(op) = block.phi_srcs() {
            for (phi, src) in op.srcs.iter() {
                map.add_phi_src(*phi, src);
            }
        }
        map
    }

    pub fn get_phi(&self, ssa: &SSAValue) -> Option<&Phi> {
        self.src_phi.get(ssa)
    }
}

pub(super) trait Spill {
    fn spill_file(&self, file: RegFile) -> RegFile;
    fn spill(&mut self, dst: SSAValue, src: Src) -> Instr;
    fn fill(&mut self, dst: Dst, src: SSAValue) -> Instr;
}

pub(super) struct SpillUniform<'a> {
    info: &'a mut ShaderInfo,
}

impl<'a> SpillUniform<'a> {
    pub fn new(info: &'a mut ShaderInfo) -> Self {
        Self { info }
    }
}

impl Spill for SpillUniform<'_> {
    fn spill_file(&self, file: RegFile) -> RegFile {
        debug_assert!(file.is_uniform());
        file.to_warp()
    }

    fn spill(&mut self, dst: SSAValue, src: Src) -> Instr {
        self.info.spills_to_reg += 1;
        Instr::new(OpCopy {
            dst: dst.into(),
            src,
        })
    }

    fn fill(&mut self, dst: Dst, src: SSAValue) -> Instr {
        self.info.fills_from_reg += 1;
        Instr::new(OpR2UR {
            dst,
            src: src.into(),
        })
    }
}

pub(super) struct SpillPred<'a> {
    info: &'a mut ShaderInfo,
}

impl<'a> SpillPred<'a> {
    pub fn new(info: &'a mut ShaderInfo) -> Self {
        Self { info }
    }
}

impl Spill for SpillPred<'_> {
    fn spill_file(&self, file: RegFile) -> RegFile {
        match file {
            RegFile::Pred => RegFile::GPR,
            RegFile::UPred => RegFile::UGPR,
            _ => panic!("Unsupported register file"),
        }
    }

    fn spill(&mut self, dst: SSAValue, src: Src) -> Instr {
        assert!(matches!(dst.file(), RegFile::GPR | RegFile::UGPR));
        self.info.spills_to_reg += 1;
        if let Some(b) = src.as_bool() {
            let u32_src = Src::from(if b { !0 } else { 0 });
            Instr::new(OpCopy {
                dst: dst.into(),
                src: u32_src,
            })
        } else {
            Instr::new(OpSel {
                dst: dst.into(),
                cond: src.bnot(),
                srcs: [0.into(), (!0).into()],
            })
        }
    }

    fn fill(&mut self, dst: Dst, src: SSAValue) -> Instr {
        assert!(matches!(src.file(), RegFile::GPR | RegFile::UGPR));
        self.info.fills_from_reg += 1;
        Instr::new(OpISetP {
            dst,
            set_op: PredSetOp::And,
            cmp_op: IntCmpOp::Ne,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [0.into(), src.into()],
            accum: true.into(),
            low_cmp: true.into(),
        })
    }
}

pub(super) struct SpillBar<'a> {
    info: &'a mut ShaderInfo,
}

impl<'a> SpillBar<'a> {
    pub fn new(info: &'a mut ShaderInfo) -> Self {
        Self { info }
    }
}

impl Spill for SpillBar<'_> {
    fn spill_file(&self, file: RegFile) -> RegFile {
        assert!(file == RegFile::Bar);
        RegFile::GPR
    }

    fn spill(&mut self, dst: SSAValue, src: Src) -> Instr {
        assert!(dst.file() == RegFile::GPR);
        self.info.spills_to_reg += 1;
        Instr::new(OpBMov {
            dst: dst.into(),
            src,
            clear: false,
        })
    }

    fn fill(&mut self, dst: Dst, src: SSAValue) -> Instr {
        assert!(src.file() == RegFile::GPR);
        self.info.fills_from_reg += 1;
        Instr::new(OpBMov {
            dst,
            src: src.into(),
            clear: false,
        })
    }
}

pub(super) struct SpillGPR<'a> {
    info: &'a mut ShaderInfo,
}

impl<'a> SpillGPR<'a> {
    pub fn new(info: &'a mut ShaderInfo) -> Self {
        Self { info }
    }
}

impl Spill for SpillGPR<'_> {
    fn spill_file(&self, file: RegFile) -> RegFile {
        assert!(file == RegFile::GPR);
        RegFile::Mem
    }

    fn spill(&mut self, dst: SSAValue, src: Src) -> Instr {
        assert!(dst.file() == RegFile::Mem);
        self.info.spills_to_mem += 1;
        if let Some(ssa) = src.as_ssa() {
            assert!(ssa.file() == RegFile::GPR);
            Instr::new(OpCopy {
                dst: dst.into(),
                src,
            })
        } else {
            // We use parallel copies for spilling non-GPR things to Mem
            let mut pcopy = OpParCopy::new();
            pcopy.push(dst.into(), src);
            Instr::new(pcopy)
        }
    }

    fn fill(&mut self, dst: Dst, src: SSAValue) -> Instr {
        assert!(src.file() == RegFile::Mem);
        self.info.fills_from_mem += 1;
        Instr::new(OpCopy {
            dst,
            src: src.into(),
        })
    }
}

#[derive(Eq, PartialEq)]
pub(super) struct SSANextUse {
    pub(super) ssa: SSAValue,
    next_use: usize,
}

impl SSANextUse {
    pub fn new(ssa: SSAValue, next_use: usize) -> Self {
        Self { ssa, next_use }
    }
}

impl Ord for SSANextUse {
    fn cmp(&self, other: &Self) -> Ordering {
        self.next_use
            .cmp(&other.next_use)
            .then_with(|| self.ssa.idx().cmp(&other.ssa.idx()))
    }
}

impl PartialOrd for SSANextUse {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub(super) struct SpillCache<'a, S: Spill> {
    pub alloc: &'a mut SSAValueAllocator,
    spill: S,
    const_tracker: ConstTracker,
    val_spill: FxHashMap<SSAValue, SSAValue>,
}

impl<'a, S: Spill> SpillCache<'a, S> {
    pub fn new(alloc: &'a mut SSAValueAllocator, spill: S) -> Self {
        SpillCache {
            alloc,
            spill,
            const_tracker: ConstTracker::new(),
            val_spill: FxHashMap::default(),
        }
    }

    pub fn add_copy_if_const(&mut self, op: &OpCopy) {
        self.const_tracker.add_copy(op);
    }

    pub fn is_const(&self, ssa: &SSAValue) -> bool {
        self.const_tracker.contains(ssa)
    }

    pub fn spill_file(&self, file: RegFile) -> RegFile {
        self.spill.spill_file(file)
    }

    pub fn get_spill(&mut self, ssa: SSAValue) -> SSAValue {
        *self
            .val_spill
            .entry(ssa)
            .or_insert_with(|| self.alloc.alloc(self.spill.spill_file(ssa.file())))
    }

    fn spill_src(&mut self, ssa: SSAValue, src: Src) -> Instr {
        let dst = self.get_spill(ssa);
        self.spill.spill(dst, src)
    }

    pub fn spill(&mut self, ssa: SSAValue) -> Instr {
        if let Some(c) = self.const_tracker.get(&ssa) {
            self.spill_src(ssa, c.clone())
        } else {
            self.spill_src(ssa, ssa.into())
        }
    }

    pub fn fill_dst(&mut self, dst: Dst, ssa: SSAValue) -> Instr {
        let src = self.get_spill(ssa);
        self.spill.fill(dst, src)
    }

    pub fn fill(&mut self, ssa: SSAValue) -> Instr {
        if let Some(c) = self.const_tracker.get(&ssa) {
            Instr::new(OpCopy {
                dst: ssa.into(),
                src: c.clone(),
            })
        } else {
            self.fill_dst(ssa.into(), ssa)
        }
    }
}

pub(super) struct SpillChooser<'a> {
    bl: &'a NextUseBlockLiveness,
    pinned: &'a FxHashSet<SSAValue>,
    ip: usize,
    count: usize,
    spills: BinaryHeap<Reverse<SSANextUse>>,
    min_next_use: usize,
}

pub(super) struct SpillChoiceIter {
    spills: BinaryHeap<Reverse<SSANextUse>>,
}

impl<'a> SpillChooser<'a> {
    pub fn new(
        bl: &'a NextUseBlockLiveness,
        pinned: &'a FxHashSet<SSAValue>,
        ip: usize,
        count: usize,
    ) -> Self {
        Self {
            bl,
            pinned,
            ip,
            count,
            spills: BinaryHeap::new(),
            min_next_use: ip + 1,
        }
    }

    pub fn add_candidate(&mut self, ssa: SSAValue) {
        // Don't spill anything that's pinned
        if self.pinned.contains(&ssa) {
            return;
        }

        // Ignore anything used sonner than spill options we've already
        // rejected.
        let next_use = self
            .bl
            .next_use_after_or_at_ip(&ssa, self.ip)
            .expect("spill candidate SSA must have a next use");
        if next_use < self.min_next_use {
            return;
        }

        self.spills.push(Reverse(SSANextUse::new(ssa, next_use)));

        if self.spills.len() > self.count {
            // Because we reversed the heap, pop actually removes the
            // one with the lowest next_use which is what we want here.
            let old = self.spills.pop().expect("heap non-empty after len check");
            debug_assert!(self.spills.len() == self.count);
            self.min_next_use = max(self.min_next_use, old.0.next_use);
        }
    }
}

impl IntoIterator for SpillChooser<'_> {
    type Item = SSAValue;
    type IntoIter = SpillChoiceIter;

    fn into_iter(self) -> SpillChoiceIter {
        SpillChoiceIter {
            spills: self.spills,
        }
    }
}

impl Iterator for SpillChoiceIter {
    type Item = SSAValue;

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.spills.len();
        (len, Some(len))
    }

    fn next(&mut self) -> Option<SSAValue> {
        self.spills.pop().map(|x| x.0.ssa)
    }
}

#[derive(Clone)]
pub(super) struct SSAState {
    // The set of variables which currently exist in registers
    pub w: LiveSet,
    // The set of variables which have already been spilled.  These don't need
    // to be spilled again.
    pub s: FxHashSet<SSAValue>,
    // The set of pinned variables
    pub p: FxHashSet<SSAValue>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{
        BasicBlock, Dst, Function, Instr, LabelAllocator, OpCopy, OpExit, OpPhiDsts, OpPhiSrcs,
        PhiAllocator, RegFile, RegFileSet, SSAValueAllocator, Src,
    };
    use crate::codegen::liveness::{Liveness, NextUseLiveness};
    use coral_reef_stubs::cfg::CFGBuilder;
    use std::cmp::Ordering;

    fn make_empty_block() -> BasicBlock {
        BasicBlock {
            label: LabelAllocator::new().alloc(),
            uniform: false,
            instrs: vec![],
        }
    }

    fn make_block_with_phi_dsts(phi: Phi, dst_ssa: SSAValue) -> BasicBlock {
        let mut instrs = vec![];
        let mut phi_dsts = OpPhiDsts::new();
        phi_dsts.dsts.push(phi, Dst::from(dst_ssa));
        instrs.push(Instr::new(phi_dsts));
        instrs.push(Instr::new(OpExit {}));
        BasicBlock {
            label: LabelAllocator::new().alloc(),
            uniform: false,
            instrs,
        }
    }

    fn make_block_with_phi_srcs(phi: Phi, src_ssa: SSAValue) -> BasicBlock {
        let mut instrs = vec![];
        instrs.push(Instr::new(OpCopy {
            dst: src_ssa.into(),
            src: Src::ZERO,
        }));
        let mut phi_srcs = OpPhiSrcs::new();
        phi_srcs.srcs.push(phi, Src::from(src_ssa));
        instrs.push(Instr::new(phi_srcs));
        instrs.push(Instr::new(OpExit {}));
        BasicBlock {
            label: LabelAllocator::new().alloc(),
            uniform: false,
            instrs,
        }
    }

    #[test]
    fn test_phi_dst_map_from_empty_block() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let some_ssa = ssa_alloc.alloc(RegFile::GPR);
        let block = make_empty_block();
        let map = PhiDstMap::from_block(&block);
        assert!(map.get_phi(&some_ssa).is_none());
    }

    #[test]
    fn test_phi_src_map_from_empty_block() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let some_ssa = ssa_alloc.alloc(RegFile::GPR);
        let block = make_empty_block();
        let map = PhiSrcMap::from_block(&block);
        assert!(map.get_phi(&some_ssa).is_none());
    }

    #[test]
    fn test_phi_dst_map_from_block_with_phi() {
        let mut phi_alloc = PhiAllocator::new();
        let mut ssa_alloc = SSAValueAllocator::new();
        let phi = phi_alloc.alloc();
        let dst_ssa = ssa_alloc.alloc(RegFile::GPR);
        let block = make_block_with_phi_dsts(phi, dst_ssa);
        let map = PhiDstMap::from_block(&block);
        assert!(map.get_phi(&dst_ssa).is_some_and(|p| *p == phi));
        assert_eq!(map.get_dst_ssa(&phi), Some(&dst_ssa));
    }

    #[test]
    fn test_phi_src_map_from_block_with_phi() {
        let mut phi_alloc = PhiAllocator::new();
        let mut ssa_alloc = SSAValueAllocator::new();
        let phi = phi_alloc.alloc();
        let src_ssa = ssa_alloc.alloc(RegFile::GPR);
        let block = make_block_with_phi_srcs(phi, src_ssa);
        let map = PhiSrcMap::from_block(&block);
        assert!(map.get_phi(&src_ssa).is_some_and(|p| *p == phi));
    }

    #[test]
    fn test_ssa_next_use_construction() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let ssa = ssa_alloc.alloc(RegFile::GPR);
        let nu = SSANextUse::new(ssa, 10);
        assert_eq!(nu.ssa, ssa);
    }

    #[test]
    fn test_ssa_next_use_ord() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let ssa1 = ssa_alloc.alloc(RegFile::GPR);
        let ssa2 = ssa_alloc.alloc(RegFile::GPR);
        let nu1 = SSANextUse::new(ssa1, 5);
        let nu2 = SSANextUse::new(ssa2, 10);
        assert_eq!(nu1.cmp(&nu2), Ordering::Less);
        assert_eq!(nu2.cmp(&nu1), Ordering::Greater);
    }

    #[test]
    fn test_ssa_next_use_ord_same_next_use_tiebreak_by_idx() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let ssa1 = ssa_alloc.alloc(RegFile::GPR);
        let ssa2 = ssa_alloc.alloc(RegFile::GPR);
        let nu1 = SSANextUse::new(ssa1, 5);
        let nu2 = SSANextUse::new(ssa2, 5);
        assert_eq!(nu1.cmp(&nu2), Ordering::Less);
        assert_eq!(nu2.cmp(&nu1), Ordering::Greater);
    }

    #[test]
    fn test_ssa_next_use_partial_ord() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let ssa = ssa_alloc.alloc(RegFile::GPR);
        let nu = SSANextUse::new(ssa, 5);
        assert_eq!(nu.partial_cmp(&nu), Some(Ordering::Equal));
    }

    #[test]
    fn test_ssa_state_construction() {
        let w = LiveSet::new();
        let s = FxHashSet::default();
        let p = FxHashSet::default();
        let state = SSAState { w, s, p };
        assert!(state.w.iter().next().is_none());
        assert!(state.s.is_empty());
        assert!(state.p.is_empty());
    }

    #[test]
    fn test_spill_chooser_iterator() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let mut instrs = vec![];
        let a = ssa_alloc.alloc(RegFile::GPR);
        let b = ssa_alloc.alloc(RegFile::GPR);
        let c = ssa_alloc.alloc(RegFile::GPR);
        instrs.push(Instr::new(OpCopy {
            dst: a.into(),
            src: Src::ZERO,
        }));
        instrs.push(Instr::new(OpCopy {
            dst: b.into(),
            src: a.into(),
        }));
        instrs.push(Instr::new(OpCopy {
            dst: c.into(),
            src: b.into(),
        }));
        instrs.push(Instr::new(OpExit {}));

        let mut label_alloc = LabelAllocator::new();
        let mut cfg_builder = CFGBuilder::new();
        cfg_builder.add_block(BasicBlock {
            label: label_alloc.alloc(),
            uniform: false,
            instrs,
        });
        let func = Function {
            ssa_alloc,
            phi_alloc: PhiAllocator::new(),
            blocks: cfg_builder.build(),
        };

        let files = RegFileSet::from_iter([RegFile::GPR]);
        let live = NextUseLiveness::for_function(&func, &files);
        let bl = live.block_live(0);
        let pinned = FxHashSet::default();
        let mut chooser = SpillChooser::new(bl, &pinned, 0, 1);
        chooser.add_candidate(a);
        chooser.add_candidate(b);
        assert!(chooser.into_iter().next().is_some());
    }
}
