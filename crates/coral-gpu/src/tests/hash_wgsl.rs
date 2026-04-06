// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

//! WGSL source hashing (determinism and FNV-1a baseline).

use super::common::EMPTY_WGSL_FNV1A_HASH;

#[test]
fn hash_deterministic() {
    let a = crate::hash::hash_wgsl("hello");
    let b = crate::hash::hash_wgsl("hello");
    let c = crate::hash::hash_wgsl("world");
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn hash_wgsl_empty_matches_fnv_offset_basis() {
    assert_eq!(crate::hash::hash_wgsl(""), EMPTY_WGSL_FNV1A_HASH);
}
