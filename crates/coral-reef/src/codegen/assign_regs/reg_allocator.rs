// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

#![allow(clippy::wildcard_imports)]

use super::super::ir::*;
use super::*;

use coral_reef_stubs::bitset::BitSet;
use coral_reef_stubs::fxhash::FxHashMap;

#[derive(Clone)]
pub(super) struct RegAllocator {
    file: RegFile,
    pub(super) reg_count: u32,
    used: BitSet<usize>,
    pinned: BitSet<usize>,
    reg_ssa: Vec<Option<SSAValue>>,
    pub(super) ssa_reg: FxHashMap<SSAValue, u32>,
}

impl RegAllocator {
    pub fn new(file: RegFile, reg_count: u32) -> Self {
        let cap = usize::try_from(reg_count).unwrap();
        Self {
            file,
            reg_count,
            used: BitSet::new(cap),
            pinned: BitSet::new(cap),
            reg_ssa: Vec::new(),
            ssa_reg: FxHashMap::default(),
        }
    }

    pub(super) const fn file(&self) -> RegFile {
        self.file
    }

    pub fn used_reg_count(&self) -> u32 {
        self.ssa_reg.len().try_into().unwrap()
    }

    pub fn reg_is_used(&self, reg: u32) -> bool {
        self.used.contains(usize::try_from(reg).unwrap())
    }

    pub fn reg_is_pinned(&self, reg: u32) -> bool {
        self.pinned.contains(usize::try_from(reg).unwrap())
    }

    pub fn try_get_reg(&self, ssa: SSAValue) -> Option<u32> {
        self.ssa_reg.get(&ssa).copied()
    }

    pub fn try_get_ssa(&self, reg: u32) -> Option<SSAValue> {
        if self.reg_is_used(reg) {
            Some(self.reg_ssa[usize::try_from(reg).unwrap()].unwrap())
        } else {
            None
        }
    }

    pub fn try_get_vec_reg(&self, vec: &[SSAValue]) -> Option<u32> {
        let reg = self.try_get_reg(vec[0])?;
        let comps = u8::try_from(vec.len()).unwrap();

        let align = u32::from(comps).next_power_of_two();
        if reg % align != 0 {
            return None;
        }

        for c in 1..comps {
            let ssa = vec[usize::from(c)];
            if self.try_get_reg(ssa) != Some(reg + u32::from(c)) {
                return None;
            }
        }
        Some(reg)
    }

    pub fn free_ssa(&mut self, ssa: SSAValue) -> u32 {
        assert!(ssa.file() == self.file);
        let reg = self.ssa_reg.remove(&ssa).unwrap();
        assert!(self.reg_is_used(reg));
        let reg_usize = usize::try_from(reg).unwrap();
        assert!(self.reg_ssa[reg_usize] == Some(ssa));
        self.used.remove(reg_usize);
        self.pinned.remove(reg_usize);
        reg
    }

    pub fn assign_reg(&mut self, ssa: SSAValue, reg: u32) {
        assert!(ssa.file() == self.file);
        assert!(reg < self.reg_count);
        assert!(!self.reg_is_used(reg));

        let reg_usize = usize::try_from(reg).unwrap();
        if reg_usize >= self.reg_ssa.len() {
            self.reg_ssa.resize(reg_usize + 1, None);
        }
        self.reg_ssa[reg_usize] = Some(ssa);
        let old = self.ssa_reg.insert(ssa, reg);
        assert!(old.is_none());
        self.used.insert(reg_usize);
    }

    pub fn pin_reg(&mut self, reg: u32) {
        assert!(self.reg_is_used(reg));
        self.pinned.insert(usize::try_from(reg).unwrap());
    }

    fn reg_range_is_unset(set: &BitSet<usize>, reg: u32, comps: u8) -> bool {
        for c in 0..u32::from(comps) {
            if set.contains(usize::try_from(reg + c).unwrap()) {
                return false;
            }
        }
        true
    }

    fn try_find_unset_reg_range(
        &self,
        set: &BitSet<usize>,
        start_reg: u32,
        comps: u8,
        align_mul: u8,
        align_offset: u8,
    ) -> Option<u32> {
        let res = set.find_aligned_unset_range(
            usize::try_from(start_reg).unwrap(),
            comps.into(),
            align_mul.into(),
            align_offset.into(),
        )?;
        let res = u32::try_from(res).unwrap();
        if res + u32::from(comps) <= self.reg_count {
            Some(res)
        } else {
            None
        }
    }

    pub fn try_find_unused_reg_range(
        &self,
        start_reg: u32,
        comps: u8,
        align_mul: u8,
        align_offset: u8,
    ) -> Option<u32> {
        self.try_find_unset_reg_range(&self.used, start_reg, comps, align_mul, align_offset)
    }

