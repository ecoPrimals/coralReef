// SPDX-License-Identifier: AGPL-3.0-or-later
//
// pairwise_l2.wgsl — Pairwise L2 distance matrix (Paper 012 MODES)
//
// Each thread computes the L2 (Euclidean) distance between one pair
// of feature vectors.  N vectors of dimension D produce N*(N-1)/2
// pairwise distances (upper triangle only). Core of novelty_metric.
//
// Absorption target: barracuda::ops::pairwise_distance

struct Params {
    n: u32,
    dim: u32,
}

@group(0) @binding(0) var<storage, read> features: array<f32>;
@group(0) @binding(1) var<storage, read_write> distances: array<f32>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(256)
fn pairwise_l2(@builtin(global_invocation_id) gid: vec3<u32>) {
    let pair_idx = gid.x;
    let n = params.n;
    let dim = params.dim;
    let n_pairs = n * (n - 1u) / 2u;
    if pair_idx >= n_pairs {
        return;
    }

    // Decode pair_idx to (i, j) where i < j (same scheme as pairwise_hamming)
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

    var sum_sq: f32 = 0.0;
    let offset_i = i * dim;
    let offset_j = j * dim;

    for (var d: u32 = 0u; d < dim; d = d + 1u) {
        let diff = features[offset_i + d] - features[offset_j + d];
        sum_sq = sum_sq + diff * diff;
    }

    distances[pair_idx] = sqrt(sum_sq);
}
