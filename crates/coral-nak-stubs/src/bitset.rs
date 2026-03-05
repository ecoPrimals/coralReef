// SPDX-License-Identifier: AGPL-3.0-only
//! Dense bit set backed by `u64` words.
//!
//! Replaces `compiler::bitset` from Mesa. Used by NAK for register tracking
//! and liveness sets where performance matters.
//!
//! The phantom type parameter `T` provides type-safe indices (e.g.
//! `BitSet<SSAValue>` vs `BitSet<Phi>`) without affecting the data layout.

use std::marker::PhantomData;
use std::ops::{BitOr, BitOrAssign, RangeFull, Sub};

/// Index trait for bit-set membership.
///
/// All implementors are `Copy` — methods accept `impl IntoBitIndex` by value.
pub trait IntoBitIndex: Copy {
    /// Convert to a zero-based bit index.
    fn bit_index(&self) -> usize;
}

impl IntoBitIndex for usize {
    fn bit_index(&self) -> usize {
        *self
    }
}

impl IntoBitIndex for u32 {
    fn bit_index(&self) -> usize {
        *self as usize
    }
}

impl IntoBitIndex for u16 {
    fn bit_index(&self) -> usize {
        usize::from(*self)
    }
}

const BITS_PER_WORD: usize = 64;

/// Dense bit set backed by a `Vec<u64>`.
///
/// The phantom type `T` provides type safety: `BitSet<SSAValue>` and
/// `BitSet<Phi>` are distinct types even though both store `usize` indices.
#[derive(Clone, PartialEq, Eq)]
pub struct BitSet<T = ()> {
    words: Vec<u64>,
    capacity: usize,
    _phantom: PhantomData<T>,
}

impl<T> std::fmt::Debug for BitSet<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BitSet")
            .field("capacity", &self.capacity)
            .field("len", &self.len())
            .finish_non_exhaustive()
    }
}

impl<T> Default for BitSet<T> {
    fn default() -> Self {
        Self::new(0)
    }
}

impl<T> BitSet<T> {
    /// Create a new bit set that can hold indices `[0, capacity)`.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let word_count = capacity.div_ceil(BITS_PER_WORD);
        Self {
            words: vec![0u64; word_count],
            capacity,
            _phantom: PhantomData,
        }
    }

    /// Insert a bit. Returns `true` if the bit was newly set.
    #[allow(clippy::needless_pass_by_value)] // IntoBitIndex: Copy
    pub fn insert(&mut self, index: impl IntoBitIndex) -> bool {
        let idx = index.bit_index();
        let (word, bit) = Self::word_bit(idx);
        if word >= self.words.len() {
            self.words.resize(word + 1, 0);
        }
        let was_clear = self.words[word] & bit == 0;
        self.words[word] |= bit;
        was_clear
    }

    /// Remove a bit. Returns `true` if the bit was previously set.
    #[allow(clippy::needless_pass_by_value)] // IntoBitIndex: Copy
    pub fn remove(&mut self, index: impl IntoBitIndex) -> bool {
        let idx = index.bit_index();
        let (word, bit) = Self::word_bit(idx);
        if word >= self.words.len() {
            return false;
        }
        let was_set = self.words[word] & bit != 0;
        self.words[word] &= !bit;
        was_set
    }

    /// Check if a bit is set.
    #[must_use]
    #[allow(clippy::needless_pass_by_value)] // IntoBitIndex: Copy
    pub fn contains(&self, index: impl IntoBitIndex) -> bool {
        let idx = index.bit_index();
        let (word, bit) = Self::word_bit(idx);
        word < self.words.len() && self.words[word] & bit != 0
    }

    /// Number of set bits (population count).
    #[must_use]
    pub fn len(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Whether no bits are set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }

    /// Capacity (maximum index + 1).
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Clear all bits.
    pub fn clear(&mut self) {
        self.words.fill(0);
    }

    /// Union with another set (self |= other).
    pub fn union(&mut self, other: &Self) {
        if other.words.len() > self.words.len() {
            self.words.resize(other.words.len(), 0);
        }
        for (a, &b) in self.words.iter_mut().zip(&other.words) {
            *a |= b;
        }
    }

    /// Alias for `union` (Mesa compatibility).
    pub fn union_with(&mut self, other: &Self) {
        self.union(other);
    }

    /// Slice view for bitwise expressions (Mesa compatibility). Returns `self`.
    #[must_use]
    pub fn s(&self, _: RangeFull) -> &Self {
        self
    }

    /// Intersection with another set (self &= other).
    pub fn intersect(&mut self, other: &Self) {
        for (i, word) in self.words.iter_mut().enumerate() {
            *word &= other.words.get(i).copied().unwrap_or(0);
        }
    }

    /// Difference (self &= !other).
    pub fn difference(&mut self, other: &Self) {
        for (a, &b) in self.words.iter_mut().zip(&other.words) {
            *a &= !b;
        }
    }

    /// Whether this set is a subset of `other`.
    #[must_use]
    pub fn is_subset_of(&self, other: &Self) -> bool {
        self.words.iter().enumerate().all(|(i, &w)| {
            let other_word = other.words.get(i).copied().unwrap_or(0);
            w & !other_word == 0
        })
    }

    /// Iterate over all set bit indices.
    #[must_use]
    pub fn iter(&self) -> BitSetIter<'_> {
        BitSetIter {
            words: &self.words,
            word_idx: 0,
            current: self.words.first().copied().unwrap_or(0),
        }
    }

    fn word_bit(index: usize) -> (usize, u64) {
        (index / BITS_PER_WORD, 1u64 << (index % BITS_PER_WORD))
    }

    /// Find an aligned range of `num_bits` unset bits starting at or after `start`.
    ///
    /// Returns the start index of the range if found, such that:
    /// - All bits in [`start_idx`, `start_idx` + `num_bits`) are unset
    /// - `start_idx` >= start
    /// - (`start_idx` - `align_offset`) % `align_mul` == 0
    ///
    /// Returns `None` if no such range exists.
    #[must_use]
    pub fn find_aligned_unset_range(
        &self,
        start: usize,
        num_bits: usize,
        align_mul: usize,
        align_offset: usize,
    ) -> Option<usize> {
        if num_bits == 0 {
            return Some(start);
        }
        let mut pos = start;
        loop {
            // Align pos: we need (pos - align_offset) % align_mul == 0
            let remainder = pos.wrapping_sub(align_offset) % align_mul;
            if remainder != 0 {
                pos += align_mul - remainder;
            }
            if pos + num_bits > self.capacity {
                return None;
            }
            // Check if all bits in [pos, pos + num_bits) are unset
            let mut all_unset = true;
            for i in 0..num_bits {
                if self.contains(pos + i) {
                    all_unset = false;
                    pos = pos + i + 1;
                    break;
                }
            }
            if all_unset {
                return Some(pos);
            }
        }
    }
}