    pub fn alloc_scalar(
        &mut self,
        ip: usize,
        sum: &SSAUseMap,
        phi_webs: &mut PhiWebs,
        ssa: SSAValue,
    ) -> u32 {
        // Bias register assignment using the phi coalescing
        if let Some(reg) = phi_webs.get(ssa) {
            if !self.reg_is_used(reg) {
                self.assign_reg(ssa, reg);
                return reg;
            }
        }

        // Otherwise, use SSAUseMap heuristics
        if let Some(u) = sum.find_vec_use_after(ssa, ip) {
            match u {
                SSAUse::FixedReg(reg) => {
                    if !self.reg_is_used(*reg) {
                        self.assign_reg(ssa, *reg);
                        return *reg;
                    }
                }
                SSAUse::Vec(vec) => {
                    let mut comp = u8::MAX;
                    for c in 0..vec.comps() {
                        if vec[usize::from(c)] == ssa {
                            comp = c;
                            break;
                        }
                    }
                    assert!(comp < vec.comps());

                    let align = vec.comps().next_power_of_two();
                    for c in 0..vec.comps() {
                        if c == comp {
                            continue;
                        }

                        let other = vec[usize::from(c)];
                        let Some(other_reg) = self.try_get_reg(other) else {
                            continue;
                        };

                        let vec_reg = other_reg & !u32::from(align - 1);
                        if other_reg != vec_reg + u32::from(c) {
                            continue;
                        }

                        let reg = vec_reg + u32::from(comp);
                        if reg < self.reg_count && !self.reg_is_used(reg) {
                            self.assign_reg(ssa, reg);
                            return reg;
                        }
                    }

                    // We weren't able to pair it with an already allocated
                    // register but maybe we can at least find a place the vec
                    // would fit
                    if let Some(base_reg) = self.try_find_unused_reg_range(0, vec.comps(), align, 0)
                    {
                        let reg = base_reg + u32::from(comp);
                        self.assign_reg(ssa, reg);
                        return reg;
                    }
                }
            }
        }

        let reg = self
            .try_find_unused_reg_range(0, 1, 1, 0)
            .expect("Failed to find free register");
        self.assign_reg(ssa, reg);
        reg
    }
}

pub(super) struct VecRegAllocator<'a> {
    ra: &'a mut RegAllocator,
    pcopy: OpParCopy,
    pinned: BitSet<usize>,
    evicted: FxHashMap<SSAValue, u32>,
}

impl<'a> VecRegAllocator<'a> {
    pub(super) fn new(ra: &'a mut RegAllocator) -> Self {
        let pinned = ra.pinned.clone();
        VecRegAllocator {
            ra,
            pcopy: OpParCopy::new(),
            pinned,
            evicted: FxHashMap::default(),
        }
    }

    pub(super) const fn file(&self) -> RegFile {
        self.ra.file()
    }

    fn pin_reg(&mut self, reg: u32) {
        self.pinned.insert(usize::try_from(reg).unwrap());
    }

    fn pin_reg_range(&mut self, reg: u32, comps: u8) {
        for c in 0..u32::from(comps) {
            self.pin_reg(reg + c);
        }
    }

    fn reg_is_pinned(&self, reg: u32) -> bool {
        self.pinned.contains(usize::try_from(reg).unwrap())
    }

    fn reg_range_is_unpinned(&self, reg: u32, comps: u8) -> bool {
        RegAllocator::reg_range_is_unset(&self.pinned, reg, comps)
    }

    fn assign_pin_reg(&mut self, ssa: SSAValue, reg: u32) -> RegRef {
        self.pin_reg(reg);
        self.ra.assign_reg(ssa, reg);
        RegRef::new(self.file(), reg, 1)
    }

    pub fn assign_pin_vec_reg(&mut self, vec: &SSARef, reg: u32) -> RegRef {
        for c in 0..vec.comps() {
            let ssa = vec[usize::from(c)];
            self.assign_pin_reg(ssa, reg + u32::from(c));
        }
        RegRef::new(self.file(), reg, vec.comps())
    }

    fn try_find_unpinned_reg_range(
        &self,
        start_reg: u32,
        comps: u8,
        align_mul: u8,
        align_offset: u8,
    ) -> Option<u32> {
        self.ra
            .try_find_unset_reg_range(&self.pinned, start_reg, comps, align_mul, align_offset)
    }

    pub fn evict_ssa(&mut self, ssa: SSAValue, old_reg: u32) {
        assert!(ssa.file() == self.file());
        assert!(!self.reg_is_pinned(old_reg));
        self.evicted.insert(ssa, old_reg);
    }

