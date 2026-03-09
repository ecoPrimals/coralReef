// SPDX-License-Identifier: AGPL-3.0-or-later
//
// msa_col_attention_scores_f64.wgsl — MSA column attention scores
//
// Per-position multi-head attention across MSA sequences (no pair bias).
// This captures sequence-level relationships at each residue position.
//
//   scores[r, h, si, sj] = sum_d Q[si,r,h,d]*K[sj,r,h,d] / sqrt(d)
//
// query/key layout: [N_seq, N_res, H, D]
// output layout:    [N_res, H, N_seq, N_seq]
//
// Three-zone core streaming: f64 buffer I/O, df64 compute, f64 output.
//
// Absorption target: barracuda::ops::msa_col_attention_scores_f64
// Requires: df64_core.wgsl + df64_transcendentals.wgsl (prepended via compile_shader_f64)

struct Params {
    n_seq:    u32,
    n_res:    u32,
    n_heads:  u32,
    head_dim: u32,
}

@group(0) @binding(0) var<storage, read>       query:  array<f64>;
@group(0) @binding(1) var<storage, read>       key:    array<f64>;
@group(0) @binding(2) var<storage, read_write> scores: array<f64>;
@group(0) @binding(3) var<uniform>             params: Params;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let S = params.n_seq;
    let N = params.n_res;
    let H = params.n_heads;
    let D = params.head_dim;

    let total = N * H * S * S;
    let idx = gid.x;
    if idx >= total { return; }

    let r    = idx / (H * S * S);
    let rem1 = idx % (H * S * S);
    let h    = rem1 / (S * S);
    let rem2 = rem1 % (S * S);
    let si   = rem2 / S;
    let sj   = rem2 % S;

    let q_base = ((si * N + r) * H + h) * D;
    let k_base = ((sj * N + r) * H + h) * D;

    // Zone 1+2: f64 → df64 dot product + scale
    var acc = df64_zero();
    for (var d = 0u; d < D; d++) {
        let q = df64_from_f64(query[q_base + d]);
        let k = df64_from_f64(key[k_base + d]);
        acc = df64_add(acc, df64_mul(q, k));
    }

    let scale = sqrt_df64(df64_from_f32(f32(D)));
    let result = df64_div(acc, scale);

    // Zone 3: df64 → f64
    scores[idx] = df64_to_f64(result);
}