impl<T> BitOr for &BitSet<T> {
    type Output = BitSet<T>;

    fn bitor(self, rhs: Self) -> BitSet<T> {
        let max_len = self.words.len().max(rhs.words.len());
        let mut words = Vec::with_capacity(max_len);
        for i in 0..max_len {
            let a = self.words.get(i).copied().unwrap_or(0);
            let b = rhs.words.get(i).copied().unwrap_or(0);
            words.push(a | b);
        }
        let capacity = self.capacity.max(rhs.capacity);
        BitSet {
            words,
            capacity,
            _phantom: PhantomData,
        }
    }
}

impl<T> Sub for &BitSet<T> {
    type Output = BitSet<T>;

    fn sub(self, rhs: Self) -> BitSet<T> {
        let max_len = self.words.len().max(rhs.words.len());
        let mut words = Vec::with_capacity(max_len);
        for i in 0..max_len {
            let a = self.words.get(i).copied().unwrap_or(0);
            let b = rhs.words.get(i).copied().unwrap_or(0);
            words.push(a & !b);
        }
        let capacity = self.capacity.max(rhs.capacity);
        BitSet {
            words,
            capacity,
            _phantom: PhantomData,
        }
    }
}

impl<T> Sub<&BitSet<T>> for BitSet<T> {
    type Output = BitSet<T>;

    fn sub(self, rhs: &BitSet<T>) -> BitSet<T> {
        &self - rhs
    }
}

impl<T> BitOrAssign<&BitSet<T>> for BitSet<T> {
    fn bitor_assign(&mut self, rhs: &BitSet<T>) {
        self.union(rhs);
    }
}

impl<'a, T> IntoIterator for &'a BitSet<T> {
    type Item = usize;
    type IntoIter = BitSetIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator over set bits in a [`BitSet`].
pub struct BitSetIter<'a> {
    words: &'a [u64],
    word_idx: usize,
    current: u64,
}

