// SPDX-License-Identifier: AGPL-3.0-or-later
//
// sdpa_scores_f64.wgsl — QK^T / sqrt(d_k) with df64 core streaming
//
// Pass 1 of 3-pass f64 Scaled Dot-Product Attention.
// Three-zone core streaming: f64 buffer I/O, df64 compute, f64 output.
// Achieves ~14-digit (fp48) precision on consumer GPU FP32 cores.
//
// Layout mirrors ToadStool's sdpa_scores.wgsl for absorption:
//   1. sdpa_scores_f64  → scores[B, H, Sq, Skv]
//   2. softmax_f64      → weights[B, H, Sq, Skv]
//   3. attention_apply_f64 → output[B, H, Sq, D]
//
// Cross-spring: baseCamp Sub-02 (attention spectral), folding (Evoformer),
// WDM surrogate attention layers.
//
// Absorption target: barracuda::ops::sdpa_scores_f64
// Requires: df64_core.wgsl + df64_transcendentals.wgsl (prepended via compile_shader_f64)

struct AttentionParams {
    batch_size: u32,
    num_heads:  u32,
    q_seq_len:  u32,
    kv_seq_len: u32,
    head_dim:   u32,
    _pad0:      u32,
    _pad1:      u32,
    _pad2:      u32,
}

@group(0) @binding(0) var<storage, read>       query:  array<f64>;
@group(0) @binding(1) var<storage, read>       key:    array<f64>;
@group(0) @binding(2) var<storage, read_write> scores: array<f64>;
@group(0) @binding(3) var<uniform>             params: AttentionParams;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let total = params.batch_size * params.num_heads * params.q_seq_len * params.kv_seq_len;
    let idx   = gid.x;
    if idx >= total { return; }

    let bh    = idx / (params.q_seq_len * params.kv_seq_len);
    let rem   = idx % (params.q_seq_len * params.kv_seq_len);
    let q_pos = rem / params.kv_seq_len;
    let k_pos = rem % params.kv_seq_len;

    let b = bh / params.num_heads;
    let h = bh % params.num_heads;
    if b >= params.batch_size { return; }

    let D   = params.head_dim;
    let Sq  = params.q_seq_len;
    let Skv = params.kv_seq_len;
    let H   = params.num_heads;

    let q_base = b * H * Sq * D + h * Sq * D + q_pos * D;
    let k_base = b * H * Skv * D + h * Skv * D + k_pos * D;

    // Zone 1 (load) + Zone 2 (compute): f64 → df64 dot product
    var acc = df64_zero();
    for (var d = 0u; d < D; d++) {
        let q = df64_from_f64(query[q_base + d]);
        let k = df64_from_f64(key[k_base + d]);
        acc = df64_add(acc, df64_mul(q, k));
    }

    let scale = sqrt_df64(df64_from_f32(f32(D)));
    let result = df64_div(acc, scale);

    // Zone 3 (store): df64 → f64
    let out_idx = b * H * Sq * Skv + h * Sq * Skv + q_pos * Skv + k_pos;
    scores[out_idx] = df64_to_f64(result);
}
