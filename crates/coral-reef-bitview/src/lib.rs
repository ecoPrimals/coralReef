// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! Bit-level field access for GPU instruction encoding.
//!
//! Replaces upstream `bitview` crate. Provides zero-copy bit-level read/write
//! over `[u32]` buffers, used by instruction encoders and Shader Program Header.
//!
//! # Traits
//!
//! - `BitViewable` — read-only: `bits()`, `get_bit()`, `get_field()`
//! - `BitMutViewable` — read-write: extends `BitViewable` with `set_bit()`,
//!   `set_field()`
//!
//! Compat aliases: `BitView = BitViewable`, `BitMutView = BitMutViewable`,
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
pub trait BitCastU64: Copy {
    /// Bit pattern as u64.
    ///
    /// Takes `self` by value because all implementors are `Copy`.
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
    #[expect(clippy::cast_sign_loss, reason = "bit-pattern reinterpretation")]
    fn as_bits(self) -> u64 {
        self as u64
    }
}
impl BitCastU64 for i32 {
    #[expect(clippy::cast_sign_loss, reason = "bit-pattern reinterpretation")]
    fn as_bits(self) -> u64 {
        u64::from(self as u32)
    }
}
impl BitCastU64 for i16 {
    #[expect(clippy::cast_sign_loss, reason = "bit-pattern reinterpretation")]
    fn as_bits(self) -> u64 {
        u64::from(self as u16)
    }
}
impl BitCastU64 for i8 {
    #[expect(clippy::cast_sign_loss, reason = "bit-pattern reinterpretation")]
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

/// Compat alias.
pub trait BitView: BitViewable {}
impl<T: BitViewable + ?Sized> BitView for T {}

/// Compat alias.
pub trait BitMutView: BitMutViewable {}
impl<T: BitMutViewable + ?Sized> BitMutView for T {}

/// Macro compat — marker trait.
pub trait SetBit: BitMutViewable {}
impl<T: BitMutViewable + ?Sized> SetBit for T {}

/// Macro compat — marker trait.
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
    pub const fn new(data: &'a mut [u32], offset: usize, num_bits: usize) -> Self {
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
pub const fn new_subset(data: &mut [u32], offset: usize, num_bits: usize) -> BitMutSubsetView<'_> {
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

// ---------------------------------------------------------------------------
// Const-generic typed bit fields
// ---------------------------------------------------------------------------

/// A compile-time bit field descriptor.
///
/// Encodes the offset and width of an instruction field at the type level,
/// enabling the Rust compiler to catch encoding errors statically.
///
/// # Example
///
/// ```
/// use bitview::TypedBitField;
///
/// type OpField = TypedBitField<18, 7>;
/// let mut words = [0u32; 2];
/// OpField::set(&mut words, 42);
/// assert_eq!(OpField::get(&words), 42);
/// ```
pub struct TypedBitField<const OFFSET: u32, const WIDTH: u32>;

impl<const OFFSET: u32, const WIDTH: u32> TypedBitField<OFFSET, WIDTH> {
    /// Maximum value this field can hold.
    pub const MAX_VALUE: u64 = if WIDTH >= 64 {
        u64::MAX
    } else {
        (1u64 << WIDTH) - 1
    };

    /// Bit range as `start..end`.
    pub const RANGE: std::ops::Range<usize> = (OFFSET as usize)..((OFFSET + WIDTH) as usize);

    /// Read this field from a word buffer.
    #[must_use]
    pub fn get(buf: &(impl BitViewable + ?Sized)) -> u64 {
        buf.get_bit_range_u64(Self::RANGE)
    }

    /// Write this field into a mutable word buffer.
    ///
    /// In debug builds, panics if `value` exceeds `MAX_VALUE`.
    pub fn set(buf: &mut (impl BitMutViewable + ?Sized), value: impl BitCastU64) {
        let bits = value.as_bits();
        debug_assert!(
            bits <= Self::MAX_VALUE,
            "value {bits:#x} exceeds {WIDTH}-bit field max {:#x}",
            Self::MAX_VALUE
        );
        buf.set_bit_range_u64(Self::RANGE, bits);
    }
}

/// A const-generic instruction word builder.
///
/// Accumulates field writes into a fixed-size `[u32; N]` buffer.
/// All field access goes through `TypedBitField` for compile-time safety.
///
/// # Example
///
/// ```
/// use bitview::{InstrBuilder, TypedBitField};
///
/// type Prefix = TypedBitField<26, 6>;
/// type Op = TypedBitField<16, 10>;
/// type Dst = TypedBitField<0, 8>;
///
/// let mut builder = InstrBuilder::<2>::new();
/// Prefix::set(&mut builder.words, 0b110101u32);
/// Op::set(&mut builder.words, 356u32);
/// Dst::set(&mut builder.words, 0u32);
/// assert_eq!(builder.words[0] >> 26 & 0x3F, 0b110101);
/// ```
pub struct InstrBuilder<const N: usize> {
    /// The instruction word buffer.
    pub words: [u32; N],
}

impl<const N: usize> InstrBuilder<N> {
    /// Create a zero-initialized builder.
    #[must_use]
    pub const fn new() -> Self {
        Self { words: [0; N] }
    }

    /// Consume the builder, returning the instruction words.
    #[must_use]
    pub const fn into_words(self) -> [u32; N] {
        self.words
    }
}

impl<const N: usize> Default for InstrBuilder<N> {
    fn default() -> Self {
        Self::new()
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
        sub.set_field(0..32, 0xDEAD_BEEFu32);
        assert_eq!(sub.get_field(0..32), 0xDEAD_BEEF);
        assert_eq!(buf[2], 0xDEAD_BEEF);
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
        buf.set_field(0..32, 0xFFFF_FFFFu32);
        assert_eq!(buf.get_field(0..32), 0xFFFF_FFFF);
        assert_eq!(buf[0], 0xFFFF_FFFF);
    }

    #[test]
    fn test_edge_case_full_width_two_words() {
        let mut buf = [0u32; 2];
        buf.set_field(0..64, 0xFFFF_FFFF_FFFF_FFFFu64);
        assert_eq!(buf.get_field(0..64), 0xFFFF_FFFF_FFFF_FFFF);
        assert_eq!(buf[0], 0xFFFF_FFFF);
        assert_eq!(buf[1], 0xFFFF_FFFF);
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

    #[test]
    fn typed_bitfield_get_set() {
        type Prefix = TypedBitField<26, 6>;
        type Opcode = TypedBitField<16, 10>;
        type Dst = TypedBitField<0, 8>;

        let mut words = [0u32; 2];
        Prefix::set(&mut words, 0b11_0101_u32);
        Opcode::set(&mut words, 356u32);
        Dst::set(&mut words, 42u32);

        assert_eq!(Prefix::get(&words), 0b11_0101);
        assert_eq!(Opcode::get(&words), 356);
        assert_eq!(Dst::get(&words), 42);
    }

    #[test]
    fn typed_bitfield_max_value() {
        assert_eq!(TypedBitField::<0, 1>::MAX_VALUE, 1);
        assert_eq!(TypedBitField::<0, 8>::MAX_VALUE, 255);
        assert_eq!(TypedBitField::<0, 16>::MAX_VALUE, 65535);
        assert_eq!(TypedBitField::<0, 32>::MAX_VALUE, 0xFFFF_FFFF);
    }

    #[test]
    fn typed_bitfield_cross_word() {
        type CrossField = TypedBitField<28, 8>;
        let mut words = [0u32; 2];
        CrossField::set(&mut words, 0xABu32);
        assert_eq!(CrossField::get(&words), 0xAB);
    }

    #[test]
    fn instr_builder_roundtrip() {
        type Op = TypedBitField<18, 7>;
        type Enc = TypedBitField<26, 6>;

        let mut b = InstrBuilder::<2>::new();
        Enc::set(&mut b.words, 0b11_0111_u32);
        Op::set(&mut b.words, 12u32);
        let w = b.into_words();
        assert_eq!((w[0] >> 26) & 0x3F, 0b11_0111);
        assert_eq!((w[0] >> 18) & 0x7F, 12);
    }

    #[test]
    #[should_panic(expected = "exceeds")]
    fn typed_bitfield_overflow_panics_in_debug() {
        type SmallField = TypedBitField<0, 3>;
        let mut words = [0u32; 1];
        SmallField::set(&mut words, 8u32); // 8 > max 7 for 3-bit field
    }
}
