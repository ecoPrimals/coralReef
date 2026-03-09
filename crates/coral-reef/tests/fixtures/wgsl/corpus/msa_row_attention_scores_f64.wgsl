// SPDX-License-Identifier: AGPL-3.0-or-later
//
// msa_row_attention_scores_f64.wgsl — MSA row attention scores with pair bias
//
// Per-sequence multi-head attention over residue positions, with additive
// pair bias from the pair representation. The pair bias is the critical
// difference from standard SDPA — it injects structural information into
// the MSA update.
//
//   scores[s, h, i, j] = sum_d Q[s,i,h,d]*K[s,j,h,d] / sqrt(d) + bias[h,i,j]
//
// query/key layout: [N_seq, N_res, H, D]
// pair_bias layout: [H, N_res, N_res]
// output layout:    [N_seq, H, N_res, N_res]
//
// Three-zone core streaming: f64 buffer I/O, df64 compute, f64 output.
//
// Absorption target: barracuda::ops::msa_row_attention_scores_f64
// Requires: df64_core.wgsl + df64_transcendentals.wgsl (prepended via compile_shader_f64)

struct Params {
    n_seq:    u32,
    n_res:    u32,
    n_heads:  u32,
    head_dim: u32,
}

@group(0) @binding(0) var<storage, read>       query:     array<f64>;
@group(0) @binding(1) var<storage, read>       key:       array<f64>;
@group(0) @binding(2) var<storage, read>       pair_bias: array<f64>;
@group(0) @binding(3) var<storage, read_write> scores:    array<f64>;
@group(0) @binding(4) var<uniform>             params:    Params;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let total = params.n_seq * params.n_heads * params.n_res * params.n_res;
    let idx = gid.x;
    if idx >= total { return; }

    let N = params.n_res;
    let H = params.n_heads;
    let D = params.head_dim;

    let s    = idx / (H * N * N);
    let rem1 = idx % (H * N * N);
    let h    = rem1 / (N * N);
    let rem2 = rem1 % (N * N);
    let i    = rem2 / N;
    let j    = rem2 % N;

    let q_base = ((s * N + i) * H + h) * D;
    let k_base = ((s * N + j) * H + h) * D;

    // Zone 1+2: f64 → df64 dot product + scale + bias
    var acc = df64_zero();
    for (var d = 0u; d < D; d++) {
        let q = df64_from_f64(query[q_base + d]);
        let k = df64_from_f64(key[k_base + d]);
        acc = df64_add(acc, df64_mul(q, k));
    }

    let scale = sqrt_df64(df64_from_f32(f32(D)));
    let scaled = df64_div(acc, scale);
    let bias = df64_from_f64(pair_bias[(h * N + i) * N + j]);
    let result = df64_add(scaled, bias);

    // Zone 3: df64 → f64
    scores[idx] = df64_to_f64(result);
}
