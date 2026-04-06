// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

/// FNV-1a 64-bit offset basis.
const FNV1A_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const FNV1A_PRIME: u64 = 0x0100_0000_01b3;

/// Compute a fast non-cryptographic hash of WGSL source (FNV-1a 64-bit).
pub fn hash_wgsl(wgsl: &str) -> u64 {
    let mut hash = FNV1A_OFFSET_BASIS;
    for byte in wgsl.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV1A_PRIME);
    }
    hash
}