    pub fn evict_reg_if_used(&mut self, reg: u32) {
        assert!(!self.reg_is_pinned(reg));

        if let Some(ssa) = self.ra.try_get_ssa(reg) {
            self.ra.free_ssa(ssa);
            self.evict_ssa(ssa, reg);
        }
    }

    fn move_ssa_to_reg(&mut self, ssa: SSAValue, new_reg: u32) {
        if let Some(old_reg) = self.ra.try_get_reg(ssa) {
            assert!(!self.evicted.contains_key(&ssa));
            assert!(!self.reg_is_pinned(old_reg));

            if new_reg == old_reg {
                self.pin_reg(new_reg);
            } else {
                self.ra.free_ssa(ssa);
                self.evict_reg_if_used(new_reg);

                self.pcopy.push(
                    RegRef::new(self.file(), new_reg, 1).into(),
                    RegRef::new(self.file(), old_reg, 1).into(),
                );

                self.assign_pin_reg(ssa, new_reg);
            }
        } else if let Some(old_reg) = self.evicted.remove(&ssa) {
            self.evict_reg_if_used(new_reg);

            self.pcopy.push(
                RegRef::new(self.file(), new_reg, 1).into(),
                RegRef::new(self.file(), old_reg, 1).into(),
            );

            self.assign_pin_reg(ssa, new_reg);
        } else {
            panic!("Unknown SSA value");
        }
    }

    pub(super) fn finish(mut self, pcopy: &mut OpParCopy) {
        pcopy.dsts_srcs.append(&mut self.pcopy.dsts_srcs);

        if !self.evicted.is_empty() {
            // Sort so we get determinism, even if the hash map order changes
            // from one run to another or due to rust compiler updates.
            let mut evicted: Vec<_> = self.evicted.drain().collect();
            evicted.sort_by_key(|(_, reg)| *reg);

            for (ssa, old_reg) in evicted {
                let mut next_reg = 0;
                let new_reg = loop {
                    let reg = self
                        .ra
                        .try_find_unused_reg_range(next_reg, 1, 1, 0)
                        .expect("Failed to find free register");
                    if !self.reg_is_pinned(reg) {
                        break reg;
                    }
                    next_reg = reg + 1;
                };

                pcopy.push(
                    RegRef::new(self.file(), new_reg, 1).into(),
                    RegRef::new(self.file(), old_reg, 1).into(),
                );
                self.assign_pin_reg(ssa, new_reg);
            }
        }
    }

    pub fn try_get_vec_reg(&self, vec: &[SSAValue]) -> Option<u32> {
        self.ra.try_get_vec_reg(vec)
    }

    pub fn collect_vector(&mut self, vec: &[SSAValue]) -> RegRef {
        if let Some(reg) = self.try_get_vec_reg(vec) {
            let comps = u8::try_from(vec.len()).unwrap();
            self.pin_reg_range(reg, comps);
            return RegRef::new(self.file(), reg, comps);
        }

        let comps = u8::try_from(vec.len()).unwrap();
        let align = comps.next_power_of_two();

        let reg = self
            .ra
            .try_find_unused_reg_range(0, comps, align, 0)
            .or_else(|| {
                for c in 0..comps {
                    let ssa = vec[usize::from(c)];
                    let Some(comp_reg) = self.ra.try_get_reg(ssa) else {
                        continue;
                    };

                    let vec_reg = comp_reg & !u32::from(align - 1);
                    if comp_reg != vec_reg + u32::from(c) {
                        continue;
                    }

                    if vec_reg + u32::from(comps) > self.ra.reg_count {
                        continue;
                    }

                    if self.reg_range_is_unpinned(vec_reg, comps) {
                        return Some(vec_reg);
                    }
                }
                None
            })
            .or_else(|| self.try_find_unpinned_reg_range(0, comps, align, 0))
            .expect("Failed to find an unpinned register range");

        for c in 0..comps {
            let ssa = vec[usize::from(c)];
            self.move_ssa_to_reg(ssa, reg + u32::from(c));
        }

        RegRef::new(self.file(), reg, comps)
    }

    pub fn alloc_vector(&mut self, vec: &SSARef) -> RegRef {
        let comps = vec.comps();
        let align = comps.next_power_of_two();

        if let Some(reg) = self.ra.try_find_unused_reg_range(0, comps, align, 0) {
            return self.assign_pin_vec_reg(vec, reg);
        }

        let reg = self
            .try_find_unpinned_reg_range(0, comps, align, 0)
            .expect("Failed to find an unpinned register range");

        for c in 0..comps {
            self.evict_reg_if_used(reg + u32::from(c));
        }
        self.assign_pin_vec_reg(vec, reg)
    }

