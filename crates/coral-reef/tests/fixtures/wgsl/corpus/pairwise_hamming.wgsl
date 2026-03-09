// SPDX-License-Identifier: AGPL-3.0-or-later
//
// pairwise_hamming.wgsl — Pairwise Hamming distance matrix (Paper 017)
//
// Each thread computes the Hamming distance (proportion of differing
// sites) between one pair of sequences.  N sequences of length L
// produce N*(N-1)/2 pairwise distances.
//
// Absorption target: barracuda::ops::pairwise_distance or cdist_wgsl

struct Params {
    n_seqs: u32,
    seq_len: u32,
}

@group(0) @binding(0) var<storage, read> sequences: array<u32>;
@group(0) @binding(1) var<storage, read_write> distances: array<f32>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(256)
fn pairwise_hamming(@builtin(global_invocation_id) gid: vec3<u32>) {
    let pair_idx = gid.x;
    let n = params.n_seqs;
    let n_pairs = n * (n - 1u) / 2u;
    if pair_idx >= n_pairs { return; }

    // Decode pair_idx to (i, j) where i < j
    // pair_idx = i*(2n - i - 1)/2 + (j - i - 1)
    var i: u32 = 0u;
    var running: u32 = 0u;
    for (var k: u32 = 0u; k < n - 1u; k = k + 1u) {
        let count = n - 1u - k;
        if running + count > pair_idx {
            i = k;
            break;
        }
        running = running + count;
    }
    let j = pair_idx - running + i + 1u;

    var diff: u32 = 0u;
    let offset_i = i * params.seq_len;
    let offset_j = j * params.seq_len;

    for (var s: u32 = 0u; s < params.seq_len; s = s + 1u) {
        if sequences[offset_i + s] != sequences[offset_j + s] {
            diff = diff + 1u;
        }
    }

    distances[pair_idx] = f32(diff) / f32(params.seq_len);
}
