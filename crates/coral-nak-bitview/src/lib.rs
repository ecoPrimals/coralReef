// SPDX-License-Identifier: AGPL-3.0-only
//! Bit-level field access for GPU instruction encoding.
//!
//! Replaces Mesa's `bitview` crate. Provides zero-copy bit-level read/write
//! over `[u32]` buffers, used by NAK's instruction encoders and SPH.
//!
//! # Traits
//!
//! - `BitViewable` — read-only: `bits()`, `get_bit()`, `get_field()`
//! - `BitMutViewable` — read-write: extends `BitViewable` with `set_bit()`,
//!   `set_field()`
//!
//! Mesa compat aliases: `BitView = BitViewable`, `BitMutView = BitMutViewable`,
//! `SetBit`, `SetField`.

use std::fmt;
use std::ops::Range;

/// Read-only bit-level view trait.
pub trait BitViewable {
    /// Total number of bits.
    fn bits(&self) -> usize;

    /// Read a range of bits as a `u64`.
    fn get_bit_range_u64(&self, range: Range<usize>) -> u64;

    /// Read a single bit.
    #[must_use]
    fn get_bit(&self, bit: usize) -> bool {
        self.get_bit_range_u64(bit..bit + 1) != 0
    }

    /// Read a field specified by a `Range<usize>`.
    #[must_use]
    fn get_field(&self, range: Range<usize>) -> u64 {
        self.get_bit_range_u64(range)
    }
}

/// Mutable bit-level view trait.
///
/// `set_field` accepts any value convertible to `u64` (`u8`, `u16`, `u32`,
/// `u64`, `i8`, `i16`, `i32`, `bool`).
pub trait BitMutViewable: BitViewable {
    /// Write a range of bits from a `u64` value.
    fn set_bit_range_u64(&mut self, range: Range<usize>, val: u64);

    /// Set a single bit.
    fn set_bit(&mut self, bit: usize, value: bool) {
        self.set_bit_range_u64(bit..bit + 1, u64::from(value));
    }

    /// Write a field. Accepts any type with `BitCastU64`.
    fn set_field(&mut self, range: Range<usize>, value: impl BitCastU64) {
        self.set_bit_range_u64(range, value.as_bits());
    }

    /// Write a value split across two non-contiguous ranges (low bits in range1, high in range2).
    fn set_field2(&mut self, range1: Range<usize>, range2: Range<usize>, value: impl BitCastU64) {
        let val = value.as_bits();
        let len1 = range1.len();
        let mask1 = if len1 >= 64 {
            u64::MAX
        } else {
            (1u64 << len1) - 1
        };
        self.set_bit_range_u64(range1, val & mask1);
        self.set_bit_range_u64(range2, val >> len1);
    }
}

/// Conversion to `u64` bit pattern, supporting both unsigned and signed types.
///
/// Unlike `Into<u64>`, this handles signed integers by taking their bit
/// representation (two's complement truncated to the field width).
pub trait BitCastU64 {
    /// Bit pattern as u64.
    fn as_bits(self) -> u64;
}

impl BitCastU64 for u64 {
    fn as_bits(self) -> u64 {
        self
    }
}
impl BitCastU64 for u32 {
    fn as_bits(self) -> u64 {
        u64::from(self)
    }
}
impl BitCastU64 for u16 {
    fn as_bits(self) -> u64 {
        u64::from(self)
    }
}
impl BitCastU64 for u8 {
    fn as_bits(self) -> u64 {
        u64::from(self)
    }
}
impl BitCastU64 for i64 {
    fn as_bits(self) -> u64 {
        self as u64
    }
}
impl BitCastU64 for i32 {
    fn as_bits(self) -> u64 {
        u64::from(self as u32)
    }
}
impl BitCastU64 for i16 {
    fn as_bits(self) -> u64 {
        u64::from(self as u16)
    }
}
impl BitCastU64 for i8 {
    fn as_bits(self) -> u64 {
        u64::from(self as u8)
    }
}
impl BitCastU64 for bool {
    fn as_bits(self) -> u64 {
        u64::from(self)
    }
}
impl BitCastU64 for usize {
    fn as_bits(self) -> u64 {
        self as u64
    }
}

/// Mesa compatibility alias.
pub trait BitView: BitViewable {}
impl<T: BitViewable + ?Sized> BitView for T {}

/// Mesa compatibility alias.
pub trait BitMutView: BitMutViewable {}
impl<T: BitMutViewable + ?Sized> BitMutView for T {}

/// NAK macro compat — marker trait.
pub trait SetBit: BitMutViewable {}
impl<T: BitMutViewable + ?Sized> SetBit for T {}

/// NAK macro compat — marker trait.
pub trait SetField: BitMutViewable {}
impl<T: BitMutViewable + ?Sized> SetField for T {}

