// SPDX-License-Identifier: AGPL-3.0-only
//! Fast, non-cryptographic hash — drop-in replacement for `rustc-hash`.
//!
//! FxHash is a speedy hash algorithm used within rustc. The implementation
//! is a Fowler–Noll–Vo-style multiply-xor-shift, using a single 64-bit
//! constant. It's deterministic, not HashDoS-resistant, and ideal for
//! compiler-internal data structures where keys are small integers or
//! pointers.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

const SEED: u64 = 0x517c_c1b7_2722_0a95;

/// A fast, non-cryptographic hasher matching `rustc-hash`'s `FxHasher`.
#[derive(Default)]
pub struct FxHasher {
    hash: u64,
}

impl FxHasher {
    #[inline]
    const fn add_to_hash(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut buf = [0u8; 8];
            buf[..chunk.len()].copy_from_slice(chunk);
            self.add_to_hash(u64::from_ne_bytes(buf));
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add_to_hash(u64::from(i));
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add_to_hash(u64::from(i));
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add_to_hash(u64::from(i));
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add_to_hash(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

/// Build-hasher for [`FxHasher`], matching `rustc_hash::FxBuildHasher`.
pub type FxBuildHasher = BuildHasherDefault<FxHasher>;

/// Hash map using [`FxHasher`], matching `rustc_hash::FxHashMap`.
pub type FxHashMap<K, V> = HashMap<K, V, FxBuildHasher>;

/// Hash set using [`FxHasher`], matching `rustc_hash::FxHashSet`.
pub type FxHashSet<T> = HashSet<T, FxBuildHasher>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let mut a = FxHasher::default();
        let mut b = FxHasher::default();
        a.write_u64(42);
        b.write_u64(42);
        assert_eq!(a.finish(), b.finish());
    }

    #[test]
    fn different_inputs_differ() {
        let mut a = FxHasher::default();
        let mut b = FxHasher::default();
        a.write_u64(1);
        b.write_u64(2);
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn map_and_set_work() {
        let mut map: FxHashMap<u32, &str> = FxHashMap::default();
        map.insert(1, "one");
        map.insert(2, "two");
        assert_eq!(map.get(&1), Some(&"one"));

        let mut set: FxHashSet<u32> = FxHashSet::default();
        set.insert(42);
        assert!(set.contains(&42));
    }

    #[test]
    fn build_hasher_is_default() {
        let bh = FxBuildHasher::default();
        let _: FxHashMap<String, u64> = HashMap::with_hasher(bh);
    }
}
