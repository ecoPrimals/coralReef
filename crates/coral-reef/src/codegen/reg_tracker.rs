// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

use coral_reef_stubs::fxhash::FxHashMap;

use super::ir::*;

use std::ops::{Index, IndexMut, Range};

pub struct RegTracker<T> {
    reg: [T; 255],
    ureg: [T; 80],
    pred: [T; 7],
    upred: [T; 7],
    carry: [T; 1],
}

fn new_array_with<T, const N: usize>(f: &impl Fn() -> T) -> [T; N] {
    let mut v = Vec::with_capacity(N);
    for _ in 0..N {
        v.push(f());
    }
    v.try_into()
        .unwrap_or_else(|_| crate::codegen::ice!("Array size mismatch"))
}

impl<T> RegTracker<T> {
    pub fn new_with(f: &impl Fn() -> T) -> Self {
        Self {
            reg: new_array_with(f),
            ureg: new_array_with(f),
            pred: new_array_with(f),
            upred: new_array_with(f),
            carry: new_array_with(f),
        }
    }
}

impl<T> Index<RegRef> for RegTracker<T> {
    type Output = [T];

    fn index(&self, reg: RegRef) -> &[T] {
        let range = reg.idx_range();
        let range = Range {
            start: usize::try_from(range.start).expect("register index must fit in usize"),
            end: usize::try_from(range.end).expect("register index must fit in usize"),
        };

        match reg.file() {
            RegFile::GPR => &self.reg[range],
            RegFile::UGPR => &self.ureg[range],
            RegFile::Pred => &self.pred[range],
            RegFile::UPred => &self.upred[range],
            RegFile::Carry => &self.carry[range],
            RegFile::Bar => &[], // Barriers have a HW scoreboard
            RegFile::Mem => crate::codegen::ice!("Not a register"),
        }
    }
}

impl<T> IndexMut<RegRef> for RegTracker<T> {
    fn index_mut(&mut self, reg: RegRef) -> &mut [T] {
        let range = reg.idx_range();
        let range = Range {
            start: usize::try_from(range.start).expect("register index must fit in usize"),
            end: usize::try_from(range.end).expect("register index must fit in usize"),
        };

        match reg.file() {
            RegFile::GPR => &mut self.reg[range],
            RegFile::UGPR => &mut self.ureg[range],
            RegFile::Pred => &mut self.pred[range],
            RegFile::UPred => &mut self.upred[range],
            RegFile::Carry => &mut self.carry[range],
            RegFile::Bar => &mut [], // Barriers have a HW scoreboard
            RegFile::Mem => crate::codegen::ice!("Not a register"),
        }
    }
}

/// Memory-light version of [RegTracker].
///
/// This version uses sparse hashmaps instead of dense arrays.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct SparseRegTracker<T: Default> {
    regs: FxHashMap<RegRef, T>,
}

impl<T: Default + Clone + Eq> SparseRegTracker<T> {
    pub fn for_each_pred(&mut self, f: impl FnMut(&mut T)) {
        self.for_each_ref_mut(RegRef::new(RegFile::Pred, 0, 7), f);
    }

    pub fn for_each_carry(&mut self, f: impl FnMut(&mut T)) {
        self.for_each_ref_mut(RegRef::new(RegFile::Carry, 0, 1), f);
    }

    pub fn merge_with(&mut self, other: &Self, mut f: impl FnMut(&mut T, &T)) {
        use std::collections::hash_map::Entry;

        for (k, v) in &other.regs {
            match self.regs.entry(*k) {
                Entry::Occupied(mut occupied_entry) => {
                    f(occupied_entry.get_mut(), v);
                }
                Entry::Vacant(vacant_entry) => {
                    vacant_entry.insert((*v).clone());
                }
            }
        }
    }

    pub fn retain(&mut self, mut f: impl FnMut(&mut T) -> bool) {
        self.regs.retain(|_k, v| f(v));
    }
}

/// Common behavior for [RegTracker] and [SparseRegTracker]
pub trait RegRefIterable<T> {
    fn for_each_ref_mut(&mut self, reg: RegRef, f: impl FnMut(&mut T));

    fn for_each_instr_pred_mut(&mut self, instr: &Instr, mut f: impl FnMut(&mut T)) {
        if let PredRef::Reg(reg) = &instr.pred.predicate {
            self.for_each_ref_mut(*reg, |t| f(t));
        }
    }

    fn for_each_instr_src_mut(&mut self, instr: &Instr, mut f: impl FnMut(usize, &mut T)) {
        for (i, src) in instr.srcs().iter().enumerate() {
            match &src.reference {
                SrcRef::Reg(reg) => {
                    self.for_each_ref_mut(*reg, |t| f(i, t));
                }
                SrcRef::CBuf(CBufRef {
                    buf: CBuf::BindlessUGPR(reg),
                    ..
                }) => {
                    self.for_each_ref_mut(*reg, |t| f(i, t));
                }
                _ => (),
            }
        }
    }

    fn for_each_instr_dst_mut(&mut self, instr: &Instr, mut f: impl FnMut(usize, &mut T)) {
        for (i, dst) in instr.dsts().iter().enumerate() {
            if let Dst::Reg(reg) = dst {
                self.for_each_ref_mut(*reg, |t| f(i, t));
            }
        }
    }
}

impl<T: Default> RegRefIterable<T> for SparseRegTracker<T> {
    fn for_each_ref_mut(&mut self, reg: RegRef, mut f: impl FnMut(&mut T)) {
        match reg.file() {
            RegFile::Bar => return, // Barriers have a HW scoreboard
            RegFile::Mem => crate::codegen::ice!("Not a register"),
            _ => {}
        }

        for i in 0..reg.comps() {
            f(self.regs.entry(reg.comp(i)).or_default());
        }
    }
}

