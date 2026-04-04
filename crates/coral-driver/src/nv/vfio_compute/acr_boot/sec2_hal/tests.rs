// SPDX-License-Identifier: AGPL-3.0-only

use crate::vfio::channel::registers::falcon;

use super::{PTOP_ENGINE_SEC2, Sec2State, classify_sec2, pack_emem_word, parse_ptop_entries};

#[test]
fn classify_pri_error_cpuctl_is_inaccessible() {
    assert_eq!(classify_sec2(0xBADF_1234, 0, 0), Sec2State::Inaccessible);
}

#[test]
fn classify_badf_dead_cpuctl_is_inaccessible() {
    assert_eq!(classify_sec2(0xBADF_DEAD, 0, 0), Sec2State::Inaccessible);
}

#[test]
fn classify_mailbox_active_non_halted_is_running() {
    let cpuctl = 0x100 & !falcon::CPUCTL_HALTED;
    assert_eq!(classify_sec2(cpuctl, 0, 1), Sec2State::Running);
}

#[test]
fn classify_sctl_bit0_set_hs_locked() {
    assert_eq!(classify_sec2(0, 1, 0), Sec2State::HsLocked);
}

#[test]
fn classify_sctl_bit0_clear_clean_reset() {
    assert_eq!(classify_sec2(0, 0x1000, 0), Sec2State::CleanReset);
}

#[test]
fn classify_all_zeros_clean_reset() {
    assert_eq!(classify_sec2(0, 0, 0), Sec2State::CleanReset);
}

#[test]
fn classify_all_bits_set_cpuctl_inaccessible() {
    assert_eq!(classify_sec2(0xFFFF_FFFF, 0, 0), Sec2State::Inaccessible);
}

#[test]
fn pack_emem_word_four_bytes() {
    assert_eq!(pack_emem_word(&[0x01, 0x02, 0x03, 0x04]), 0x0403_0201);
}

#[test]
fn pack_emem_word_three_bytes() {
    assert_eq!(pack_emem_word(&[0xAA, 0xBB, 0xCC]), 0x00CC_BBAA);
}

#[test]
fn pack_emem_word_two_bytes() {
    assert_eq!(pack_emem_word(&[0x10, 0x20]), 0x0000_2010);
}

#[test]
fn pack_emem_word_one_byte() {
    assert_eq!(pack_emem_word(&[0x7F]), 0x0000_007F);
}

#[test]
fn pack_emem_word_empty() {
    assert_eq!(pack_emem_word(&[]), 0);
}

#[test]
fn pack_emem_word_overlong_chunk_returns_zero() {
    assert_eq!(pack_emem_word(&[1, 2, 3, 4, 5]), 0);
}

fn ptop_engine(engine: u32) -> u32 {
    1 | ((engine & 0x3FF) << 2)
}

fn ptop_reset_enable(has_reset: bool, has_enable: bool, reset_bit: u32, enable_bit: u32) -> u32 {
    let mut e = 3u32;
    if has_reset {
        e |= 1 << 14;
    }
    if has_enable {
        e |= 1 << 15;
    }
    e |= (reset_bit & 0x1F) << 16;
    e |= (enable_bit & 0x1F) << 21;
    e
}

#[test]
fn parse_ptop_sec2_reset_and_enable_prefers_enable() {
    let table = vec![
        ptop_engine(PTOP_ENGINE_SEC2),
        ptop_reset_enable(true, true, 17, 22),
    ];
    assert_eq!(parse_ptop_entries(&table, PTOP_ENGINE_SEC2), Some(22));
}

#[test]
fn parse_ptop_sec2_reset_only() {
    let table = vec![
        ptop_engine(PTOP_ENGINE_SEC2),
        ptop_reset_enable(true, false, 19, 0),
    ];
    assert_eq!(parse_ptop_entries(&table, PTOP_ENGINE_SEC2), Some(19));
}

#[test]
fn parse_ptop_no_sec2_engine_returns_none() {
    let table = vec![ptop_engine(0x10), ptop_reset_enable(true, true, 3, 4)];
    assert_eq!(parse_ptop_entries(&table, PTOP_ENGINE_SEC2), None);
}

#[test]
fn parse_ptop_empty_table_returns_none() {
    assert_eq!(parse_ptop_entries(&[], PTOP_ENGINE_SEC2), None);
}

#[test]
fn parse_ptop_all_zero_returns_none() {
    let table = vec![0u32; 64];
    assert_eq!(parse_ptop_entries(&table, PTOP_ENGINE_SEC2), None);
}

#[test]
fn parse_ptop_interleaved_engines() {
    let table = vec![
        ptop_engine(0x03),
        ptop_engine(PTOP_ENGINE_SEC2),
        ptop_reset_enable(true, true, 5, 11),
        ptop_engine(0x04),
    ];
    assert_eq!(parse_ptop_entries(&table, PTOP_ENGINE_SEC2), Some(11));
}

#[test]
fn parse_ptop_sec2_duplicate_engine_definitions_second_wins() {
    let table = vec![
        ptop_engine(PTOP_ENGINE_SEC2),
        ptop_reset_enable(true, false, 7, 0),
        ptop_engine(PTOP_ENGINE_SEC2),
        ptop_reset_enable(true, true, 0, 13),
    ];
    assert_eq!(parse_ptop_entries(&table, PTOP_ENGINE_SEC2), Some(13));
}

#[test]
fn parse_ptop_terminator_resets_scan_before_match() {
    let table = vec![
        0xFFFF_FFFF,
        ptop_engine(PTOP_ENGINE_SEC2),
        ptop_reset_enable(true, true, 4, 8),
    ];
    assert_eq!(parse_ptop_entries(&table, PTOP_ENGINE_SEC2), Some(8));
}
