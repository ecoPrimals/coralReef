// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bit-field writers for GPU QMD word layouts.

use super::types::QMD_SIZE_WORDS;

/// Helper: set a bitfield within the QMD word array.
///
/// `bit_start` is the starting bit (0-indexed from LSB of word 0),
/// `width` is the field width in bits, `value` is the value to set.
#[expect(
    clippy::cast_possible_truncation,
    reason = "GPU QMD fields are always ≤32 bits"
)]
pub(crate) const fn qmd_set_field(
    q: &mut [u32; QMD_SIZE_WORDS],
    bit_start: usize,
    width: usize,
    value: u64,
) {
    let word_idx = bit_start / 32;
    let bit_off = bit_start % 32;

    if bit_off + width <= 32 {
        let mask = if width >= 32 {
            u32::MAX
        } else {
            (1u32 << width) - 1
        };
        q[word_idx] &= !(mask << bit_off);
        q[word_idx] |= ((value as u32) & mask) << bit_off;
    } else {
        let lo_bits = 32 - bit_off;
        let lo_mask = u32::MAX << bit_off;
        q[word_idx] = (q[word_idx] & !lo_mask) | ((value as u32) << bit_off);

        let hi_bits = width - lo_bits;
        let hi_mask = if hi_bits >= 32 {
            u32::MAX
        } else {
            (1u32 << hi_bits) - 1
        };
        q[word_idx + 1] = (q[word_idx + 1] & !hi_mask) | (((value >> lo_bits) as u32) & hi_mask);
    }
}

/// Dynamic-size variant of `qmd_set_field` for Vec-backed QMDs.
#[expect(
    clippy::cast_possible_truncation,
    reason = "GPU QMD fields are always ≤32 bits"
)]
pub(crate) fn qmd_set_field_dyn(q: &mut [u32], bit_start: usize, width: usize, value: u64) {
    let word_idx = bit_start / 32;
    let bit_off = bit_start % 32;
    if word_idx >= q.len() {
        return;
    }

    if bit_off + width <= 32 {
        let mask = if width >= 32 {
            u32::MAX
        } else {
            (1u32 << width) - 1
        };
        q[word_idx] &= !(mask << bit_off);
        q[word_idx] |= ((value as u32) & mask) << bit_off;
    } else {
        let lo_bits = 32 - bit_off;
        let lo_mask = u32::MAX << bit_off;
        q[word_idx] = (q[word_idx] & !lo_mask) | ((value as u32) << bit_off);

        if word_idx + 1 < q.len() {
            let hi_bits = width - lo_bits;
            let hi_mask = if hi_bits >= 32 {
                u32::MAX
            } else {
                (1u32 << hi_bits) - 1
            };
            q[word_idx + 1] =
                (q[word_idx + 1] & !hi_mask) | (((value >> lo_bits) as u32) & hi_mask);
        }
    }
}