/// A mutable subset view into a larger bit buffer.
pub struct BitMutSubsetView<'a> {
    data: &'a mut [u32],
    offset: usize,
    num_bits: usize,
}

impl<'a> BitMutSubsetView<'a> {
    /// Create a subset view starting at `offset` bits with `num_bits` width.
    pub fn new(data: &'a mut [u32], offset: usize, num_bits: usize) -> Self {
        Self {
            data,
            offset,
            num_bits,
        }
    }
}

impl BitViewable for BitMutSubsetView<'_> {
    fn bits(&self) -> usize {
        self.num_bits
    }

    fn get_bit_range_u64(&self, range: Range<usize>) -> u64 {
        let shifted = (range.start + self.offset)..(range.end + self.offset);
        self.data.get_bit_range_u64(shifted)
    }
}

impl BitMutViewable for BitMutSubsetView<'_> {
    fn set_bit_range_u64(&mut self, range: Range<usize>, val: u64) {
        let shifted = (range.start + self.offset)..(range.end + self.offset);
        self.data.set_bit_range_u64(shifted, val);
    }
}

/// Create a subset view into a mutable word buffer.
pub fn new_subset(data: &mut [u32], offset: usize, num_bits: usize) -> BitMutSubsetView<'_> {
    BitMutSubsetView::new(data, offset, num_bits)
}

// ---------------------------------------------------------------------------
// Core implementations
// ---------------------------------------------------------------------------

fn u32_slice_get_bit_range(words: &[u32], range: Range<usize>) -> u64 {
    if range.is_empty() {
        return 0;
    }
    let width = range.end - range.start;
    debug_assert!(width <= 64, "field width {width} exceeds u64");
    let mut result = 0u64;
    for i in 0..width {
        let pos = range.start + i;
        let word_idx = pos / 32;
        let bit_idx = pos % 32;
        if let Some(&word) = words.get(word_idx) {
            if word & (1 << bit_idx) != 0 {
                result |= 1u64 << i;
            }
        }
    }
    result
}

fn u32_slice_set_bit_range(words: &mut [u32], range: Range<usize>, val: u64) {
    if range.is_empty() {
        return;
    }
    let width = range.end - range.start;
    debug_assert!(width <= 64, "field width {width} exceeds u64");
    for i in 0..width {
        let pos = range.start + i;
        let word_idx = pos / 32;
        let bit_idx = pos % 32;
        if let Some(word) = words.get_mut(word_idx) {
            if val & (1u64 << i) != 0 {
                *word |= 1 << bit_idx;
            } else {
                *word &= !(1 << bit_idx);
            }
        }
    }
}

impl BitViewable for [u32] {
    fn bits(&self) -> usize {
        self.len() * 32
    }
    fn get_bit_range_u64(&self, range: Range<usize>) -> u64 {
        u32_slice_get_bit_range(self, range)
    }
}

impl BitMutViewable for [u32] {
    fn set_bit_range_u64(&mut self, range: Range<usize>, val: u64) {
        u32_slice_set_bit_range(self, range, val);
    }
}

impl BitViewable for Vec<u32> {
    fn bits(&self) -> usize {
        self.len() * 32
    }
    fn get_bit_range_u64(&self, range: Range<usize>) -> u64 {
        u32_slice_get_bit_range(self, range)
    }
}

impl BitMutViewable for Vec<u32> {
    fn set_bit_range_u64(&mut self, range: Range<usize>, val: u64) {
        u32_slice_set_bit_range(self, range, val);
    }
}

impl<const N: usize> BitViewable for [u32; N] {
    fn bits(&self) -> usize {
        N * 32
    }
    fn get_bit_range_u64(&self, range: Range<usize>) -> u64 {
        u32_slice_get_bit_range(self, range)
    }
}

impl<const N: usize> BitMutViewable for [u32; N] {
    fn set_bit_range_u64(&mut self, range: Range<usize>, val: u64) {
        u32_slice_set_bit_range(self, range, val);
    }
}

impl BitViewable for u32 {
    fn bits(&self) -> usize {
        32
    }
    fn get_bit_range_u64(&self, range: Range<usize>) -> u64 {
        u32_slice_get_bit_range(std::slice::from_ref(self), range)
    }
}

