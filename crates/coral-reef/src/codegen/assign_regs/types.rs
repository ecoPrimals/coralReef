// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

#![allow(clippy::wildcard_imports)]

use super::super::ir::*;
use super::super::union_find::UnionFind;

use coral_reef_stubs::fxhash::{FxBuildHasher, FxHashMap, FxHashSet};
use std::cmp::Ordering;

pub(super) struct KillSet {
    pub(super) set: FxHashSet<SSAValue>,
    vec: Vec<SSAValue>,
}

impl KillSet {
    pub fn new() -> Self {
        Self {
            set: FxHashSet::default(),
            vec: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.vec.len()
    }

    pub fn clear(&mut self) {
        self.set.clear();
        self.vec.clear();
    }

    pub fn insert(&mut self, ssa: SSAValue) {
        if self.set.insert(ssa) {
            self.vec.push(ssa);
        }
    }

    pub fn iter(&self) -> std::slice::Iter<'_, SSAValue> {
        self.vec.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }
}

// These two helpers are carefully paired for the purposes of RA.
// src_ssa_ref() returns whatever SSARef is present in the source, if any.
// src_set_reg() overwrites that SSARef with a RegRef.
#[inline]
pub(super) fn src_ssa_ref(src: &Src) -> Option<&[SSAValue]> {
    match &src.reference {
        SrcRef::SSA(ssa) => Some(&ssa[..]),
        SrcRef::CBuf(CBufRef {
            buf: CBuf::BindlessSSA(ssa),
            ..
        }) => Some(&ssa[..]),
        _ => None,
    }
}

#[inline]
pub(super) fn src_set_reg(src: &mut Src, reg: RegRef) {
    match &mut src.reference {
        SrcRef::SSA(_) => {
            src.reference = reg.into();
        }
        SrcRef::CBuf(cb) => {
            debug_assert!(matches!(&cb.buf, CBuf::BindlessSSA(_)));
            debug_assert!(reg.file() == RegFile::UGPR && reg.comps() == 2);
            cb.buf = CBuf::BindlessUGPR(reg);
        }
        _ => (),
    }
}

pub(super) enum SSAUse {
    FixedReg(u32),
    Vec(SSARef),
}

pub(super) struct SSAUseMap {
    ssa_map: FxHashMap<SSAValue, Vec<(usize, SSAUse)>>,
}

impl SSAUseMap {
    fn add_fixed_reg_use(&mut self, ip: usize, ssa: SSAValue, reg: u32) {
        let v = self.ssa_map.entry(ssa).or_default();
        v.push((ip, SSAUse::FixedReg(reg)));
    }

    fn add_vec_use(&mut self, ip: usize, vec: &[SSAValue]) {
        if vec.len() == 1 {
            return;
        }

        for ssa in vec {
            let v = self.ssa_map.entry(*ssa).or_default();
            v.push((ip, SSAUse::Vec(SSARef::new(vec))));
        }
    }

    pub(super) fn find_vec_use_after(&self, ssa: SSAValue, ip: usize) -> Option<&SSAUse> {
        self.ssa_map.get(&ssa).and_then(|v| {
            let p = v.partition_point(|(uip, _)| *uip <= ip);
            if p == v.len() {
                None
            } else {
                let (_, u) = &v[p];
                Some(u)
            }
        })
    }

    pub fn add_block(&mut self, b: &BasicBlock) {
        for (ip, instr) in b.instrs.iter().enumerate() {
            match &instr.op {
                Op::RegOut(op) => {
                    for (i, src) in op.srcs.iter().enumerate() {
                        let out_reg = u32::try_from(i).unwrap();
                        if let Some(ssa) = src_ssa_ref(src) {
                            assert!(ssa.len() == 1);
                            self.add_fixed_reg_use(ip, ssa[0], out_reg);
                        }
                    }
                }
                _ => {
                    // We don't care about predicates because they're scalar
                    for src in instr.srcs() {
                        if let Some(ssa) = src_ssa_ref(src) {
                            self.add_vec_use(ip, ssa);
                        }
                    }
                }
            }
        }
    }

    pub fn for_block(b: &BasicBlock) -> Self {
        let mut am = Self {
            ssa_map: FxHashMap::default(),
        };
        am.add_block(b);
        am
    }
}

/// Tracks the most recent register assigned to a given phi web
///
/// During register assignment, we then try to assign this register
/// to the next SSAValue in the same web.
///
/// This heuristic is inspired by the "Aggressive pre-coalescing" described in
/// section 4 of Colombet et al 2011.
///
/// Q. Colombet, B. Boissinot, P. Brisk, S. Hack and F. Rastello,
///     "Graph-coloring and treescan register allocation using repairing," 2011
///     Proceedings of the 14th International Conference on Compilers,
///     Architectures and Synthesis for Embedded Systems (CASES), Taipei,
///     Taiwan, 2011, pp. 45-54, doi: 10.1145/2038698.2038708.
pub(super) struct PhiWebs {
    uf: UnionFind<SSAValue, FxBuildHasher>,
    assignments: FxHashMap<SSAValue, u32>,
}

impl PhiWebs {
    pub fn new(f: &Function) -> Self {
        let mut uf = UnionFind::new();

        // Populate uf with phi equivalence classes
        //
        // Note that we intentionally don't pay attention to move instructions
        // below - the assumption is that any move instructions at this point
        // were inserted by cssa-conversion and will hurt the coalescing
        for b_idx in 0..f.blocks.len() {
            let Some(phi_dsts) = f.blocks[b_idx].phi_dsts() else {
                continue;
            };
            let dsts: FxHashMap<Phi, &SSARef> = phi_dsts
                .dsts
                .iter()
                .map(|(idx, dst)| {
                    let ssa_ref = dst.as_ssa().expect("Expected ssa form");
                    (*idx, ssa_ref)
                })
                .collect();

            for pred_idx in f.blocks.pred_indices(b_idx) {
                let phi_srcs = f.blocks[*pred_idx].phi_srcs().expect("Missing phi_srcs");
                for (src_idx, src) in phi_srcs.srcs.iter() {
                    let a = src.as_ssa().expect("Expected ssa form");
                    let b = dsts[src_idx];

                    assert_eq!(a.comps(), 1);
                    assert_eq!(b.comps(), 1);

                    uf.union(a[0], b[0]);
                }
            }
        }

        Self {
            uf,
            assignments: FxHashMap::default(),
        }
    }

    pub fn get(&mut self, ssa: SSAValue) -> Option<u32> {
        let phi_web_id = self.uf.find(ssa);
        self.assignments.get(&phi_web_id).copied()
    }

    pub fn set(&mut self, ssa: SSAValue, reg: u32) {
        let phi_web_id = self.uf.find(ssa);
        self.assignments.insert(phi_web_id, reg);
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum LiveRef {
    SSA(SSAValue),
    Phi(Phi),
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) struct LiveValue {
    pub live_ref: LiveRef,
    pub reg_ref: RegRef,
}

// We need a stable ordering of live values so that RA is deterministic
impl Ord for LiveValue {
    fn cmp(&self, other: &Self) -> Ordering {
        let s_file = u8::from(self.reg_ref.file());
        let o_file = u8::from(other.reg_ref.file());
        match s_file.cmp(&o_file) {
            Ordering::Equal => {
                let s_idx = self.reg_ref.base_idx();
                let o_idx = other.reg_ref.base_idx();
                s_idx.cmp(&o_idx)
            }
            ord => ord,
        }
    }
}

impl PartialOrd for LiveValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