impl<T> RegRefIterable<T> for RegTracker<T> {
    fn for_each_ref_mut(&mut self, reg: RegRef, mut f: impl FnMut(&mut T)) {
        for entry in &mut self[reg] {
            f(entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reg_tracker_new_with() {
        let tracker: RegTracker<u32> = RegTracker::new_with(&|| 0);
        let reg = RegRef::new(RegFile::GPR, 0, 1);
        assert_eq!(tracker[reg].len(), 1);
        assert_eq!(tracker[reg][0], 0);
    }

    #[test]
    fn reg_tracker_index_gpr() {
        let mut tracker: RegTracker<u32> = RegTracker::new_with(&|| 0);
        let reg = RegRef::new(RegFile::GPR, 5, 2);
        tracker[reg][0] = 42;
        tracker[reg][1] = 43;
        assert_eq!(tracker[reg], [42, 43]);
    }

    #[test]
    fn reg_tracker_index_ugpr() {
        let mut tracker: RegTracker<u32> = RegTracker::new_with(&|| 0);
        let reg = RegRef::new(RegFile::UGPR, 0, 1);
        tracker[reg][0] = 10;
        assert_eq!(tracker[reg][0], 10);
    }

    #[test]
    fn reg_tracker_index_pred() {
        let mut tracker: RegTracker<bool> = RegTracker::new_with(&|| false);
        let reg = RegRef::new(RegFile::Pred, 0, 7);
        tracker[reg][0] = true;
        assert!(tracker[reg][0]);
    }

    #[test]
    fn reg_tracker_index_bar_returns_empty() {
        let tracker: RegTracker<u32> = RegTracker::new_with(&|| 0);
        let reg = RegRef::new(RegFile::Bar, 0, 1);
        assert!(tracker[reg].is_empty());
    }

    #[test]
    fn reg_tracker_for_each_ref_mut() {
        let mut tracker: RegTracker<u32> = RegTracker::new_with(&|| 0);
        let reg = RegRef::new(RegFile::GPR, 1, 2);
        tracker.for_each_ref_mut(reg, |t| *t += 1);
        assert_eq!(tracker[reg], [1, 1]);
    }

    #[test]
    fn sparse_reg_tracker_for_each_pred() {
        let mut tracker = SparseRegTracker::<u32>::default();
        tracker.for_each_pred(|t| *t = 1);
        let mut sum = 0;
        tracker.for_each_pred(|t| sum += *t);
        assert_eq!(sum, 7, "all 7 preds should be set to 1");
    }

    #[test]
    fn sparse_reg_tracker_for_each_carry() {
        let mut tracker = SparseRegTracker::<u32>::default();
        tracker.for_each_carry(|t| *t = 99);
        let mut val = 0;
        tracker.for_each_carry(|t| val = *t);
        assert_eq!(val, 99);
    }

    #[test]
    fn sparse_reg_tracker_merge_with() {
        let mut a = SparseRegTracker::<u32>::default();
        let mut b = SparseRegTracker::<u32>::default();
        a.for_each_ref_mut(RegRef::new(RegFile::GPR, 0, 1), |t| *t = 1);
        b.for_each_ref_mut(RegRef::new(RegFile::GPR, 0, 1), |t| *t = 2);
        b.for_each_ref_mut(RegRef::new(RegFile::GPR, 1, 1), |t| *t = 3);
        a.merge_with(&b, |a, b| *a += *b);
        let mut v0 = 0;
        let mut v1 = 0;
        a.for_each_ref_mut(RegRef::new(RegFile::GPR, 0, 1), |t| v0 = *t);
        a.for_each_ref_mut(RegRef::new(RegFile::GPR, 1, 1), |t| v1 = *t);
        assert_eq!(v0, 3);
        assert_eq!(v1, 3);
    }

    #[test]
    fn sparse_reg_tracker_retain() {
        let mut tracker = SparseRegTracker::<u32>::default();
        tracker.for_each_ref_mut(RegRef::new(RegFile::Pred, 0, 1), |t| *t = 1);
        tracker.for_each_ref_mut(RegRef::new(RegFile::Pred, 1, 1), |t| *t = 2);
        tracker.for_each_ref_mut(RegRef::new(RegFile::Pred, 2, 1), |t| *t = 3);
        tracker.retain(|t| *t != 2);
        let mut sum = 0;
        tracker.for_each_pred(|t| sum += *t);
        assert_eq!(
            sum, 4,
            "retain should remove pred 1 (value 2), leaving 1+3=4"
        );
    }

    #[test]
    fn sparse_reg_tracker_for_each_ref_mut_bar_skips() {
        let mut tracker = SparseRegTracker::<u32>::default();
        let mut count = 0;
        tracker.for_each_ref_mut(RegRef::new(RegFile::Bar, 0, 1), |_| count += 1);
        assert_eq!(count, 0, "Bar should be skipped");
    }

    #[test]
    #[should_panic(expected = "Not a register")]
    fn reg_tracker_index_mem_panics() {
        let tracker: RegTracker<u32> = RegTracker::new_with(&|| 0);
        let reg = RegRef::new(RegFile::Mem, 0, 1);
        let _ = &tracker[reg];
    }

    #[test]
    #[should_panic(expected = "Not a register")]
    fn sparse_reg_tracker_for_each_ref_mut_mem_panics() {
        let mut tracker = SparseRegTracker::<u32>::default();
        tracker.for_each_ref_mut(RegRef::new(RegFile::Mem, 0, 1), |_| {});
    }
}