impl BitMutViewable for u32 {
    fn set_bit_range_u64(&mut self, range: Range<usize>, val: u64) {
        u32_slice_set_bit_range(std::slice::from_mut(self), range, val);
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

/// Hex display wrapper for a word buffer.
pub struct BitViewDisplay<'a>(pub &'a [u32]);

impl fmt::Display for BitViewDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, word) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            write!(f, "{word:08x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for BitViewDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_set_bit() {
        let mut buf = [0u32; 4];
        assert!(!buf.get_bit(0));
        buf.set_bit(0, true);
        assert!(buf.get_bit(0));
        buf.set_bit(31, true);
        assert!(buf.get_bit(31));
        buf.set_bit(32, true);
        assert!(buf.get_bit(32));
        buf.set_bit(0, false);
        assert!(!buf.get_bit(0));
    }

    #[test]
    fn test_get_set_field() {
        let mut buf = [0u32; 4];
        buf.set_field(0..8, 0xFFu32);
        assert_eq!(buf.get_field(0..8), 0xFF);

        buf.set_field(4..12, 0xABu32);
        assert_eq!(buf.get_field(4..12), 0xAB);

        let mut buf2 = [0u32; 2];
        buf2.set_field(24..40, 0xBEEFu64);
        assert_eq!(buf2.get_field(24..40), 0xBEEF);
    }

    #[test]
    fn test_set_field_signed() {
        let mut buf = [0u32; 2];
        buf.set_field(0..32, -1i32);
        assert_eq!(buf.get_field(0..32), 0xFFFF_FFFF);
    }

    #[test]
    fn test_u32_bitview() {
        let mut val = 0u32;
        val.set_bit(0, true);
        assert!(val.get_bit(0));
        val.set_field(4..8, 0xFu32);
        assert_eq!(val.get_field(4..8), 0xF);
    }

    #[test]
    fn test_subset_view() {
        let mut buf = [0u32; 4];
        let mut sub = BitMutSubsetView::new(&mut buf, 32, 32);
        sub.set_field(0..8, 0xABu32);
        assert_eq!(sub.get_field(0..8), 0xAB);
        assert_eq!(buf[1], 0xAB);
    }

    #[test]
    fn test_display() {
        let buf = [0xDEAD_BEEFu32, 0xCAFE_BABEu32];
        let display = format!("{}", BitViewDisplay(&buf));
        assert_eq!(display, "deadbeef cafebabe");
    }

    #[test]
    fn test_get_set_bit_range_across_word_boundary() {
        let mut buf = [0u32; 4];
        // Set bits 28..36 (8 bits, spans word 0 and word 1)
        buf.set_field(28..36, 0xFFu64);
        assert_eq!(buf.get_field(28..36), 0xFF);
        assert!(buf.get_bit(28));
        assert!(buf.get_bit(31));
        assert!(buf.get_bit(32));
        assert!(buf.get_bit(35));
    }

    #[test]
    fn test_field_across_two_words() {
        let mut buf = [0u32; 4];
        buf.set_field(24..56, 0x1234_5678u64);
        assert_eq!(buf.get_field(24..56), 0x1234_5678);
    }

    #[test]
    fn test_subset_view_offset() {
        let mut buf = [0u32; 4];
        let mut sub = BitMutSubsetView::new(&mut buf, 64, 64);
        sub.set_field(0..32, 0xDEADBEEFu32);
        assert_eq!(sub.get_field(0..32), 0xDEADBEEF);
        assert_eq!(buf[2], 0xDEADBEEF);
    }

    #[test]
    fn test_subset_view_partial_word() {
        let mut buf = [0u32; 4];
        let mut sub = BitMutSubsetView::new(&mut buf, 16, 32);
        sub.set_field(0..16, 0xABCDu32);
        assert_eq!(sub.get_field(0..16), 0xABCD);
        assert_eq!(buf[0] >> 16, 0xABCD);
    }

    #[test]
    fn test_edge_case_zero_width_range() {
        let mut buf = [0u32; 2];
        buf.set_field(8..8, 0xFFFFu64);
        assert_eq!(buf.get_field(8..8), 0);
        assert_eq!(buf.get_bit_range_u64(8..8), 0);
    }

    #[test]
    fn test_edge_case_full_width_single_word() {
        let mut buf = [0u32; 2];
        buf.set_field(0..32, 0xFFFFFFFFu32);
        assert_eq!(buf.get_field(0..32), 0xFFFFFFFF);
        assert_eq!(buf[0], 0xFFFFFFFF);
    }

    #[test]
    fn test_edge_case_full_width_two_words() {
        let mut buf = [0u32; 2];
        buf.set_field(0..64, 0xFFFF_FFFF_FFFF_FFFFu64);
        assert_eq!(buf.get_field(0..64), 0xFFFF_FFFF_FFFF_FFFF);
        assert_eq!(buf[0], 0xFFFFFFFF);
        assert_eq!(buf[1], 0xFFFFFFFF);
    }

    #[test]
    fn test_set_field2_split_ranges() {
        let mut buf = [0u32; 4];
        buf.set_field2(0..8, 32..40, 0x1234u64);
        assert_eq!(buf.get_field(0..8), 0x34);
        assert_eq!(buf.get_field(32..40), 0x12);
    }

    #[test]
    fn test_bits_total() {
        let buf = [0u32; 4];
        assert_eq!(buf.bits(), 128);
        let mut data = [0u32; 2];
        let sub = BitMutSubsetView::new(&mut data, 0, 48);
        assert_eq!(sub.bits(), 48);
    }
}