impl Iterator for BitSetIter<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        while self.current == 0 {
            self.word_idx += 1;
            if self.word_idx >= self.words.len() {
                return None;
            }
            self.current = self.words[self.word_idx];
        }
        let bit = self.current.trailing_zeros() as usize;
        self.current &= self.current - 1;
        Some(self.word_idx * BITS_PER_WORD + bit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_contains_remove() {
        let mut bs = BitSet::<()>::new(128);
        assert!(bs.insert(0_usize));
        assert!(bs.insert(42_usize));
        assert!(bs.insert(127_usize));
        assert!(!bs.insert(42_usize));
        assert!(bs.contains(0_usize));
        assert!(bs.contains(42_usize));
        assert!(bs.contains(127_usize));
        assert!(!bs.contains(1_usize));
        assert_eq!(bs.len(), 3);

        assert!(bs.remove(42_usize));
        assert!(!bs.remove(42_usize));
        assert!(!bs.contains(42_usize));
        assert_eq!(bs.len(), 2);
    }

    #[test]
    fn test_phantom_type() {
        struct Tag;
        let mut bs = BitSet::<Tag>::new(64);
        bs.insert(5_usize);
        assert!(bs.contains(5_usize));
    }

    #[test]
    fn test_u32_and_u16_index() {
        let mut bs = BitSet::<()>::new(64);
        bs.insert(10_u32);
        bs.insert(20_u16);
        assert!(bs.contains(10_u32));
        assert!(bs.contains(20_u16));
    }

    #[test]
    fn test_set_operations() {
        let mut set_a = BitSet::<()>::new(128);
        let mut set_b = BitSet::<()>::new(128);
        set_a.insert(1_usize);
        set_a.insert(2_usize);
        set_a.insert(3_usize);
        set_b.insert(2_usize);
        set_b.insert(3_usize);
        set_b.insert(4_usize);

        let mut union = set_a.clone();
        union.union(&set_b);
        assert_eq!(union.len(), 4);

        let mut intersection = set_a.clone();
        intersection.intersect(&set_b);
        assert_eq!(intersection.len(), 2);

        let mut diff = set_a.clone();
        diff.difference(&set_b);
        assert_eq!(diff.len(), 1);
        assert!(diff.contains(1_usize));
    }

    #[test]
    fn test_iter() {
        let mut bs = BitSet::<()>::new(256);
        bs.insert(0_usize);
        bs.insert(63_usize);
        bs.insert(64_usize);
        bs.insert(200_usize);
        let bits: Vec<usize> = bs.iter().collect();
        assert_eq!(bits, vec![0, 63, 64, 200]);
    }

    #[test]
    fn test_default_and_clear() {
        let bs = BitSet::<()>::default();
        assert!(bs.is_empty());
        assert_eq!(bs.len(), 0);
    }

    #[test]
    fn test_find_aligned_unset_range_empty_set() {
        let bs = BitSet::<()>::new(256);
        // Empty set: should find range at start with align 1
        assert_eq!(bs.find_aligned_unset_range(0, 4, 1, 0), Some(0));
        assert_eq!(bs.find_aligned_unset_range(10, 8, 1, 0), Some(10));
    }

    #[test]
    fn test_find_aligned_unset_range_with_alignment() {
        let mut bs = BitSet::<()>::new(128);
        bs.insert(0_usize);
        bs.insert(1_usize);
        bs.insert(2_usize);
        bs.insert(3_usize);
        // First 4 bits set; next unset range of 4 at align 4
        assert_eq!(bs.find_aligned_unset_range(0, 4, 4, 0), Some(4));
    }

    #[test]
    fn test_find_aligned_unset_range_align_offset() {
        let mut bs = BitSet::<()>::new(64);
        bs.insert(0_usize);
        bs.insert(1_usize);
        // Need (pos - 2) % 4 == 0, so pos in {2, 6, 10, ...}
        // First unset range of 2 starting at or after 0: pos 2 works
        assert_eq!(bs.find_aligned_unset_range(0, 2, 4, 2), Some(2));
    }

    #[test]
    fn test_find_aligned_unset_range_no_space() {
        let mut bs = BitSet::<()>::new(32);
        for i in 0_usize..32 {
            bs.insert(i);
        }
        assert_eq!(bs.find_aligned_unset_range(0, 1, 1, 0), None);
    }

    #[test]
    fn test_find_aligned_unset_range_zero_bits() {
        let bs = BitSet::<()>::new(64);
        assert_eq!(bs.find_aligned_unset_range(5, 0, 1, 0), Some(5));
    }

    #[test]
    fn test_empty_set_edge_case() {
        let bs = BitSet::<()>::new(0);
        assert!(bs.is_empty());
        assert_eq!(bs.len(), 0);
        assert!(!bs.contains(0_usize));
    }

    #[test]
    fn test_boundary_bits_word_boundary() {
        let mut bs = BitSet::<()>::new(128);
        bs.insert(63_usize);
        bs.insert(64_usize);
        assert!(bs.contains(63_usize));
        assert!(bs.contains(64_usize));
        assert_eq!(bs.len(), 2);
        let bits: Vec<usize> = bs.iter().collect();
        assert_eq!(bits, vec![63, 64]);
    }

    #[test]
    fn test_property_inserted_bit_is_found() {
        let mut bs = BitSet::<()>::new(256);
        for i in [0_usize, 1, 63, 64, 127, 200] {
            bs.insert(i);
            assert!(bs.contains(i), "inserted bit {i} should be found");
        }
    }

    #[test]
    fn test_property_removed_bit_not_found() {
        let mut bs = BitSet::<()>::new(64);
        bs.insert(42_usize);
        assert!(bs.contains(42_usize));
        bs.remove(42_usize);
        assert!(!bs.contains(42_usize));
    }

    #[test]
    fn test_clear_removes_all() {
        let mut bs = BitSet::<()>::new(128);
        for i in 0_usize..64 {
            bs.insert(i);
        }
        bs.clear();
        assert!(bs.is_empty());
        for i in 0_usize..64 {
            assert!(!bs.contains(i));
        }
    }

    #[test]
    fn test_is_subset_of() {
        let mut a = BitSet::<()>::new(64);
        let mut b = BitSet::<()>::new(64);
        a.insert(1_usize);
        a.insert(2_usize);
        b.insert(1_usize);
        b.insert(2_usize);
        b.insert(3_usize);
        assert!(a.is_subset_of(&b));
        a.insert(5_usize);
        assert!(!a.is_subset_of(&b));
    }
}
