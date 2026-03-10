// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//
// coralReef PRNG preamble — xorshift32 and wang_hash.
// Auto-prepended when WGSL source uses xorshift32 or wang_hash.

fn xorshift32(state: ptr<function, u32>) -> u32 {
    var x = *state;
    x = x ^ (x << 13u);
    x = x ^ (x >> 17u);
    x = x ^ (x << 5u);
    *state = x;
    return x;
}

fn wang_hash(seed: u32) -> u32 {
    var x = seed;
    x = (x ^ 61u) ^ (x >> 16u);
    x = x * 9u;
    x = x ^ (x >> 4u);
    x = x * 0x27d4eb2du;
    x = x ^ (x >> 15u);
    return x;
}
