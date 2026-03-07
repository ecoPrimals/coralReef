// SPDX-License-Identifier: AGPL-3.0-or-later
//
// logsumexp_reduce.wgsl — Batched numerically-stable log-sum-exp (f32)
//
// Each thread computes logsumexp over one row of a [batch × width] matrix.
// Algorithm: max-subtract trick for numerical stability.
//   logsumexp(x) = max(x) + log(Σ exp(x_i - max(x)))
//
// Primary use: HMM forward/backward batching (Papers 016–018),
// log-likelihood computation in phylogenetics.
//
// Absorption target: barracuda::ops::reduce or StatefulPipeline extension

struct Params {
    batch: u32,
    width: u32,
}

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(256)
fn logsumexp_reduce(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = gid.x;
    if row >= params.batch { return; }

    let base = row * params.width;

    // Pass 1: find max for numerical stability
    var max_val: f32 = -3.4028235e+38;
    for (var i: u32 = 0u; i < params.width; i = i + 1u) {
        max_val = max(max_val, input[base + i]);
    }

    // Pass 2: accumulate exp(x - max)
    var sum_exp: f32 = 0.0;
    for (var i: u32 = 0u; i < params.width; i = i + 1u) {
        sum_exp = sum_exp + exp(input[base + i] - max_val);
    }

    output[row] = max_val + log(sum_exp);
}