    pub fn free_killed(&mut self, killed: &KillSet) {
        for ssa in killed.iter() {
            if ssa.file() == self.file() {
                self.ra.free_ssa(*ssa);
            }
        }
    }
}

impl Drop for VecRegAllocator<'_> {
    fn drop(&mut self) {
        assert!(self.evicted.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ssa_value::SSAValueAllocator;

    fn make_gpr_allocator(reg_count: u32) -> RegAllocator {
        RegAllocator::new(RegFile::GPR, reg_count)
    }

    #[test]
    fn test_reg_allocator_new() {
        let ra = make_gpr_allocator(64);
        assert_eq!(ra.used_reg_count(), 0);
        assert!(!ra.reg_is_used(0));
        assert!(!ra.reg_is_pinned(0));
    }

    #[test]
    fn test_assign_reg_and_try_get() {
        let mut alloc = SSAValueAllocator::new();
        let ssa = alloc.alloc(RegFile::GPR);
        let mut ra = make_gpr_allocator(64);

        ra.assign_reg(ssa, 5);
        assert_eq!(ra.used_reg_count(), 1);
        assert!(ra.reg_is_used(5));
        assert_eq!(ra.try_get_reg(ssa), Some(5));
        assert_eq!(ra.try_get_ssa(5), Some(ssa));
    }

    #[test]
    fn test_free_ssa() {
        let mut alloc = SSAValueAllocator::new();
        let ssa = alloc.alloc(RegFile::GPR);
        let mut ra = make_gpr_allocator(64);
        ra.assign_reg(ssa, 3);
        let freed = ra.free_ssa(ssa);
        assert_eq!(freed, 3);
        assert_eq!(ra.used_reg_count(), 0);
        assert!(!ra.reg_is_used(3));
        assert_eq!(ra.try_get_reg(ssa), None);
    }

    #[test]
    fn test_pin_reg() {
        let mut alloc = SSAValueAllocator::new();
        let ssa = alloc.alloc(RegFile::GPR);
        let mut ra = make_gpr_allocator(64);
        ra.assign_reg(ssa, 2);
        assert!(!ra.reg_is_pinned(2));
        ra.pin_reg(2);
        assert!(ra.reg_is_pinned(2));
    }

    #[test]
    fn test_try_find_unused_reg_range() {
        let ra = make_gpr_allocator(64);
        assert_eq!(ra.try_find_unused_reg_range(0, 1, 1, 0), Some(0));
        assert_eq!(ra.try_find_unused_reg_range(0, 4, 4, 0), Some(0));
        assert_eq!(ra.try_find_unused_reg_range(10, 2, 2, 0), Some(10));
    }

    #[test]
    fn test_try_find_unused_reg_range_with_used() {
        let mut alloc = SSAValueAllocator::new();
        let ssa = alloc.alloc(RegFile::GPR);
        let mut ra = make_gpr_allocator(64);
        ra.assign_reg(ssa, 0);
        // With reg 0 used, finding 1 comp should start at 1
        assert_eq!(ra.try_find_unused_reg_range(0, 1, 1, 0), Some(1));
    }

    #[test]
    fn test_try_get_vec_reg_aligned() {
        let mut alloc = SSAValueAllocator::new();
        let v0 = alloc.alloc(RegFile::GPR);
        let v1 = alloc.alloc(RegFile::GPR);
        let vec = [v0, v1];
        let mut ra = make_gpr_allocator(64);
        ra.assign_reg(v0, 4); // 4 is 4-aligned for vec2
        ra.assign_reg(v1, 5);
        assert_eq!(ra.try_get_vec_reg(&vec), Some(4));
    }

    #[test]
    fn test_try_get_vec_reg_unaligned_returns_none() {
        let mut alloc = SSAValueAllocator::new();
        let v0 = alloc.alloc(RegFile::GPR);
        let v1 = alloc.alloc(RegFile::GPR);
        let vec = [v0, v1];
        let mut ra = make_gpr_allocator(64);
        ra.assign_reg(v0, 5); // 5 is not 2-aligned for vec2
        ra.assign_reg(v1, 6);
        assert_eq!(ra.try_get_vec_reg(&vec), None);
    }

    #[test]
    fn test_try_get_vec_reg_not_contiguous_returns_none() {
        let mut alloc = SSAValueAllocator::new();
        let v0 = alloc.alloc(RegFile::GPR);
        let v1 = alloc.alloc(RegFile::GPR);
        let vec = [v0, v1];
        let mut ra = make_gpr_allocator(64);
        ra.assign_reg(v0, 4);
        ra.assign_reg(v1, 6); // gap - not contiguous
        assert_eq!(ra.try_get_vec_reg(&vec), None);
    }
}
