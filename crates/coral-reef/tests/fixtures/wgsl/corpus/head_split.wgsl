// SPDX-License-Identifier: AGPL-3.0-or-later
//
// head_split.wgsl — GPU-resident head split for MHA
//
// Reindexes [B, S, D] → [B, H, S, D/H] without matmul.
// Pure data movement: one thread per output element.
//
// The projection matmul is done SEPARATELY via validated barracuda::matmul.
// This avoids the S-03b hang caused by fusing matmul into the head-split shader.
//
// Absorption target: barracuda::ops::mha (replace project_with_head_split WGSL)

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

@group(0) @binding(0) var<storage, read> input: array<f32>;       // [B, S, D]
@group(0) @binding(1) var<storage, read_write> output: array<f32>; // [B, H, S, D/H]
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(256)
fn head_split(@builtin(global_invocation_id) gid: vec3<u32>) {
    let total = params.batch_size * params.num_heads * params.seq_len * params.head_dim;
    let idx = gid.x;
    if idx >= total { return; }

    // Decode flat index → (b, h, s, d) in output layout [B, H, S, D/H]
    let d = idx % params.head_dim;
    let s = (idx / params.head_dim) % params.seq_len;
    let h = (idx / (params.head_dim * params.seq_len)) % params.num_heads;
    let b = idx / (params.head_dim * params.seq_len * params.num_heads);

    // Source index in [B, S, D] where D = H * D/H
    let src = b * params.seq_len * params.d_model
            + s * params.d_model
            + h * params.head_dim + d;

    output[idx] = input[src];
}
