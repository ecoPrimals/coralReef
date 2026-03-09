// SPDX-License-Identifier: AGPL-3.0-or-later
//
// triangle_attention_f64.wgsl — Triangle self-attention with pair bias
//
// From Jumper et al. 2021 (AlphaFold2), Algorithms 13-14.
// Performs row-wise self-attention on pair representation z[i,j,c]
// with additive pair bias b[j,k]:
//
//   For each row i:
//     Q[j,h] = linear_q(z[i,j])  (query projection)
//     K[k,h] = linear_k(z[i,k])  (key projection)
//     V[k,h] = linear_v(z[i,k])  (value projection)
//     logit[j,k] = Q[j,h] * K[k,h] / sqrt(c_h) + bias[j,k]
//     w[j,k] = softmax_k(logit[j,k]) * gate[j]
//     z[i,j] += sum_k w[j,k] * V[k,h]
//
// This shader computes the biased attention scores (pass 1).
// Three-zone core streaming: f64 buffer I/O, df64 compute, f64 output.
//
// Absorption target: barracuda::ops::triangle_attention_f64
// Requires: df64_core.wgsl + df64_transcendentals.wgsl (prepended via compile_shader_f64)

struct Params {
    n_rows:   u32,
    n_res:    u32,
    n_heads:  u32,
    head_dim: u32,
}

@group(0) @binding(0) var<storage, read>       query: array<f64>;
@group(0) @binding(1) var<storage, read>       key:   array<f64>;
@group(0) @binding(2) var<storage, read>       bias:  array<f64>;
@group(0) @binding(3) var<storage, read_write> scores: array<f64>;
@group(0) @binding(4) var<uniform>             params: Params;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let N = params.n_res;
    let H = params.n_heads;
    let D = params.head_dim;
    let R = params.n_rows;

    let total = R * H * N * N;
    let idx = gid.x;
    if idx >= total { return; }

    let rh  = idx / (N * N);
    let rem = idx % (N * N);
    let j   = rem / N;
    let k   = rem % N;
    let row = rh / H;
    let h   = rh % H;

    if row >= R { return; }

    let q_base = (row * N * H + j * H + h) * D;
    let k_base = (row * N * H + k * H + h) * D;

    // Zone 1+2: f64 → df64 dot product
    var acc = df64_zero();
    for (var d = 0u; d < D; d++) {
        let q = df64_from_f64(query[q_base + d]);
        let kv = df64_from_f64(key[k_base + d]);
        acc = df64_add(acc, df64_mul(q, kv));
    }

    let scale = sqrt_df64(df64_from_f32(f32(D)));
    let score = df64_div(acc, scale);
    let bias_val = df64_from_f64(bias[h * N * N + j * N + k]);
    let biased = df64_add(score, bias_val);

    // Zone 3: df64 → f64
    let out_idx = row * H * N * N + h * N * N + j * N + k;
    scores[out_idx] = df64_to_f64(biased);
}
