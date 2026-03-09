// SPDX-License-Identifier: AGPL-3.0-or-later
//
// head_concat.wgsl — GPU-resident head concatenation for MHA
//
// Reindexes [B, H, S, D/H] → [B, S, D] without matmul.
// Pure data movement: one thread per output element.
//
// The output projection matmul is done SEPARATELY via validated barracuda::matmul.
// This avoids the S-03b hang caused by fusing matmul into the concat shader.
//
// Absorption target: barracuda::ops::mha (replace concat_and_project WGSL)

struct Params {
    batch_size: u32,
    seq_len: u32,
    d_model: u32,
    num_heads: u32,
    head_dim: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<storage, read> input: array<f32>;       // [B, H, S, D/H]
@group(0) @binding(1) var<storage, read_write> output: array<f32>; // [B, S, D]
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(256)
fn head_concat(@builtin(global_invocation_id) gid: vec3<u32>) {
    let total = params.batch_size * params.seq_len * params.d_model;
    let idx = gid.x;
    if idx >= total { return; }

    // Decode flat index → (b, s, j) in output layout [B, S, D]
    let j = idx % params.d_model;
    let s = (idx / params.d_model) % params.seq_len;
    let b = idx / (params.d_model * params.seq_len);

    // Decode j → (h, d) where j = h * head_dim + d
    let h = j / params.head_dim;
    let d = j % params.head_dim;

    // Source index in [B, H, S, D/H]
    let src = b * params.num_heads * params.seq_len * params.head_dim
            + h * params.seq_len * params.head_dim
            + s * params.head_dim
            + d;

    output[idx] = input[src];
}
